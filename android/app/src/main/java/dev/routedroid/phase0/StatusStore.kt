package dev.routedroid.phase0

import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update

/**
 * Process-wide status for the UI. Low-rate fields go through a StateFlow; packet counters are
 * plain atomics so the hot path never allocates or emits per packet (the UI polls them).
 */
object StatusStore {
    enum class State { IDLE, CONNECTING, NEGOTIATED, CONFIGURING, ACTIVE, STOPPING, CLOSED }

    data class Status(
        val state: State = State.IDLE,
        val session: String? = null,
        val devicePort: Int? = null,
        val addresses: List<String> = emptyList(),
        val routes: List<String> = emptyList(),
        val dns: List<String> = emptyList(),
        val mtu: Int? = null,
        val lastError: String? = null,
    )

    private val _status = MutableStateFlow(Status())
    val status: StateFlow<Status> = _status

    /** Packets read from the VPN and sent to the host. */
    val packetsOut = AtomicLong()
    /** Packets received from the host and written into the VPN. */
    val packetsIn = AtomicLong()
    val bytesOut = AtomicLong()
    val bytesIn = AtomicLong()
    /** Locally-read packets that were dropped (non-IPv4, oversize). */
    val droppedOut = AtomicLong()

    fun reset(session: String, devicePort: Int) {
        packetsOut.set(0); packetsIn.set(0); bytesOut.set(0); bytesIn.set(0); droppedOut.set(0)
        _status.value = Status(state = State.CONNECTING, session = session, devicePort = devicePort)
    }

    fun setState(state: State) = _status.update { it.copy(state = state) }

    fun setConfig(addresses: List<String>, routes: List<String>, dns: List<String>, mtu: Int) =
        _status.update { it.copy(addresses = addresses, routes = routes, dns = dns, mtu = mtu) }

    fun setMtu(mtu: Int) = _status.update { it.copy(mtu = mtu) }

    fun setError(message: String) = _status.update { it.copy(lastError = message) }

    fun closed() = _status.update { it.copy(state = State.CLOSED) }
}
