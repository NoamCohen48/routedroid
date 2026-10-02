package dev.routedroid.link

import dev.routedroid.link.SessionEnd.Ended
import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.auth.BootstrapRecord
import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.transport.Connection
import dev.routedroid.transport.PacketDevice
import dev.routedroid.transport.PacketPath
import dev.routedroid.transport.Traffic
import java.io.IOException
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/**
 * One launch from the PC, on its own thread from the matched record to Closed:
 * authenticate, await consent, await the VPN service, configure, run the packet path.
 * [stop] may come from any thread at any point; the first end recorded wins, and the
 * thread always finishes by sending that end's last frame and closing everything.
 */
internal class Session(private val link: DeviceLink, private val record: BootstrapRecord, private val previous: Session?) {
    val traffic = Traffic()
    private val conn = Connection(link.uptime)
    private val requested = AtomicReference<SessionEnd?>()
    private val signals = LinkedBlockingQueue<Signal>()
    private val thread = Thread(::run, "session")
    @Volatile private var phase: LinkState = LinkState.Authenticating
    @Volatile private var authenticated = false
    @Volatile private var authAt = 0L
    @Volatile private var path: PacketPath? = null
    @Volatile private var tun: PacketDevice? = null

    private sealed interface Signal {
        data object Consent : Signal
        class Host(val host: VpnHost) : Signal
        data object Wake : Signal
    }

    fun start() = thread.start()

    fun join(ms: Long) = thread.join(ms)

    /** Main thread, with the consent result. True when the VPN service must be started now. */
    fun consent(granted: Boolean): Boolean {
        if (phase != LinkState.AwaitingConsent) return false
        when {
            !granted -> stop(SessionEnd.ConsentDenied)
            link.clock.now() - authAt > CONSENT_LIMIT_MS -> stop(SessionEnd.ConsentExpired)
            else -> {
                enter(LinkState.Configuring)
                signals.offer(Signal.Consent)
                return true
            }
        }
        return false
    }

    fun attach(host: VpnHost): Boolean {
        if (phase != LinkState.Configuring || requested.get() != null) return false
        signals.offer(Signal.Host(host))
        return true
    }

    /** The bootstrap screen went away for good before the user consented. */
    fun abandon() = when (phase) {
        LinkState.Authenticating -> stop(SessionEnd.UserStopped)
        LinkState.AwaitingConsent -> stop(SessionEnd.ConsentDenied)
        else -> Unit
    }

    fun stop(why: SessionEnd) {
        if (!requested.compareAndSet(null, why)) return
        link.publish(this, LinkState.Stopping)
        link.timers.schedule(::closeAll, FORCE_CLOSE_MS, TimeUnit.MILLISECONDS)
        signals.offer(Signal.Wake)
        // An authenticated host is still owed a last frame; before that, just cut it off.
        if (authenticated) conn.shutdownInput() else conn.close()
        path?.stop(why)
    }

    private fun run() {
        previous?.join(PREVIOUS_JOIN_MS)
        val end = try {
            phases()
        } catch (e: Ended) {
            e.end
        } catch (e: FrameException) {
            SessionEnd.Violation("bad frame: ${e.message}")
        } catch (e: BodyException) {
            SessionEnd.Violation(e.message ?: "bad body")
        } catch (_: IOException) {
            SessionEnd.HostClosed
        } catch (e: Exception) {
            SessionEnd.Failed(e.toString())
        }
        requested.compareAndSet(null, end)
        val final = requested.get()!!
        if (authenticated && path == null) {
            link.timers.schedule(::closeAll, FORCE_CLOSE_MS, TimeUnit.MILLISECONDS)
            SessionEnd.lastFrame(final)?.let { runCatching { conn.send(it) } }
        }
        closeAll()
        link.closed(this, final)
    }

    private fun phases(): SessionEnd {
        val mtu = link.authenticator.run(record, conn)
        authAt = link.clock.now()
        authenticated = true
        requested.get()?.let { return it }
        enter(LinkState.AwaitingConsent)
        await<Signal.Consent>(authAt + CONSENT_LIMIT_MS) ?: return requested.get() ?: SessionEnd.ConsentExpired
        val host = await<Signal.Host>(link.clock.now() + HOST_ATTACH_MS)?.host
            ?: return requested.get() ?: SessionEnd.Failed("the VPN service did not start")
        val config = Configure(conn, host, mtu, authAt + CONSENT_LIMIT_MS, link.clock).awaitConfig()
        val device = host.establish(config)
        tun = device
        requested.get()?.let { return it }
        Configure.ready(conn, config)
        enter(LinkState.Active(config))
        val p = PacketPath(conn, device, mtu, traffic, link.uptime)
        path = p
        requested.get()?.let(p::stop)
        return p.run()
    }

    private inline fun <reified T : Signal> await(deadline: Long): T? {
        while (requested.get() == null) {
            val left = deadline - link.clock.now()
            if (left <= 0) return null
            val s = signals.poll(left, TimeUnit.MILLISECONDS) ?: return null
            if (s is T) return s
        }
        return null
    }

    private fun enter(state: LinkState) {
        phase = state
        if (requested.get() == null) link.publish(this, state)
    }

    /** Idempotent; also the backstop a stop schedules in case a write is wedged. */
    private fun closeAll() {
        conn.close()
        tun?.close()
    }

    companion object {
        /** §5 step 5 allows 120 s after AUTH; keep a margin for establish and transit. */
        const val CONSENT_LIMIT_MS = Protocol.CONFIGURE_DEADLINE_MS - 5_000
        const val HOST_ATTACH_MS = 10_000L
        const val FORCE_CLOSE_MS = 2_000L
        const val PREVIOUS_JOIN_MS = 3_000L
    }
}
