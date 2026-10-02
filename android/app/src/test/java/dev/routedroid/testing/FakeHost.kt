package dev.routedroid.testing

import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.auth.Auth
import dev.routedroid.protocol.auth.BootstrapRecord
import dev.routedroid.protocol.auth.Hex
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.AuthBody
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.Hello
import dev.routedroid.protocol.message.HelloAck
import dev.routedroid.protocol.net.Ipv4Address
import dev.routedroid.protocol.net.Ipv4Prefix
import java.io.Closeable
import java.net.ServerSocket
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue

/** The host's §5 steps 1–3 on a loopback listener, as routedroidd does them. */
class FakeHost(private val secret: ByteArray = ByteArray(32) { 7 }, private val proveWith: ByteArray = secret) : Closeable {
    private val server: ServerSocket = HostEnd.listen()
    val port get() = server.localPort
    val session = "s${port}"
    private val accepted = CompletableFuture.supplyAsync { HostEnd(server.accept()) }

    fun record(): BootstrapRecord = BootstrapRecord.of(session, port, secret.copyOf())

    /** HELLO → HELLO_ACK → AUTH verified → CONFIGURE_VPN; returns the connection to keep talking on. */
    fun handshake(): HostEnd {
        val end = accepted.get(5, TimeUnit.SECONDS)
        val hello = Hello.decode(end.read(null)!!.body)
        assertEquals(session, hello.session)
        assertEquals(port, hello.devicePort)
        assertTrue(hello.app!!.startsWith("test"))
        val hostNonce = Auth.randomNonce()
        val transcript = Auth.transcript(session, port, Hex.decode(hello.clientNonce)!!, hostNonce)
        val proofs = Auth.proofs(proveWith.copyOf(), transcript)
        end.send(MessageType.HELLO_ACK, HelloAck(Protocol.VERSION, HostEnd.MTU, Hex.encode(hostNonce), Hex.encode(proofs.host)).encode())
        if (!proveWith.contentEquals(secret)) return end
        val auth = end.read(null)!!
        assertEquals(MessageType.AUTH, auth.type)
        assertTrue(Auth.matches(Auth.proofs(secret.copyOf(), transcript).android, AuthBody.decode(auth.body).androidProof))
        end.send(MessageType.CONFIGURE_VPN, CONFIG.encode())
        return end
    }

    override fun close() = server.close()

    companion object {
        val CONFIG = ConfigureVpn(
            mtu = HostEnd.MTU,
            address = Ipv4Prefix(Ipv4Address.parse("10.0.0.2")!!, 32),
            routes = listOf(Ipv4Prefix(Ipv4Address.parse("0.0.0.0")!!, 0)),
            dns = listOf(Ipv4Address.parse("10.0.0.1")!!),
            sessionName = "Routedroid",
        )
    }
}
