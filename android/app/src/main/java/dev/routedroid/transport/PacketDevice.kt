package dev.routedroid.transport

import java.io.IOException

/** The VPN interface as the packet path sees it: whole IPv4 packets in and out. */
interface PacketDevice {
    /** Blocks for one packet into `buf[off, off+len)`; -1 once [wake] was called. */
    @Throws(IOException::class)
    fun read(buf: ByteArray, off: Int, len: Int): Int

    /** Writes one whole packet; a short write is an [IOException]. */
    @Throws(IOException::class)
    fun write(buf: ByteArray, off: Int, len: Int)

    /** Makes the current and every later [read] return -1. Any thread, idempotent. */
    fun wake()

    /** Releases the interface; only after every reader and writer has returned. */
    fun close()
}
