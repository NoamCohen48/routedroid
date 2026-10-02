package dev.routedroid.link

import android.util.Log
import dev.routedroid.link.SessionEnd.Ended
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.VpnReady
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State
import dev.routedroid.transport.Clock
import dev.routedroid.transport.Connection
import java.net.SocketTimeoutException

/**
 * §5 steps 3–5 after consent: read the CONFIGURE_VPN the host sent right after AUTH (it
 * has been waiting in the socket while the user decided), and make sure the host is still
 * there and still waiting before the VPN goes up.
 */
internal class Configure(
    private val conn: Connection,
    private val host: VpnHost,
    private val mtu: Int,
    /** When the host stops waiting for VPN_READY, on [clock]. */
    private val deadline: Long,
    private val clock: Clock,
) {
    fun awaitConfig(): ConfigureVpn {
        // Before establish(), so the socket keeps reaching adbd directly. Loopback never
        // routes into a VPN, so a refusal here is logged rather than fatal.
        if (!host.protect(conn.socket)) Log.w(TAG, "protect() refused the host socket")
        val frame = read(State.Negotiated) ?: throw Ended(SessionEnd.HostClosed)
        val config = try {
            ConfigureVpn.decode(frame.body, mtu)
        } catch (e: BodyException) {
            throw Ended(SessionEnd.ConfigRejected(e.message ?: "invalid CONFIGURE_VPN"))
        }
        // Anything the host sent since is STOP, ERROR or a violation: all of them end it.
        if (conn.pending() > 0) {
            read(State.Configuring)
            throw Ended(SessionEnd.HostClosed)
        }
        if (clock.now() >= deadline) throw Ended(SessionEnd.ConsentExpired)
        return config
    }

    /** One frame legal in [state]; STOP and ERROR end the session here, null at EOF. */
    private fun read(state: State): Frame? {
        val left = deadline - clock.now()
        if (left <= 0) throw Ended(SessionEnd.ConsentExpired)
        val frame = try {
            conn.within(left) { conn.reader.frame(mtu) }
        } catch (_: SocketTimeoutException) {
            throw Ended(SessionEnd.TimedOut("CONFIGURE_VPN"))
        } ?: return null
        if (!Allowlist.isAllowed(Role.ANDROID, state, frame.type)) throw Ended(SessionEnd.Violation("${frame.type} while $state"))
        when (frame.type) {
            MessageType.STOP -> throw Ended(SessionEnd.HostStopped)
            MessageType.ERROR -> throw Ended(ErrorBody.decode(frame.body).let { SessionEnd.HostRefused(it.code, it.message) })
            else -> return frame
        }
    }

    companion object {
        private const val TAG = "Configure"

        /**
         * VPN_READY: Active from here (§5 step 5). A PING the host sent before reading this
         * is accepted by the packet path as Active traffic; only a broken host does that.
         */
        fun ready(conn: Connection, config: ConfigureVpn) =
            conn.send(Frame(MessageType.VPN_READY, VpnReady(listOf(config.address), config.mtu).encode()))
    }
}
