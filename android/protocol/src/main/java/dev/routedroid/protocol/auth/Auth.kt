package dev.routedroid.protocol.auth

import dev.routedroid.protocol.Protocol
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * Mutual proof (§7.3).
 *
 *     transcript    = "routedroid-auth-v1" 0x00 | protocol u8 | session | 0x00 | port u16be
 *                   | "android" | client_nonce[32] | "host" | host_nonce[32]
 *     host_proof    = HMAC-SHA256(secret, "host"    || transcript)
 *     android_proof = HMAC-SHA256(secret, "android" || transcript)
 */
object Auth {
    const val DOMAIN = "routedroid-auth-v1"
    const val SECRET_LEN = 32
    const val NONCE_LEN = 32
    const val PROOF_LEN = 32
    const val ROLE_HOST = "host"
    const val ROLE_ANDROID = "android"

    private val rng = SecureRandom()

    fun randomNonce(): ByteArray = ByteArray(NONCE_LEN).also(rng::nextBytes)

    fun transcript(session: String, devicePort: Int, clientNonce: ByteArray, hostNonce: ByteArray): ByteArray {
        require(clientNonce.size == NONCE_LEN && hostNonce.size == NONCE_LEN)
        val out = ByteArrayOutputStream(96 + session.length)
        out.write(DOMAIN.toByteArray(Charsets.US_ASCII)); out.write(0)
        out.write(Protocol.VERSION)
        out.write(session.toByteArray(Charsets.UTF_8)); out.write(0)
        out.write(devicePort ushr 8); out.write(devicePort and 0xFF)
        out.write(ROLE_ANDROID.toByteArray(Charsets.US_ASCII)); out.write(clientNonce)
        out.write(ROLE_HOST.toByteArray(Charsets.US_ASCII)); out.write(hostNonce)
        return out.toByteArray()
    }

    fun proof(secret: ByteArray, role: String, transcript: ByteArray): ByteArray {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(secret, "HmacSHA256"))
        mac.update(role.toByteArray(Charsets.US_ASCII))
        mac.update(transcript)
        return mac.doFinal()
    }

    /** Constant-time comparison; a malformed [receivedHex] simply fails. */
    fun verify(secret: ByteArray, role: String, transcript: ByteArray, receivedHex: String): Boolean {
        val received = Hex.decode(receivedHex) ?: return false
        if (received.size != PROOF_LEN) return false
        return MessageDigest.isEqual(proof(secret, role, transcript), received)
    }
}
