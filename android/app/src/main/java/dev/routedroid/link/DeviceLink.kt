package dev.routedroid.link

import android.util.Log
import dev.routedroid.bootstrap.HostAuthenticator
import dev.routedroid.bootstrap.RecordVault
import dev.routedroid.transport.Clock
import dev.routedroid.transport.Traffic
import java.util.concurrent.ScheduledExecutorService
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The phone's one link to the PC, owned by the process. Activities and the VpnService are
 * shells: they forward launches, consent, stop and service lifecycle here, and render
 * [state]. A matched launch always wins: it supersedes whatever session was running.
 */
class DeviceLink(
    internal val authenticator: HostAuthenticator,
    /** Starts the VpnService (main thread, while the consenting activity is visible). */
    private val startVpnService: () -> Unit,
    /** Deadlines the host also keeps; counts time asleep, which is the conservative side. */
    internal val clock: Clock,
    internal val timers: ScheduledExecutorService,
    internal val uptime: Clock = Clock.UPTIME,
) {
    val vault = RecordVault(clock, timers)
    private val mutableState = MutableStateFlow<LinkState>(LinkState.Idle(null))
    val state: StateFlow<LinkState> = mutableState.asStateFlow()

    /** Counters of the current, or else the last, session. */
    @Volatile var traffic = Traffic()
        private set

    private val lock = Any()
    private var current: Session? = null
    private var host: VpnHost? = null

    /** A launch naming [session]; false, with nothing changed, unless the pending record matches. */
    fun launch(session: String): Boolean {
        val record = vault.take(session) ?: return false
        synchronized(lock) {
            val previous = current
            previous?.stop(SessionEnd.Superseded)
            val next = Session(this, record, previous)
            current = next
            traffic = next.traffic
            mutableState.value = LinkState.Authenticating
            next.start()
        }
        return true
    }

    fun consent(granted: Boolean) {
        if (current()?.consent(granted) == true) startVpnService()
    }

    fun abandon() {
        current()?.abandon()
    }

    fun stop() {
        current()?.stop(SessionEnd.UserStopped)
    }

    /** The service started; false when no session is waiting for it. */
    fun attach(host: VpnHost): Boolean = synchronized(lock) {
        this.host = host
        current?.attach(host) == true
    }

    fun revoked() {
        current()?.stop(SessionEnd.Revoked)
    }

    fun detach(host: VpnHost) {
        synchronized(lock) {
            if (this.host !== host) return
            this.host = null
            current
        }?.stop(SessionEnd.Failed("the VPN service was destroyed"))
    }

    private fun current() = synchronized(lock) { current }

    internal fun publish(session: Session, state: LinkState) = synchronized(lock) {
        if (current === session) mutableState.value = state
    }

    internal fun closed(session: Session, end: SessionEnd) {
        // Never the session id: it is what selects the record (§7.2).
        Log.i(TAG, "session ended: $end")
        val release = synchronized(lock) {
            if (current !== session) return
            current = null
            mutableState.value = LinkState.Idle(end)
            host
        }
        release?.release()
    }

    private companion object {
        const val TAG = "DeviceLink"
    }
}
