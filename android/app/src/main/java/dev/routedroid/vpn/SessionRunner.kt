package dev.routedroid.vpn

import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.system.ErrnoException
import android.util.Log
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.protocol.message.VpnReady
import dev.routedroid.session.PendingConnection
import dev.routedroid.session.StatusStore
import dev.routedroid.transport.ChannelInput
import dev.routedroid.transport.ChannelOutput
import dev.routedroid.transport.Pumps
import dev.routedroid.transport.SocketReader
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * One session from the Negotiated handoff to Closed: CONFIGURE_VPN → establish → VPN_READY →
 * pumps → teardown. Runs on [scope]; [stop] may be called from any thread.
 */
class SessionRunner(
    private val scope: CoroutineScope,
    private val handoff: PendingConnection.Handoff,
    private val protect: (java.net.Socket) -> Boolean,
    private val newBuilder: () -> VpnService.Builder,
) {
    companion object {
        private const val TAG = "Session"
        private const val CONFIGURE_TIMEOUT_MS = 15_000L
        private const val STOP_FLUSH_MS = 500L
    }

    /** Why the app is ending the session; decides the last frame written. */
    enum class LocalStop { USER, REVOKED, DESTROYED }

    private val channel = handoff.channel
    private val output = ChannelOutput(channel)
    private val stopped = AtomicBoolean(false)
    @Volatile private var localStop: LocalStop? = null
    @Volatile private var pumps: Pumps? = null
    @Volatile private var vpnFd: ParcelFileDescriptor? = null

    /** Returns a failure description, or null for a clean end. */
    suspend fun run(): String? {
        var failure: String? = null
        try {
            failure = session()
        } catch (e: CancellationException) {
            throw e
        } catch (e: VpnFailure) {
            failure = "${e.code.wire}: ${e.message}"
            sendLast(Frame(MessageType.VPN_ERROR, e.body().encode()))
        } catch (e: IOException) {
            failure = if (localStop != null) null else "io: ${e.message ?: e.javaClass.simpleName}"
        } catch (e: ErrnoException) {
            failure = if (localStop != null) null else "errno: ${e.message}"
        } catch (e: Exception) {
            failure = "unexpected: $e"
            sendLast(Frame(MessageType.VPN_ERROR, ErrorBody(ErrorCode.INTERNAL, e.toString().take(200)).encode()))
        } finally {
            teardown()
            StatusStore.closed()
        }
        return failure
    }

    private suspend fun session(): String? {
        // protect() BEFORE establish(): once the default route points into the VPN this socket
        // must still reach loopback/adb directly. Works on a connected socket.
        if (!protect(channel.socket())) throw VpnFailure(ErrorCode.INTERNAL, "VpnService.protect() returned false")
        val input = ChannelInput(channel)
        val mtu = handoff.mtu
        StatusStore.setMtu(mtu)
        StatusStore.setState(StatusStore.State.NEGOTIATED)

        val outcome = withDeadline(CONFIGURE_TIMEOUT_MS, "CONFIGURE_VPN") { Configure.await(input, mtu) }
        val config = when (outcome) {
            is Configure.Outcome.HostStop -> return null
            is Configure.Outcome.HostError -> return "host: ${outcome.body.code}: ${outcome.body.message}"
            is Configure.Outcome.Config -> outcome.config
        }
        StatusStore.setState(StatusStore.State.CONFIGURING)
        val pfd = VpnConfigurator.establish(newBuilder(), config)
        vpnFd = pfd
        val addresses = config.addresses.map { "${it.address}/${it.prefix}" }
        StatusStore.setConfig(addresses, config.routes.map { "${it.address}/${it.prefix}" }, config.dns, config.mtu)
        output.writeFrame(Frame(MessageType.VPN_READY, VpnReady(addresses, config.mtu).encode()))
        StatusStore.setState(StatusStore.State.ACTIVE)
        Log.i(TAG, "Active: $addresses mtu=${config.mtu}")

        val p = Pumps(scope, input, output, pfd.fileDescriptor, mtu, ::closeSocket)
        pumps = p
        if (stopped.get()) return null // stop raced the handshake
        return when (val end = p.run()) {
            null -> null
            SocketReader.End.HostStop -> null
            SocketReader.End.HostClosed -> if (localStop != null) null else "host closed the connection"
            SocketReader.End.Dead -> "no frame from host for 30s"
            is SocketReader.End.HostError -> "host: ${end.body.code}: ${end.body.message}"
            is SocketReader.End.Violation -> {
                sendLast(Frame(MessageType.VPN_ERROR, ErrorBody(ErrorCode.PROTOCOL_ERROR, end.message).encode()))
                "protocol: ${end.message}"
            }
        }
    }

    /** Blocking reads have no timeout on a channel; a watchdog closes it instead. */
    private suspend fun <T> withDeadline(ms: Long, what: String, block: () -> T): T {
        var timedOut = false
        val watchdog = scope.launch { delay(ms); timedOut = true; closeSocket() }
        try {
            return block()
        } catch (e: IOException) {
            throw if (timedOut) VpnFailure(ErrorCode.PROTOCOL_ERROR, "$what timed out") else e
        } finally { watchdog.cancel() }
    }

    /** Best-effort final frame, bounded so a wedged socket cannot delay teardown. */
    private suspend fun sendLast(frame: Frame) {
        if (localStop == LocalStop.DESTROYED) return
        withTimeoutOrNull(STOP_FLUSH_MS) {
            scope.launch { runCatching { output.writeFrame(frame) } }.join()
        }
    }

    /** Idempotent. STOP (user) or VPN_ERROR (revoke) is sent first, then the socket closes. */
    fun stop(why: LocalStop) {
        if (!stopped.compareAndSet(false, true)) return
        localStop = why
        Log.i(TAG, "stop: $why")
        StatusStore.setState(StatusStore.State.STOPPING)
        pumps?.stop()
        scope.launch {
            val active = StatusStore.status.value.let { it.state == StatusStore.State.ACTIVE || it.state == StatusStore.State.STOPPING }
            val last = when (why) {
                LocalStop.USER -> Frame.empty(MessageType.STOP)
                LocalStop.REVOKED -> if (active) Frame(MessageType.VPN_ERROR, ErrorBody(ErrorCode.VPN_PERMISSION_DENIED, "VPN revoked").encode()) else Frame.empty(MessageType.STOP)
                LocalStop.DESTROYED -> null
            }
            if (last != null) sendLast(last)
            closeSocket()
        }
    }

    private fun closeSocket() {
        try { channel.close() } catch (_: IOException) {}
    }

    /** Closes the socket (ends both packet paths) and the VPN interface. */
    private fun teardown() {
        pumps?.stop()
        closeSocket()
        vpnFd?.let { try { it.close() } catch (_: IOException) {} }
        vpnFd = null
    }
}
