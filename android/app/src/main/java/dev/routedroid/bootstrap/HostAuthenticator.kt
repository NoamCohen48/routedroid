package dev.routedroid.bootstrap

import dev.routedroid.link.SessionEnd
import dev.routedroid.link.SessionEnd.Ended
import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.auth.Auth
import dev.routedroid.protocol.auth.BootstrapRecord
import dev.routedroid.protocol.auth.Hex
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.AuthBody
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.Hello
import dev.routedroid.protocol.message.HelloAck
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State
import dev.routedroid.transport.Connection
import java.io.IOException
import java.net.SocketTimeoutException

/**
 * §5 steps 1–2 on the phone, before any consent: connect to the port the record names,
 * HELLO, HELLO_ACK, verify the host's proof, AUTH. Nothing is sent to a host that has not
 * proved itself, so every failure here closes silently. The record is wiped on every path.
 */
class HostAuthenticator(private val app: String) {
    /** Blocking. Returns the negotiated MTU; throws [Ended]. */
    fun run(record: BootstrapRecord, conn: Connection): Int {
        try {
            try {
                conn.connect(record.devicePort, CONNECT_TIMEOUT_MS)
            } catch (_: IOException) {
                throw Ended(SessionEnd.HostUnreachable)
            }
            val clientNonce = Auth.randomNonce()
            conn.send(Frame(MessageType.HELLO, Hello(Protocol.VERSION, record.session, record.devicePort, Hex.encode(clientNonce), app).encode()))
            val ack = try {
                conn.within(HELLO_ACK_TIMEOUT_MS) { helloAck(conn) }
            } catch (_: SocketTimeoutException) {
                throw Ended(SessionEnd.TimedOut("HELLO_ACK"))
            }
            val hostNonce = Hex.decode(ack.hostNonce)!!
            val proofs = record.withSecret { Auth.proofs(it, Auth.transcript(record.session, record.devicePort, clientNonce, hostNonce)) }
            if (!Auth.matches(proofs.host, ack.hostProof)) throw Ended(SessionEnd.AuthFailed)
            conn.send(Frame(MessageType.AUTH, AuthBody(Hex.encode(proofs.android)).encode()))
            return ack.mtu
        } finally {
            record.wipe()
        }
    }

    /** Authenticating: HELLO_ACK, ERROR and STOP are legal (§5). */
    private fun helloAck(conn: Connection): HelloAck {
        val frame = conn.reader.frame(null) ?: throw Ended(SessionEnd.HostClosed)
        if (!Allowlist.isAllowed(Role.ANDROID, State.Authenticating, frame.type)) {
            throw Ended(SessionEnd.Violation("${frame.type} before HELLO_ACK"))
        }
        return when (frame.type) {
            MessageType.HELLO_ACK -> HelloAck.decode(frame.body)
            MessageType.ERROR -> throw Ended(ErrorBody.decode(frame.body).let { SessionEnd.HostRefused(it.code, it.message) })
            else -> throw Ended(SessionEnd.HostStopped)
        }
    }

    companion object {
        const val CONNECT_TIMEOUT_MS = 5_000
        const val HELLO_ACK_TIMEOUT_MS = 10_000L
    }
}
