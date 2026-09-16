package dev.routedroid.phase0

import android.util.Log
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.SocketChannel
import java.security.SecureRandom
import org.json.JSONException
import org.json.JSONObject

/**
 * Client half of the §3.4 handshake, run by [BootstrapActivity] BEFORE any VPN consent:
 * connect -> HELLO(client_nonce) -> HELLO_ACK(host_nonce, host_proof) -> verify -> AUTH(android_proof).
 * The secret is wiped on every exit path. On success the caller owns the channel, which the
 * host will use next for CONFIGURE_VPN.
 */
object HostHandshake {
    private const val TAG = "Phase0Handshake"
    private const val CONNECT_TIMEOUT_MS = 5_000
    private const val ACK_TIMEOUT_MS = 10_000L

    class Failure(message: String) : Exception(message)

    class Result(val channel: SocketChannel, val mtu: Int)

    /** Blocking; call off the main thread. Consumes (wipes) [record]. */
    fun run(record: Auth.Record, devicePort: Int): Result {
        var ch: SocketChannel? = null
        try {
            val clientNonce = ByteArray(Auth.NONCE_LEN).also { SecureRandom().nextBytes(it) }
            val sock = SocketChannel.open()
            ch = sock
            sock.socket().tcpNoDelay = true
            // Explicit 127.0.0.1 (adbd's reverse listener is IPv4-only; getLoopbackAddress() may be ::1).
            sock.socket().connect(InetSocketAddress(InetAddress.getByAddress(byteArrayOf(127, 0, 0, 1)), devicePort), CONNECT_TIMEOUT_MS)

            val hello = JSONObject()
                .put("protocol", FrameCodec.PROTOCOL_VERSION)
                .put("session", record.session)
                .put("device_port", devicePort)
                .put("client_nonce", Auth.toHex(clientNonce))
            write(sock, FrameCodec.encodeControl(FrameCodec.Type.HELLO, hello.toString().toByteArray(Charsets.UTF_8)))

            // Blocking read bounded by a watchdog that closes the channel.
            var timedOut = false
            val watchdog = Thread {
                try { Thread.sleep(ACK_TIMEOUT_MS) } catch (_: InterruptedException) { return@Thread }
                timedOut = true; runCatching { sock.close() }
            }.apply { isDaemon = true; start() }
            val ack = try {
                readControl(sock, FrameCodec.Type.HELLO_ACK)
            } catch (e: IOException) {
                throw if (timedOut) Failure("HELLO_ACK timed out") else e
            } finally { watchdog.interrupt() }

            if (ack.optInt("protocol", -1) != FrameCodec.PROTOCOL_VERSION) throw Failure("HELLO_ACK protocol")
            val mtu = ack.optInt("mtu", -1)
            if (mtu !in 68..FrameCodec.IPV4_ABSOLUTE_MAX) throw Failure("HELLO_ACK mtu $mtu")
            val hostNonce = Auth.nonce32(ack.optString("host_nonce")) ?: throw Failure("HELLO_ACK host_nonce")
            val transcript = Auth.transcript(FrameCodec.PROTOCOL_VERSION, record.session, devicePort, clientNonce, hostNonce)
            if (!Auth.verify(record.secret, "host", transcript, ack.optString("host_proof"))) {
                throw Failure("host_proof does not verify (wrong or stale secret)")
            }
            val androidProof = Auth.toHex(Auth.proof(record.secret, "android", transcript))
            record.wipe()
            write(sock, FrameCodec.encodeControl(FrameCodec.Type.AUTH, JSONObject().put("android_proof", androidProof).toString().toByteArray(Charsets.UTF_8)))
            Log.i(TAG, "host authenticated for session=${record.session}")
            ch = null
            return Result(sock, mtu)
        } finally {
            record.wipe()
            ch?.let { runCatching { it.close() } }
        }
    }

    private fun write(ch: SocketChannel, frame: ByteArray) {
        val bb = ByteBuffer.wrap(frame)
        while (bb.hasRemaining()) ch.write(bb)
    }

    private fun readFully(ch: SocketChannel, buf: ByteArray) {
        val bb = ByteBuffer.wrap(buf)
        while (bb.hasRemaining()) { if (ch.read(bb) < 0) throw IOException("peer closed connection") }
    }

    /** PING is answered; STOP/ERROR/other types fail the handshake. */
    private fun readControl(ch: SocketChannel, expected: Int): JSONObject {
        val header = ByteArray(FrameCodec.HEADER_LENGTH)
        while (true) {
            readFully(ch, header)
            val h = FrameCodec.decodeHeader(header, 0, FrameCodec.DEFAULT_MTU)
            when (h.type) {
                expected -> {
                    val body = ByteArray(h.bodyLength); readFully(ch, body)
                    return try { JSONObject(String(body, Charsets.UTF_8)) } catch (e: JSONException) { throw Failure("bad JSON in ${FrameCodec.Type.name(expected)}") }
                }
                FrameCodec.Type.PING -> write(ch, FrameCodec.encodeEmpty(FrameCodec.Type.PONG))
                FrameCodec.Type.PONG -> Unit
                FrameCodec.Type.ERROR -> { val body = ByteArray(h.bodyLength); readFully(ch, body); throw Failure("host ERROR: ${String(body, Charsets.UTF_8)}") }
                else -> throw Failure("unexpected ${FrameCodec.Type.name(h.type)} before ${FrameCodec.Type.name(expected)}")
            }
        }
    }
}
