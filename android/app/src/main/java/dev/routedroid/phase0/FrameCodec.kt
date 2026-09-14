package dev.routedroid.phase0

/**
 * Phase 0 frame codec (protocol/phase0-draft.md). Pure Kotlin, no Android imports, so it runs
 * in JVM unit tests.
 *
 * Header (8 bytes, network byte order):
 *
 * ```
 * u32 body_length   // excludes header; 0 allowed only for PING/PONG/STOP
 * u8  version       // must be 0
 * u8  message_type
 * u16 flags         // must be 0
 * ```
 *
 * All limits are enforced on the header BEFORE any body allocation happens.
 */
object FrameCodec {
    const val HEADER_LENGTH = 8
    const val PROTOCOL_VERSION = 0
    const val CONTROL_BODY_MAX = 65536
    const val DEFAULT_MTU = 1400

    /** Draft: IP_PACKET bodies must be `> 20` bytes. */
    const val IP_PACKET_BODY_MIN_EXCLUSIVE = 20
    const val IPV4_HEADER_MIN = 20
    const val IPV4_ABSOLUTE_MAX = 65535

    object Type {
        const val HELLO = 0x01
        const val HELLO_ACK = 0x02
        const val CONFIGURE_VPN = 0x03
        const val VPN_READY = 0x04
        const val VPN_ERROR = 0x05
        const val IP_PACKET = 0x10
        const val PING = 0x20
        const val PONG = 0x21
        const val STOP = 0x30
        const val ERROR = 0x7F

        fun name(type: Int): String = when (type) {
            HELLO -> "HELLO"
            HELLO_ACK -> "HELLO_ACK"
            CONFIGURE_VPN -> "CONFIGURE_VPN"
            VPN_READY -> "VPN_READY"
            VPN_ERROR -> "VPN_ERROR"
            IP_PACKET -> "IP_PACKET"
            PING -> "PING"
            PONG -> "PONG"
            STOP -> "STOP"
            ERROR -> "ERROR"
            else -> "0x%02X".format(type)
        }

        fun isJsonControl(type: Int): Boolean = when (type) {
            HELLO, HELLO_ACK, CONFIGURE_VPN, VPN_READY, VPN_ERROR, ERROR -> true
            else -> false
        }

        fun isEmptyControl(type: Int): Boolean = when (type) {
            PING, PONG, STOP -> true
            else -> false
        }
    }

    /** Any violation of the draft that must close the session. */
    class FrameException(message: String) : Exception(message)

    /** Decoded header. `bodyLength` is already validated against the per-type limit. */
    data class Header(val bodyLength: Int, val type: Int)

    /**
     * Writes a header for `type` with `bodyLength` into `dst` at `offset`. Does not validate
     * limits; callers encode only bodies they constructed themselves.
     */
    fun encodeHeader(dst: ByteArray, offset: Int, bodyLength: Int, type: Int) {
        require(bodyLength >= 0) { "negative body length" }
        require(type in 0..0xFF) { "type out of range" }
        require(dst.size - offset >= HEADER_LENGTH) { "destination too small for header" }
        dst[offset] = (bodyLength ushr 24).toByte()
        dst[offset + 1] = (bodyLength ushr 16).toByte()
        dst[offset + 2] = (bodyLength ushr 8).toByte()
        dst[offset + 3] = bodyLength.toByte()
        dst[offset + 4] = PROTOCOL_VERSION.toByte()
        dst[offset + 5] = type.toByte()
        dst[offset + 6] = 0
        dst[offset + 7] = 0
    }

    /** Allocating convenience for control frames (rare, off the packet hot path). */
    fun encodeControl(type: Int, body: ByteArray): ByteArray {
        if (Type.isEmptyControl(type)) {
            require(body.isEmpty()) { "${Type.name(type)} must have an empty body" }
        } else {
            require(body.size <= CONTROL_BODY_MAX) { "control body exceeds $CONTROL_BODY_MAX" }
        }
        val out = ByteArray(HEADER_LENGTH + body.size)
        encodeHeader(out, 0, body.size, type)
        System.arraycopy(body, 0, out, HEADER_LENGTH, body.size)
        return out
    }

    fun encodeEmpty(type: Int): ByteArray = encodeControl(type, ByteArray(0))

    /**
     * Parses and validates the 8-byte header at `src[offset]`.
     *
     * `mtu` is the negotiated packet limit (use [DEFAULT_MTU] before HELLO_ACK). Throws
     * [FrameException] for: wrong version, nonzero flags, unknown type, body_length above the
     * type's limit, or a nonzero body on PING/PONG/STOP. Nothing is allocated based on the
     * untrusted length, so a 0xFFFFFFFF length is rejected without any allocation.
     */
    fun decodeHeader(src: ByteArray, offset: Int, mtu: Int): Header {
        require(src.size - offset >= HEADER_LENGTH) { "source too small for header" }
        val bodyLength: Long =
            ((src[offset].toLong() and 0xFF) shl 24) or
                ((src[offset + 1].toLong() and 0xFF) shl 16) or
                ((src[offset + 2].toLong() and 0xFF) shl 8) or
                (src[offset + 3].toLong() and 0xFF)
        val version = src[offset + 4].toInt() and 0xFF
        val type = src[offset + 5].toInt() and 0xFF
        val flags = ((src[offset + 6].toInt() and 0xFF) shl 8) or (src[offset + 7].toInt() and 0xFF)

        if (version != PROTOCOL_VERSION) {
            throw FrameException("unsupported version $version")
        }
        if (flags != 0) {
            throw FrameException("nonzero flags 0x%04X".format(flags))
        }
        when {
            type == Type.IP_PACKET -> {
                if (bodyLength <= IP_PACKET_BODY_MIN_EXCLUSIVE) {
                    throw FrameException("IP_PACKET body $bodyLength must be > $IP_PACKET_BODY_MIN_EXCLUSIVE")
                }
                if (bodyLength > mtu) {
                    throw FrameException("IP_PACKET body $bodyLength exceeds mtu $mtu")
                }
            }
            Type.isEmptyControl(type) -> {
                if (bodyLength != 0L) {
                    throw FrameException("${Type.name(type)} must have empty body, got $bodyLength")
                }
            }
            Type.isJsonControl(type) -> {
                if (bodyLength == 0L) {
                    throw FrameException("${Type.name(type)} must have a body")
                }
                if (bodyLength > CONTROL_BODY_MAX) {
                    throw FrameException("${Type.name(type)} body $bodyLength exceeds $CONTROL_BODY_MAX")
                }
            }
            else -> throw FrameException("unknown message type ${Type.name(type)}")
        }
        return Header(bodyLength.toInt(), type)
    }

    /**
     * Draft rule, applied on both ends before injection:
     * version nibble == 4, IHL >= 5, total_length == body_length, total_length >= IHL*4.
     * Returns null when valid, otherwise a reason string.
     */
    fun checkIpv4(buf: ByteArray, offset: Int, length: Int): String? {
        if (length < IPV4_HEADER_MIN) return "packet shorter than minimum IPv4 header ($length)"
        if (length > IPV4_ABSOLUTE_MAX) return "packet longer than 65535 ($length)"
        val first = buf[offset].toInt() and 0xFF
        val version = first ushr 4
        if (version != 4) return "IP version $version is not 4"
        val ihl = first and 0x0F
        if (ihl < 5) return "IHL $ihl < 5"
        val totalLength = ((buf[offset + 2].toInt() and 0xFF) shl 8) or (buf[offset + 3].toInt() and 0xFF)
        if (totalLength != length) return "total_length $totalLength != body_length $length"
        if (totalLength < ihl * 4) return "total_length $totalLength < IHL*4 (${ihl * 4})"
        return null
    }

    fun validateIpv4(buf: ByteArray, offset: Int, length: Int) {
        val reason = checkIpv4(buf, offset, length)
        if (reason != null) throw FrameException("invalid IPv4 packet: $reason")
    }
}
