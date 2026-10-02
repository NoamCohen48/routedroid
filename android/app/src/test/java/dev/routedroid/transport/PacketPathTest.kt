package dev.routedroid.transport

import dev.routedroid.link.SessionEnd
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.testing.FakeTun
import dev.routedroid.testing.HostEnd
import dev.routedroid.testing.ipv4
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

/** The Active phase over a real loopback socket and an in-memory VPN interface. */
class PacketPathTest {
    private val tun = FakeTun()
    private val traffic = Traffic()
    private lateinit var conn: Connection
    private lateinit var host: HostEnd
    private lateinit var path: PacketPath
    private lateinit var result: CompletableFuture<SessionEnd>

    @Before fun connect() {
        HostEnd.listen().use { server ->
            conn = Connection().apply { connect(server.localPort, 2_000) }
            host = HostEnd(server.accept())
        }
        path = PacketPath(conn, tun, HostEnd.MTU, traffic)
        result = CompletableFuture.supplyAsync { path.run().also { conn.close() } }
    }

    @After fun close() {
        conn.close()
        host.close()
    }

    private fun end(): SessionEnd = result.get(5, TimeUnit.SECONDS)

    private fun vpnError(f: Frame?): ErrorBody {
        assertEquals(MessageType.VPN_ERROR, f?.type)
        return ErrorBody.decode(f!!.body)
    }

    @Test fun packetsFlowBothWaysAndPingIsAnswered() {
        val out = ipv4(60, tag = 1)
        tun.inject(out)
        assertArrayEquals(out, host.read()!!.body)
        val inbound = ipv4(1400, tag = 2)
        host.send(MessageType.IP_PACKET, inbound)
        assertArrayEquals(inbound, tun.nextDelivered())
        host.send(MessageType.PING)
        assertEquals(MessageType.PONG, host.read()!!.type)
        host.send(MessageType.STOP)
        assertEquals(SessionEnd.HostStopped, end())
        assertNull("nothing after the host's STOP", host.read())
        assertEquals(1L, traffic.packetsOut.get())
        assertEquals(1L, traffic.packetsIn.get())
    }

    @Test fun localStopSendsStopLast() {
        tun.inject(ipv4(40))
        assertEquals(MessageType.IP_PACKET, host.read()!!.type)
        path.stop(SessionEnd.UserStopped)
        assertEquals(SessionEnd.UserStopped, end())
        assertEquals(MessageType.STOP, host.read()!!.type)
        assertNull(host.read())
    }

    /** A-1.1: the host stops reading, the tx side fills up, and stop must still end everything. */
    @Test fun stopWithAWedgedWriterReturns() {
        // Far more than the socket buffers hold: the writer blocks and the queue fills.
        repeat(20_000) { tun.inject(ipv4(1400)) }
        val deadline = System.nanoTime() + 5_000_000_000
        // Wedged: the queue is (nearly) full and stays where it is.
        var size = -1
        while (System.nanoTime() < deadline && (path.tx.size < path.txPool.depth - 2 || path.tx.size != size)) {
            size = path.tx.size
            Thread.sleep(300)
        }
        assert(path.tx.size >= path.txPool.depth - 2 && path.tx.size == size) { "writer never blocked: ${path.tx.size}" }
        val started = System.nanoTime()
        path.stop(SessionEnd.UserStopped)
        assertEquals(SessionEnd.UserStopped, end())
        val ms = (System.nanoTime() - started) / 1_000_000
        assert(ms < 3_000) { "stop took $ms ms" }
    }

    @Test fun aFailingInterfaceEndsWithVpnError() {
        tun.failWrites = true
        host.send(MessageType.IP_PACKET, ipv4(40))
        assert(end() is SessionEnd.Failed)
        assertEquals(ErrorCode.INTERNAL.wire, vpnError(host.readUntil(MessageType.VPN_ERROR)).code)
        assertNull(host.read())
    }

    @Test fun anIllegalFrameIsAViolationReported() {
        host.send(MessageType.HELLO, "{}".toByteArray())
        assert(end() is SessionEnd.Violation)
        assertEquals(ErrorCode.PROTOCOL_ERROR.wire, vpnError(host.read()).code)
    }

    @Test fun badPacketsAreDroppedAndCounted() {
        host.send(MessageType.IP_PACKET, ipv4(40).also { it[0] = 0x65 })
        val good = ipv4(30)
        host.send(MessageType.IP_PACKET, good)
        assertArrayEquals(good, tun.nextDelivered())
        assertEquals(1L, traffic.droppedIn.get())
        tun.inject(ByteArray(1401).also { it[0] = 0x45 })
        tun.inject(ByteArray(40).also { it[0] = 0x60 })
        val sent = ipv4(25)
        tun.inject(sent)
        assertArrayEquals(sent, host.read()!!.body)
        assertEquals(1L, traffic.oversizeOut.get())
        assertEquals(1L, traffic.droppedOut.get())
        path.stop(SessionEnd.Revoked)
        assertEquals(ErrorCode.VPN_PERMISSION_DENIED.wire, vpnError(host.read()).code)
    }

    @Test fun hostErrorEndsWithItsCode() {
        host.send(MessageType.ERROR, ErrorBody.of(ErrorCode.INTERNAL, "bye").encode())
        assertEquals(SessionEnd.HostRefused("internal", "bye"), end())
        assertNull(host.read())
    }

    @Test fun hostEofIsHostClosed() {
        host.socket.shutdownOutput()
        assertEquals(SessionEnd.HostClosed, end())
    }
}
