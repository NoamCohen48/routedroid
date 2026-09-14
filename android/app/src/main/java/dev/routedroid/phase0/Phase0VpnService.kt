package dev.routedroid.phase0

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import android.util.Log
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.DataInputStream
import java.io.FileDescriptor
import java.io.IOException
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.SocketTimeoutException
import java.nio.channels.SocketChannel
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ClosedReceiveChannelException
import kotlinx.coroutines.channels.ClosedSendChannelException
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

/**
 * Phase 0 foreground VpnService: one loopback TCP stream (via `adb reverse`) carrying framed
 * control messages and raw IPv4 packets, per protocol/phase0-draft.md.
 *
 * Lifecycle: [ACTION_START] with session/device_port extras -> handshake -> establish VPN ->
 * two packet pumps until STOP/socket close/[ACTION_STOP]/onRevoke -> teardown -> stopSelf.
 */
class Phase0VpnService : VpnService() {

    companion object {
        private const val TAG = "Phase0Vpn"
        const val ACTION_START = "dev.routedroid.phase0.START"
        const val ACTION_STOP = "dev.routedroid.phase0.STOP"
        const val EXTRA_SESSION = "session"
        const val EXTRA_DEVICE_PORT = "device_port"

        private const val CHANNEL_ID = "phase0_tunnel"
        private const val NOTIFICATION_ID = 1

        /** Bounded in-flight packets per direction; the reader suspends when this is full. */
        private const val QUEUE_DEPTH = 256
        /** Slots must cover the queue plus one being filled and one being written. */
        private const val POOL_SIZE = QUEUE_DEPTH + 2
        private const val CONNECT_TIMEOUT_MS = 5_000
        private const val HANDSHAKE_TIMEOUT_MS = 15_000
        private const val VPN_POLL_TIMEOUT_MS = 500
    }

    /** One preallocated frame buffer: header at [0, 8), body at [8, 8 + len). */
    private class Slot(capacity: Int, val pooled: Boolean) {
        val buf = ByteArray(FrameCodec.HEADER_LENGTH + capacity)
        var len = 0
        val frameLength: Int get() = FrameCodec.HEADER_LENGTH + len
    }

    private class ProtocolError(message: String) : Exception(message)
    private class VpnFailure(val code: String, message: String) : Exception(message)

    private data class VpnConfig(
        val mtu: Int,
        val addresses: List<Pair<InetAddress, Int>>,
        val routes: List<Pair<InetAddress, Int>>,
        val dns: List<InetAddress>,
        val sessionName: String,
    )

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var sessionJob: Job? = null

    // Session resources; only touched by the session coroutine and teardown().
    @Volatile private var channel: SocketChannel? = null
    @Volatile private var txOutput: OutputStream? = null
    @Volatile private var vpnFd: ParcelFileDescriptor? = null
    @Volatile private var running = false
    private val stopRequested = AtomicBoolean(false)
    /** True only for deliberate stops (UI, revoke, host STOP, destroy); IO errors after this are expected. */
    @Volatile private var cleanStop = false
    @Volatile private var sendStopOnTeardown = false

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                val session = intent.getStringExtra(EXTRA_SESSION)
                val port = intent.getIntExtra(EXTRA_DEVICE_PORT, -1)
                startForegroundCompat()
                if (session.isNullOrEmpty() || port !in 1..65535) {
                    Log.e(TAG, "START without valid extras; stopping")
                    StatusStore.setError("service started without session/device_port")
                    finishService()
                    return START_NOT_STICKY
                }
                if (sessionJob?.isActive == true) {
                    Log.w(TAG, "session already running; ignoring second START")
                    return START_NOT_STICKY
                }
                stopRequested.set(false)
                cleanStop = false
                StatusStore.reset(session, port)
                sessionJob = scope.launch { runSession(session, port) }
            }
            ACTION_STOP -> { cleanStop = true; requestStop("stop requested from UI", sendStop = true) }
            else -> {
                // Restart without an intent (START_STICKY is not used) or unknown action.
                if (sessionJob?.isActive != true) finishService()
            }
        }
        return START_NOT_STICKY
    }

    override fun onRevoke() {
        Log.w(TAG, "VPN revoked by system/user")
        StatusStore.setError("VPN permission revoked")
        cleanStop = true
        requestStop("revoked", sendStop = true)
    }

    override fun onDestroy() {
        cleanStop = true
        requestStop("service destroyed", sendStop = false)
        scope.cancel()
        super.onDestroy()
    }

    // ---------------------------------------------------------------- session

    private suspend fun runSession(session: String, devicePort: Int) {
        var failure: String? = null
        try {
            // SocketChannel.open() creates the fd eagerly (java.net.Socket() does not), so
            // protect() can be applied BEFORE connect and, crucially, BEFORE establish():
            // once the default route points into the VPN this socket must still reach
            // loopback/adb directly.
            val ch = SocketChannel.open()
            channel = ch
            val sock = ch.socket()
            if (!protect(sock)) throw VpnFailure("protect_failed", "VpnService.protect() returned false")
            sock.tcpNoDelay = true
            sock.connect(InetSocketAddress(InetAddress.getLoopbackAddress(), devicePort), CONNECT_TIMEOUT_MS)
            running = true

            val input = DataInputStream(BufferedInputStream(sock.getInputStream(), 64 * 1024))
            val output = BufferedOutputStream(sock.getOutputStream(), 64 * 1024)
            txOutput = output

            // Blocking reads: bound the handshake with a socket read timeout, not a
            // coroutine timeout (there is no suspension point to cancel at).
            sock.soTimeout = HANDSHAKE_TIMEOUT_MS
            val (mtu, config) = try {
                handshake(input, output, session, devicePort)
            } catch (e: SocketTimeoutException) {
                throw ProtocolError("handshake timed out")
            }
            sock.soTimeout = 0

            val pfd = try {
                establishVpn(config)
            } catch (e: VpnFailure) {
                sendControl(output, FrameCodec.Type.VPN_ERROR, errorBody(e.code, e.message ?: e.code))
                throw e
            }
            vpnFd = pfd
            StatusStore.setConfig(
                addresses = config.addresses.map { "${it.first.hostAddress}/${it.second}" },
                routes = config.routes.map { "${it.first.hostAddress}/${it.second}" },
                dns = config.dns.map { it.hostAddress ?: it.toString() },
                mtu = config.mtu,
            )
            sendControl(output, FrameCodec.Type.VPN_READY, JSONObject().apply {
                put("addresses", JSONArray(config.addresses.map { "${it.first.hostAddress}/${it.second}" }))
                put("mtu", config.mtu)
            }.toString().toByteArray(Charsets.UTF_8))
            StatusStore.setState(StatusStore.State.ACTIVE)
            sendStopOnTeardown = true

            pumpUntilClosed(input, output, pfd.fileDescriptor, mtu)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ProtocolError) {
            failure = "protocol: ${e.message}"
        } catch (e: FrameCodec.FrameException) {
            failure = "frame: ${e.message}"
        } catch (e: VpnFailure) {
            failure = "vpn ${e.code}: ${e.message}"
        } catch (e: IOException) {
            failure = if (cleanStop) null else "io: ${e.message ?: e.javaClass.simpleName}"
        } catch (e: ErrnoException) {
            failure = if (cleanStop) null else "errno: ${e.message}"
        } catch (e: Exception) {
            failure = "unexpected: $e"
        } finally {
            if (failure != null) {
                Log.e(TAG, "session ended: $failure")
                StatusStore.setError(failure)
            } else {
                Log.i(TAG, "session ended cleanly")
            }
            teardown()
            StatusStore.closed()
            finishService()
        }
    }

    /** Returns (negotiated packet mtu, vpn config). Throws on any protocol violation. */
    private fun handshake(input: DataInputStream, output: OutputStream, session: String, devicePort: Int): Pair<Int, VpnConfig> {
        sendControl(output, FrameCodec.Type.HELLO, JSONObject().apply {
            put("protocol", FrameCodec.PROTOCOL_VERSION)
            put("session", session)
            put("device_port", devicePort)
        }.toString().toByteArray(Charsets.UTF_8))

        // Expect HELLO_ACK.
        val ack = readControl(input, output, FrameCodec.DEFAULT_MTU, FrameCodec.Type.HELLO_ACK)
        val ackProtocol = ack.optInt("protocol", -1)
        if (ackProtocol != FrameCodec.PROTOCOL_VERSION) throw ProtocolError("HELLO_ACK protocol $ackProtocol")
        val mtu = ack.optInt("mtu", -1)
        if (mtu !in 68..FrameCodec.IPV4_ABSOLUTE_MAX) throw ProtocolError("HELLO_ACK mtu $mtu out of range")
        StatusStore.setMtu(mtu)
        StatusStore.setState(StatusStore.State.NEGOTIATED)

        // Expect CONFIGURE_VPN.
        val cfgJson = readControl(input, output, mtu, FrameCodec.Type.CONFIGURE_VPN)
        StatusStore.setState(StatusStore.State.CONFIGURING)
        val config = try {
            parseConfig(cfgJson, mtu)
        } catch (e: VpnFailure) {
            sendControl(output, FrameCodec.Type.VPN_ERROR, errorBody(e.code, e.message ?: e.code))
            throw e
        }
        return mtu to config
    }

    /**
     * Reads frames until one of `expected` arrives. PING is answered inline; STOP/ERROR close;
     * IP_PACKET before Active closes; anything else is out-of-state and closes.
     */
    private fun readControl(input: DataInputStream, output: OutputStream, mtu: Int, expected: Int): JSONObject {
        val header = ByteArray(FrameCodec.HEADER_LENGTH)
        while (true) {
            input.readFully(header)
            val h = FrameCodec.decodeHeader(header, 0, mtu)
            when (h.type) {
                expected -> {
                    val body = ByteArray(h.bodyLength) // bounded by CONTROL_BODY_MAX already
                    input.readFully(body)
                    return try {
                        JSONObject(String(body, Charsets.UTF_8))
                    } catch (e: JSONException) {
                        throw ProtocolError("${FrameCodec.Type.name(expected)} body is not a JSON object")
                    }
                }
                FrameCodec.Type.PING -> writeFully(output, FrameCodec.encodeEmpty(FrameCodec.Type.PONG))
                FrameCodec.Type.PONG -> Unit
                FrameCodec.Type.STOP -> throw ProtocolError("host sent STOP during handshake")
                FrameCodec.Type.ERROR -> {
                    val body = ByteArray(h.bodyLength)
                    input.readFully(body)
                    throw ProtocolError("host ERROR: ${String(body, Charsets.UTF_8)}")
                }
                else -> throw ProtocolError("unexpected ${FrameCodec.Type.name(h.type)} while waiting for ${FrameCodec.Type.name(expected)}")
            }
        }
    }

    private fun parseConfig(json: JSONObject, negotiatedMtu: Int): VpnConfig {
        fun bad(msg: String): Nothing = throw VpnFailure("invalid_config", msg)
        val mtu = json.optInt("mtu", -1)
        if (mtu !in 68..FrameCodec.IPV4_ABSOLUTE_MAX) bad("mtu $mtu out of range")
        // The frame limit is the HELLO_ACK mtu; an interface MTU above it could emit packets we
        // are not allowed to frame, so reject that combination.
        if (mtu > negotiatedMtu) bad("CONFIGURE_VPN mtu $mtu exceeds negotiated $negotiatedMtu")

        fun parsePrefixed(arr: JSONArray?, what: String, maxPrefix: Int): List<Pair<InetAddress, Int>> {
            if (arr == null) bad("missing $what")
            val out = ArrayList<Pair<InetAddress, Int>>(arr.length())
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: bad("$what[$i] is not an object")
                val addr = parseIpv4(o.optString("address", ""), "$what[$i].address")
                val prefix = o.optInt("prefix", -1)
                if (prefix !in 0..maxPrefix) bad("$what[$i].prefix $prefix out of range")
                out += addr to prefix
            }
            return out
        }
        val addresses = parsePrefixed(json.optJSONArray("addresses"), "addresses", 32)
        if (addresses.isEmpty()) bad("addresses is empty")
        val routes = parsePrefixed(json.optJSONArray("routes"), "routes", 32)
        val dnsArr = json.optJSONArray("dns") ?: JSONArray()
        val dns = (0 until dnsArr.length()).map { parseIpv4(dnsArr.optString(it, ""), "dns[$it]") }
        val sessionName = json.optString("session_name", "Routedroid Phase 0").ifEmpty { "Routedroid Phase 0" }
        return VpnConfig(mtu, addresses, routes, dns, sessionName)
    }

    private fun parseIpv4(text: String, what: String): InetAddress {
        // Strict dotted-quad; never resolve hostnames.
        val parts = text.split('.')
        if (parts.size != 4) throw VpnFailure("invalid_config", "$what '$text' is not dotted-quad IPv4")
        val bytes = ByteArray(4)
        for (i in 0 until 4) {
            val v = parts[i].toIntOrNull()
            if (v == null || v !in 0..255 || parts[i].isEmpty() || parts[i].length > 3) {
                throw VpnFailure("invalid_config", "$what '$text' is not dotted-quad IPv4")
            }
            bytes[i] = v.toByte()
        }
        return InetAddress.getByAddress(bytes)
    }

    private fun establishVpn(config: VpnConfig): ParcelFileDescriptor {
        val builder = Builder()
        try {
            builder.setSession(config.sessionName)
            builder.setMtu(config.mtu)
            for ((addr, prefix) in config.addresses) builder.addAddress(addr, prefix) // in given order
            for ((addr, prefix) in config.routes) builder.addRoute(addr, prefix)
            for (d in config.dns) builder.addDnsServer(d)
            builder.setBlocking(true)
            // Deliberately no allowBypass(): architecture.md §7.
        } catch (e: IllegalArgumentException) {
            throw VpnFailure("invalid_config", "builder rejected config: ${e.message}")
        }
        val pfd = try {
            builder.establish()
        } catch (e: SecurityException) {
            throw VpnFailure("vpn_permission_denied", "establish: ${e.message}")
        } catch (e: IllegalStateException) {
            throw VpnFailure("vpn_establish_failed", "establish: ${e.message}")
        }
        return pfd ?: throw VpnFailure("vpn_permission_denied", "establish() returned null (not prepared or revoked)")
    }

    // ---------------------------------------------------------------- pumps

    /**
     * Runs the four pump coroutines. Returns when the host sends STOP, when the socket closes,
     * or when any pump fails (the failure is rethrown after the others are stopped).
     */
    private suspend fun pumpUntilClosed(input: DataInputStream, output: OutputStream, tun: FileDescriptor, mtu: Int) {
        // VPN -> socket direction (tx). Extra byte lets the VPN reader detect an oversize packet.
        val txFree = Channel<Slot>(POOL_SIZE)
        val txFilled = Channel<Slot>(QUEUE_DEPTH)
        repeat(POOL_SIZE) { txFree.trySend(Slot(mtu + 1, pooled = true)) }
        // socket -> VPN direction (rx).
        val rxFree = Channel<Slot>(POOL_SIZE)
        val rxFilled = Channel<Slot>(QUEUE_DEPTH)
        repeat(POOL_SIZE) { rxFree.trySend(Slot(mtu, pooled = true)) }
        // Constant PONG frame, never returned to a pool.
        val pongSlot = Slot(0, pooled = false).also {
            FrameCodec.encodeHeader(it.buf, 0, 0, FrameCodec.Type.PONG)
        }

        var firstFailure: Throwable? = null
        val failLock = Any()
        fun fail(t: Throwable) {
            synchronized(failLock) { if (firstFailure == null) firstFailure = t }
            requestStop("pump: ${t.message}", sendStop = false)
        }

        val jobs = listOf(
            scope.launch {
                try { vpnReader(tun, mtu, txFree, txFilled) } catch (t: Throwable) { if (running) fail(t) }
                finally { txFilled.close() }
            },
            scope.launch {
                try { socketWriter(output, txFilled, txFree) } catch (t: Throwable) { if (running) fail(t) }
            },
            scope.launch {
                try { socketReader(input, mtu, rxFree, rxFilled, txFilled, pongSlot) } catch (t: Throwable) { if (running) fail(t) }
                finally { rxFilled.close(); requestStop("socket reader ended", sendStop = false) }
            },
            scope.launch {
                try { vpnWriter(tun, rxFilled, rxFree) } catch (t: Throwable) { if (running) fail(t) }
            },
        )
        jobs.joinAll()
        firstFailure?.let { throw it }
    }

    /** VPN fd read -> frame -> txFilled. Suspends on txFree when the writer is behind. */
    private suspend fun vpnReader(tun: FileDescriptor, mtu: Int, free: Channel<Slot>, filled: Channel<Slot>) {
        val pollfd = arrayOf(StructPollfd().apply { fd = tun; events = OsConstants.POLLIN.toShort() })
        while (running) {
            val slot = free.receive()
            var n: Int
            while (true) {
                if (!running) { free.trySend(slot); return }
                pollfd[0].revents = 0
                val ready = try {
                    Os.poll(pollfd, VPN_POLL_TIMEOUT_MS)
                } catch (e: ErrnoException) {
                    if (e.errno == OsConstants.EINTR) continue else throw e
                }
                if (ready == 0) continue
                if ((pollfd[0].revents.toInt() and (OsConstants.POLLERR or OsConstants.POLLHUP or OsConstants.POLLNVAL)) != 0) {
                    throw IOException("VPN fd poll error revents=${pollfd[0].revents}")
                }
                n = try {
                    Os.read(tun, slot.buf, FrameCodec.HEADER_LENGTH, mtu + 1)
                } catch (e: ErrnoException) {
                    if (e.errno == OsConstants.EINTR || e.errno == OsConstants.EAGAIN) continue else throw e
                }
                break
            }
            if (n <= 0) throw IOException("VPN fd read returned $n")
            if (n > mtu) {
                // The interface MTU should prevent this; drop rather than send an illegal frame.
                StatusStore.droppedOut.incrementAndGet()
                free.send(slot)
                continue
            }
            if (FrameCodec.checkIpv4(slot.buf, FrameCodec.HEADER_LENGTH, n) != null) {
                // Not IPv4 (e.g. stray IPv6/ND). Locally-read, so drop instead of closing.
                StatusStore.droppedOut.incrementAndGet()
                free.send(slot)
                continue
            }
            slot.len = n
            FrameCodec.encodeHeader(slot.buf, 0, n, FrameCodec.Type.IP_PACKET)
            filled.send(slot)
        }
    }

    /** txFilled -> socket. Whole frame per write, explicit flush per frame; returns slots. */
    private suspend fun socketWriter(output: OutputStream, filled: Channel<Slot>, free: Channel<Slot>) {
        for (slot in filled) {
            synchronized(output) {
                output.write(slot.buf, 0, slot.frameLength)
                output.flush()
            }
            if (slot.pooled) {
                StatusStore.packetsOut.incrementAndGet()
                StatusStore.bytesOut.addAndGet(slot.len.toLong())
                free.send(slot)
            }
        }
    }

    /** socket -> validate -> rxFilled; PING -> PONG via txFilled; STOP/ERROR end the loop. */
    private suspend fun socketReader(
        input: DataInputStream,
        mtu: Int,
        free: Channel<Slot>,
        filled: Channel<Slot>,
        tx: Channel<Slot>,
        pongSlot: Slot,
    ) {
        val header = ByteArray(FrameCodec.HEADER_LENGTH)
        while (running) {
            input.readFully(header)
            val h = FrameCodec.decodeHeader(header, 0, mtu)
            when (h.type) {
                FrameCodec.Type.IP_PACKET -> {
                    val slot = free.receive()
                    input.readFully(slot.buf, FrameCodec.HEADER_LENGTH, h.bodyLength)
                    try {
                        FrameCodec.validateIpv4(slot.buf, FrameCodec.HEADER_LENGTH, h.bodyLength)
                    } catch (e: FrameCodec.FrameException) {
                        free.trySend(slot)
                        throw e
                    }
                    slot.len = h.bodyLength
                    filled.send(slot)
                }
                FrameCodec.Type.PING -> try { tx.send(pongSlot) } catch (_: ClosedSendChannelException) { return }
                FrameCodec.Type.PONG -> Unit
                FrameCodec.Type.STOP -> {
                    Log.i(TAG, "host sent STOP")
                    sendStopOnTeardown = false
                    cleanStop = true
                    return
                }
                FrameCodec.Type.ERROR -> {
                    val body = ByteArray(h.bodyLength)
                    input.readFully(body)
                    sendStopOnTeardown = false
                    throw ProtocolError("host ERROR: ${String(body, Charsets.UTF_8)}")
                }
                else -> throw ProtocolError("out-of-state ${FrameCodec.Type.name(h.type)} while Active")
            }
        }
    }

    /** rxFilled -> VPN fd. One whole packet per write; short write is fatal; EINTR(0) retries. */
    private suspend fun vpnWriter(tun: FileDescriptor, filled: Channel<Slot>, free: Channel<Slot>) {
        try {
            for (slot in filled) {
                val written: Int
                while (true) {
                    val w = try {
                        Os.write(tun, slot.buf, FrameCodec.HEADER_LENGTH, slot.len)
                    } catch (e: ErrnoException) {
                        // EINTR means the kernel transferred nothing; retrying is safe.
                        if (e.errno == OsConstants.EINTR) continue else throw e
                    }
                    written = w
                    break
                }
                if (written != slot.len) {
                    throw IOException("short VPN write: $written of ${slot.len} bytes; packet path reset")
                }
                StatusStore.packetsIn.incrementAndGet()
                StatusStore.bytesIn.addAndGet(slot.len.toLong())
                free.send(slot)
            }
        } catch (_: ClosedReceiveChannelException) {
            // rxFilled closed by the socket reader: normal end.
        }
    }

    // ---------------------------------------------------------------- teardown

    private fun requestStop(reason: String, sendStop: Boolean) {
        if (!stopRequested.compareAndSet(false, true)) return
        Log.i(TAG, "stop requested: $reason")
        StatusStore.setState(StatusStore.State.STOPPING)
        if (sendStop && sendStopOnTeardown) {
            // Best-effort STOP to the host before the socket closes. Done off the caller's
            // thread; the socket close in teardown() bounds it.
            scope.launch {
                try {
                    txOutput?.let { writeFully(it, FrameCodec.encodeEmpty(FrameCodec.Type.STOP)) }
                } catch (_: IOException) {
                }
                closeSocketQuietly()
            }
        } else {
            closeSocketQuietly()
        }
        running = false
        if (sessionJob?.isActive != true) {
            // No session coroutine to run teardown; do it here.
            teardown()
            finishService()
        }
    }

    private fun closeSocketQuietly() {
        try { channel?.close() } catch (_: IOException) {}
    }

    /** Idempotent. Closes the socket, waits briefly for pumps, closes the VPN fd. */
    private fun teardown() {
        running = false
        closeSocketQuietly()
        channel = null
        txOutput = null
        val pfd = vpnFd
        vpnFd = null
        if (pfd != null) {
            try { pfd.close() } catch (_: IOException) {}
        }
    }

    private fun finishService() {
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    // ---------------------------------------------------------------- io helpers

    private fun sendControl(output: OutputStream, type: Int, body: ByteArray) {
        writeFully(output, FrameCodec.encodeControl(type, body))
    }

    private fun writeFully(output: OutputStream, frame: ByteArray) {
        synchronized(output) {
            output.write(frame)
            output.flush()
        }
    }

    private fun errorBody(code: String, message: String): ByteArray =
        JSONObject().put("code", code).put("message", message).toString().toByteArray(Charsets.UTF_8)

    // ---------------------------------------------------------------- notification

    private fun createNotificationChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, getString(R.string.notification_channel_name), NotificationManager.IMPORTANCE_LOW)
        )
    }

    private fun buildNotification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(getString(R.string.notification_title))
            .setContentText("adb reverse packet tunnel (Phase 0)")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }

    private fun startForegroundCompat() {
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }
}
