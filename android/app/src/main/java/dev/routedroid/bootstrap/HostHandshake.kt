package dev.routedroid.bootstrap

import android.util.Log
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
import dev.routedroid.transport.ChannelInput
import dev.routedroid.transport.ChannelOutput
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.nio.channels.SocketChannel

/**
 * Android half of §5 steps 1–2, run by BootstrapActivity BEFORE any VPN consent:
 * connect → HELLO → HELLO_ACK → verify host_proof → AUTH. The secret is wiped on every exit
 * path. On success the caller owns the channel (state Negotiated); the host sends
 * CONFIGURE_VPN on it next.
 */
object HostHandshake {
    private const val TAG = "HostHandshake"
    private const val CONNECT_TIMEOUT_MS = 5_000
    private const val ACK_TIMEOUT_MS = 10_000L

    class Failure(message: String) : Exception(message)

    class Result(val channel: SocketChannel, val mtu: Int)

    /** Blocking; call off the main thread. Consumes (wipes) [record]. */
    fun run(record: BootstrapRecord, devicePort: Int): Result {
        var ch: SocketChannel? = null
        try {
            val clientNonce = Auth.randomNonce()
            val sock = SocketChannel.open()
            ch = sock
            sock.socket().tcpNoDelay = true
            // Explicit 127.0.0.1: adbd's reverse listener is IPv4-only and getLoopbackAddress() may be ::1.
            val loopback = InetAddress.getByAddress(byteArrayOf(127, 0, 0, 1))
            sock.socket().connect(InetSocketAddress(loopback, devicePort), CONNECT_TIMEOUT_MS)
            val out = ChannelOutput(sock)

            val hello = Hello(Protocol.VERSION, record.session, devicePort, Hex.encode(clientNonce))
            out.writeFrame(Frame(MessageType.HELLO, hello.encode()))

            val ack = withDeadline(sock, ACK_TIMEOUT_MS, "HELLO_ACK") { readHelloAck(ChannelInput(sock)) }
            if (ack.protocol != Protocol.VERSION) throw Failure("HELLO_ACK protocol ${ack.protocol}")
            val hostNonce = Hex.decode(ack.hostNonce) ?: throw Failure("HELLO_ACK host_nonce")
            val transcript = Auth.transcript(record.session, devicePort, clientNonce, hostNonce)
            if (!Auth.verify(record.secret, Auth.ROLE_HOST, transcript, ack.hostProof)) {
                // §5 step 2: close silently, no hint to a guesser.
                throw Failure("host_proof does not verify (wrong or stale secret)")
            }
            val androidProof = Hex.encode(Auth.proof(record.secret, Auth.ROLE_ANDROID, transcript))
            record.wipe()
            out.writeFrame(Frame(MessageType.AUTH, AuthBody(androidProof).encode()))
            Log.i(TAG, "host authenticated for session=${record.session}")
            ch = null
            return Result(sock, ack.mtu)
        } finally {
            record.wipe()
            ch?.let { runCatching { it.close() } }
        }
    }

    /** Blocking reads have no timeout on a channel; a watchdog closes it instead. */
    private fun <T> withDeadline(sock: SocketChannel, ms: Long, what: String, block: () -> T): T {
        var timedOut = false
        val watchdog = Thread {
            try { Thread.sleep(ms) } catch (_: InterruptedException) { return@Thread }
            timedOut = true; runCatching { sock.close() }
        }.apply { isDaemon = true; start() }
        try {
            return block()
        } catch (e: IOException) {
            throw if (timedOut) Failure("$what timed out") else e
        } finally { watchdog.interrupt() }
    }

    /** Authenticating: only HELLO_ACK, ERROR and STOP are legal (§5). */
    private fun readHelloAck(input: ChannelInput): HelloAck {
        val frame = input.readFrame(Protocol.DEFAULT_MTU) ?: throw Failure("host closed before HELLO_ACK")
        if (!Allowlist.isAllowed(Role.ANDROID, State.Authenticating, frame.type)) {
            throw Failure("unexpected ${frame.type.wireName} before HELLO_ACK")
        }
        return when (frame.type) {
            MessageType.HELLO_ACK -> HelloAck.decode(frame.body)
            MessageType.ERROR -> throw Failure("host refused: ${ErrorBody.decode(frame.body).let { "${it.code}: ${it.message}" }}")
            else -> throw Failure("host sent STOP before HELLO_ACK")
        }
    }
}
