package dev.routedroid.phase0

import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * Phase 0 §3.4 mutual authentication, mirroring host/phase0-tunnel/src/auth.rs exactly:
 *
 * ```
 * transcript = "rd-p0-auth" 0x00 | protocol u8 | session utf8 | 0x00 | device_port u16be
 *            | "android" | client_nonce[32] | "host" | host_nonce[32]
 * host_proof    = HMAC-SHA256(secret, "host"    | transcript)
 * android_proof = HMAC-SHA256(secret, "android" | transcript)
 * ```
 *
 * Bootstrap record (80 bytes) streamed to [BootstrapProvider]:
 * `"RDB0"[4] | version u8 = 0 | reserved[3] | session[40] NUL-padded | secret[32]`.
 */
object Auth {
    const val SECRET_LEN = 32
    const val NONCE_LEN = 32
    const val PROOF_LEN = 32
    const val RECORD_LEN = 80
    const val RECORD_VERSION = 0
    private const val RECORD_SESSION_LEN = 40
    private val RECORD_MAGIC = "RDB0".toByteArray(Charsets.US_ASCII)

    class Record(val session: String, val secret: ByteArray) {
        fun wipe() = secret.fill(0)
    }

    /** Parses a record; null if the layout, version, or session are invalid. Caller owns [secret]. */
    fun parseRecord(bytes: ByteArray): Record? {
        if (bytes.size != RECORD_LEN) return null
        if (!bytes.copyOfRange(0, 4).contentEquals(RECORD_MAGIC)) return null
        if (bytes[4].toInt() != RECORD_VERSION) return null
        val sessionField = bytes.copyOfRange(8, 8 + RECORD_SESSION_LEN)
        val end = sessionField.indexOf(0).let { if (it < 0) RECORD_SESSION_LEN else it }
        if (end == 0) return null
        if (sessionField.copyOfRange(end, RECORD_SESSION_LEN).any { it.toInt() != 0 }) return null
        val session = String(sessionField, 0, end, Charsets.UTF_8)
        return Record(session, bytes.copyOfRange(48, 80))
    }

    fun transcript(protocol: Int, session: String, devicePort: Int, clientNonce: ByteArray, hostNonce: ByteArray): ByteArray {
        require(clientNonce.size == NONCE_LEN && hostNonce.size == NONCE_LEN)
        val s = session.toByteArray(Charsets.UTF_8)
        val out = java.io.ByteArrayOutputStream(64 + s.size + 2 * NONCE_LEN)
        out.write("rd-p0-auth".toByteArray(Charsets.US_ASCII)); out.write(0)
        out.write(protocol and 0xFF)
        out.write(s); out.write(0)
        out.write((devicePort ushr 8) and 0xFF); out.write(devicePort and 0xFF)
        out.write("android".toByteArray(Charsets.US_ASCII)); out.write(clientNonce)
        out.write("host".toByteArray(Charsets.US_ASCII)); out.write(hostNonce)
        return out.toByteArray()
    }

    fun proof(secret: ByteArray, role: String, transcript: ByteArray): ByteArray {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(secret, "HmacSHA256"))
        mac.update(role.toByteArray(Charsets.US_ASCII))
        mac.update(transcript)
        return mac.doFinal()
    }

    /** Constant-time check of a hex-encoded proof. */
    fun verify(secret: ByteArray, role: String, transcript: ByteArray, proofHex: String): Boolean {
        val raw = fromHex(proofHex) ?: return false
        if (raw.size != PROOF_LEN) return false
        return MessageDigest.isEqual(proof(secret, role, transcript), raw)
    }

    fun toHex(b: ByteArray): String {
        val sb = StringBuilder(b.size * 2)
        for (x in b) { sb.append("0123456789abcdef"[(x.toInt() ushr 4) and 15]); sb.append("0123456789abcdef"[x.toInt() and 15]) }
        return sb.toString()
    }

    fun fromHex(s: String): ByteArray? {
        if (s.length % 2 != 0) return null
        val out = ByteArray(s.length / 2)
        for (i in out.indices) {
            val hi = Character.digit(s[2 * i], 16); val lo = Character.digit(s[2 * i + 1], 16)
            if (hi < 0 || lo < 0) return null
            out[i] = ((hi shl 4) or lo).toByte()
        }
        return out
    }

    fun nonce32(hex: String): ByteArray? = fromHex(hex)?.takeIf { it.size == NONCE_LEN }
}
