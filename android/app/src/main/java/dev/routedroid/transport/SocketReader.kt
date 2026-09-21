package dev.routedroid.transport

import android.util.Log
import dev.routedroid.protocol.Ipv4
import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State
import dev.routedroid.session.StatusStore
import kotlinx.coroutines.channels.Channel

/**
 * socket -> validate -> rxFilled while Active. PING is answered via the tx queue; STOP, ERROR,
 * a violation, or EOF end the loop with a verdict the session turns into VPN_ERROR/teardown.
 */
class SocketReader(private val input: ChannelInput, private val mtu: Int, private val keepalive: Keepalive) {
    companion object { private const val TAG = "SocketReader" }

    sealed class End {
        object HostStop : End()
        object HostClosed : End()
        object Dead : End()
        class HostError(val body: ErrorBody) : End()
        class Violation(val message: String) : End()
    }

    suspend fun run(running: () -> Boolean, free: Channel<Slot>, filled: Channel<Slot>, tx: Channel<Slot>): End {
        while (running()) {
            val (type, len) = try {
                input.readHeader(mtu) ?: return End.HostClosed
            } catch (e: FrameException) {
                return End.Violation("bad frame: ${e.message}")
            }
            keepalive.received()
            if (!Allowlist.isAllowed(Role.ANDROID, State.Active, type)) {
                return End.Violation("${type.wireName} is not legal while Active")
            }
            when (type) {
                MessageType.IP_PACKET -> {
                    val slot = free.receive()
                    input.readFully(slot.buf, Protocol.HEADER_LEN, len)
                    val reason = Ipv4.reject(slot.buf, Protocol.HEADER_LEN, len)
                    if (reason != null) {
                        // §6: an IPv4 check failure is a drop, not a violation.
                        StatusStore.droppedIn.incrementAndGet()
                        free.send(slot)
                        continue
                    }
                    slot.len = len
                    filled.send(slot)
                }
                MessageType.PING -> if (!tx.trySend(Slot.PONG).isSuccess) Log.w(TAG, "tx queue full; PONG dropped")
                MessageType.PONG -> Unit
                MessageType.STOP -> return End.HostStop
                MessageType.ERROR -> {
                    val body = ByteArray(len).also { input.readFully(it) }
                    return try { End.HostError(ErrorBody.decode(body)) } catch (e: BodyException) { End.Violation("ERROR body: ${e.message}") }
                }
                else -> return End.Violation("${type.wireName} while Active")
            }
        }
        return End.HostClosed
    }
}
