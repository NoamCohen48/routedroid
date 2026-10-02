package dev.routedroid.transport

import dev.routedroid.link.SessionEnd
import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.FrameHeader
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.net.Ipv4Packet
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State

/** The loops of [PacketPath], one per thread. Each returns when the path ends. */
internal class Pumps(private val p: PacketPath) {
    private val header = Protocol.HEADER_LEN

    fun tunReader() {
        while (p.ending() == null) {
            val slot = p.txPool.take()
            val n = p.tun.read(slot.buf, header, p.mtu + 1)
            when {
                n < 0 -> return p.txPool.give(slot)
                n > p.mtu -> { p.traffic.oversizeOut.incrementAndGet(); p.txPool.give(slot) }
                Ipv4Packet.reject(slot.buf, header, n) != null -> { p.traffic.droppedOut.incrementAndGet(); p.txPool.give(slot) }
                else -> {
                    FrameHeader.write(slot.buf, 0, MessageType.IP_PACKET, n)
                    slot.len = header + n
                    p.tx.offer(slot)
                }
            }
        }
    }

    fun socketWriter() {
        while (true) {
            when (val item = p.tx.take()) {
                is Slot -> {
                    p.conn.write(item.buf, 0, item.len)
                    p.traffic.packetsOut.incrementAndGet()
                    p.traffic.bytesOut.addAndGet((item.len - header).toLong())
                    p.txPool.give(item)
                }
                TxItem.Ping, TxItem.Pong -> {
                    p.sentControl(item)
                    val frame = if (item == TxItem.Ping) PING else PONG
                    p.conn.write(frame, 0, frame.size)
                }
                TxItem.End -> {
                    // §5 step 7: the last frame is the last thing written, after no queued data.
                    SessionEnd.lastFrame(p.ending()!!)?.let(p.conn::send)
                    return
                }
            }
            p.keepalive.sent(p.clock.now())
        }
    }

    fun socketReader() {
        val reader = p.conn.reader
        while (reader.next(p.mtu)) {
            p.keepalive.received(p.clock.now())
            val type = reader.type
            if (!Allowlist.isAllowed(Role.ANDROID, State.Active, type)) return p.stop(SessionEnd.Violation("$type while Active"))
            when (type) {
                MessageType.IP_PACKET -> packet(reader.bodyLength)
                MessageType.PING -> p.requestPong()
                MessageType.PONG -> Unit
                MessageType.STOP -> return p.stop(SessionEnd.HostStopped)
                else -> return p.stop(hostError(reader.body()))
            }
        }
        p.stop(SessionEnd.HostClosed)
    }

    private fun packet(len: Int) {
        val slot = p.rxPool.take()
        slot.buffer.clear()
        p.conn.reader.readBody(slot.buffer)
        if (Ipv4Packet.reject(slot.buf, 0, len) != null) {
            // §6: a packet that fails the checks is dropped, not a violation.
            p.traffic.droppedIn.incrementAndGet()
            p.rxPool.give(slot)
            return
        }
        slot.len = len
        p.rx.offer(slot)
    }

    fun tunWriter() {
        while (true) {
            val slot = p.rx.take()
            p.tun.write(slot.buf, 0, slot.len)
            p.traffic.packetsIn.incrementAndGet()
            p.traffic.bytesIn.addAndGet(slot.len.toLong())
            p.rxPool.give(slot)
        }
    }

    fun keepalive() {
        while (true) {
            when (p.keepalive.check(p.clock.now())) {
                Keepalive.Verdict.DEAD -> return p.stop(SessionEnd.HostSilent)
                Keepalive.Verdict.PING -> p.requestPing()
                Keepalive.Verdict.NONE -> Unit
            }
            Thread.sleep(p.keepalive.nextCheckIn(p.clock.now()))
        }
    }

    private companion object {
        val PING = Frame(MessageType.PING).encode()
        val PONG = Frame(MessageType.PONG).encode()

        fun hostError(body: ByteArray): SessionEnd = try {
            ErrorBody.decode(body).let { SessionEnd.HostRefused(it.code, it.message) }
        } catch (e: BodyException) {
            SessionEnd.Violation("ERROR body: ${e.message}")
        }
    }
}
