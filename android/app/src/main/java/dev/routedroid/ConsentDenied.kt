package dev.routedroid

import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.session.PendingConnection
import dev.routedroid.session.StatusStore
import dev.routedroid.transport.ChannelOutput
import kotlin.concurrent.thread

/**
 * The user declined the VPN consent while the host is waiting in Configuring: tell it
 * (VPN_ERROR `vpn_permission_denied`, §4.6) and close, so `routedroid start` fails fast
 * instead of waiting for its timeout.
 */
object ConsentDenied {
    fun report(session: String) {
        StatusStore.setError("VPN consent denied")
        StatusStore.closed()
        val handoff = PendingConnection.take(session) ?: return
        thread(name = "consent-denied", isDaemon = true) {
            try {
                val body = ErrorBody(ErrorCode.VPN_PERMISSION_DENIED, "user declined the VPN consent")
                ChannelOutput(handoff.channel).writeFrame(Frame(MessageType.VPN_ERROR, body.encode()))
            } catch (_: Exception) {
            } finally {
                runCatching { handoff.channel.close() }
            }
        }
    }
}
