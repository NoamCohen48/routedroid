package dev.routedroid.link

import dev.routedroid.bootstrap.HostAuthenticator
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.VpnReady
import dev.routedroid.testing.FakeHost
import dev.routedroid.testing.FakeTun
import dev.routedroid.testing.HostEnd
import dev.routedroid.transport.Clock
import dev.routedroid.transport.PacketDevice
import java.net.Socket
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger
import kotlin.concurrent.thread
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** A whole session through [DeviceLink], against a host that really authenticates. */
class DeviceLinkTest {
    @Volatile private var skew = 0L
    private val tun = FakeTun()
    private val established = AtomicInteger()
    private val released = AtomicInteger()
    private val vpn = object : VpnHost {
        override fun protect(socket: Socket) = true
        override fun establish(config: ConfigureVpn): PacketDevice = tun.also { established.incrementAndGet() }
        override fun release() { released.incrementAndGet() }
    }
    private val link: DeviceLink = DeviceLink(
        authenticator = HostAuthenticator("test 1.0"),
        startVpnService = { thread { link.attach(vpn) } },
        clock = Clock { System.nanoTime() / 1_000_000 + skew },
        timers = Executors.newSingleThreadScheduledExecutor(),
    )
    private val hosts = mutableListOf<FakeHost>()

    @After fun close() = hosts.forEach(FakeHost::close)

    private fun host(proveWith: ByteArray? = null) =
        (if (proveWith == null) FakeHost() else FakeHost(proveWith = proveWith)).also { hosts += it; link.vault.put(it.record()) }

    private fun await(what: (LinkState) -> Boolean): LinkState = runBlocking { withTimeout(5_000) { link.state.first(what) } }

    private fun ended(): SessionEnd? = (await { it is LinkState.Idle } as LinkState.Idle).last

    private fun vpnErrorCode(end: HostEnd): String = end.readUntil(MessageType.VPN_ERROR)!!.let {
        assertEquals(MessageType.VPN_ERROR, it.type)
        ErrorBody.decode(it.body).code
    }

    @Test fun consentThenActiveThenUserStop() {
        val h = host()
        assertTrue(link.launch(h.session))
        val end = h.handshake()
        await { it == LinkState.AwaitingConsent }
        link.consent(true)
        val ready = VpnReady.decode(end.read()!!.also { assertEquals(MessageType.VPN_READY, it.type) }.body)
        assertEquals("10.0.0.2/32", ready.addresses.single().toString())
        assertEquals(FakeHost.CONFIG, (await { it is LinkState.Active } as LinkState.Active).config)
        link.stop()
        assertEquals(MessageType.STOP, end.readUntil(MessageType.STOP)!!.type)
        assertNull(end.read())
        assertEquals(SessionEnd.UserStopped, ended())
        assertEquals(1, released.get())
        assertTrue(tun.closed)
    }

    @Test fun aLaunchForAnotherSessionChangesNothing() {
        val h = host()
        assertFalse(link.launch("someone-else"))
        assertEquals(LinkState.Idle(null), link.state.value)
        assertTrue("the record survives a wrong guess", link.launch(h.session))
        h.handshake()
        await { it == LinkState.AwaitingConsent }
        link.stop()
    }

    @Test fun declinedConsentIsReported() {
        val h = host()
        link.launch(h.session)
        val end = h.handshake()
        await { it == LinkState.AwaitingConsent }
        link.consent(false)
        assertEquals("vpn_permission_denied", vpnErrorCode(end))
        assertEquals(SessionEnd.ConsentDenied, ended())
        assertEquals(0, established.get())
    }

    @Test fun lateConsentNeverEstablishes() {
        val h = host()
        link.launch(h.session)
        val end = h.handshake()
        await { it == LinkState.AwaitingConsent }
        skew = Session.CONSENT_LIMIT_MS + 1
        link.consent(true)
        assertEquals(SessionEnd.ConsentExpired, ended())
        assertEquals("consent_timeout", vpnErrorCode(end))
        assertEquals(0, established.get())
    }

    @Test fun aHostThatGaveUpMeanwhileIsNotConfigured() {
        val h = host()
        link.launch(h.session)
        val end = h.handshake()
        await { it == LinkState.AwaitingConsent }
        end.send(MessageType.STOP)
        Thread.sleep(100)
        link.consent(true)
        assertEquals(SessionEnd.HostStopped, ended())
        assertEquals(0, established.get())
    }

    @Test fun aHostWithoutTheSecretGetsNothing() {
        val h = host(proveWith = ByteArray(32) { 9 })
        link.launch(h.session)
        val end = h.handshake()
        assertEquals(SessionEnd.AuthFailed, ended())
        assertNull("silence, not AUTH", end.read(null))
    }

    @Test fun aNewLaunchSupersedesTheRunningSession() {
        val first = host()
        link.launch(first.session)
        val firstEnd = first.handshake()
        await { it == LinkState.AwaitingConsent }
        val second = host()
        assertTrue(link.launch(second.session))
        assertEquals("internal", vpnErrorCode(firstEnd))
        second.handshake()
        await { it == LinkState.AwaitingConsent }
        link.stop()
        assertEquals(SessionEnd.UserStopped, ended())
    }

    @Test fun anUnreachableHostEndsQuietly() {
        val h = host()
        h.close()
        link.launch(h.session)
        assertEquals(SessionEnd.HostUnreachable, ended())
    }
}
