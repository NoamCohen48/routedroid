package dev.routedroid.protocol.auth

import dev.routedroid.protocol.Protocol
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
import javax.security.auth.DestroyFailedException

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
    private const val ROLE_HOST = "host"
    private const val ROLE_ANDROID = "android"

    private val rng = SecureRandom()

    fun randomNonce(): ByteArray = ByteArray(NONCE_LEN).also(rng::nextBytes)

    fun transcript(session: String, devicePort: Int, clientNonce: ByteArray, hostNonce: ByteArray): ByteArray {
        require(Protocol.validSession(session)) { "invalid session id" }
        require(devicePort in 1..65535) { "invalid device port" }
        require(clientNonce.size == NONCE_LEN && hostNonce.size == NONCE_LEN) { "nonces are $NONCE_LEN bytes" }
        val out = ByteArrayOutputStream(DOMAIN.length + 4 + session.length + 2 + 11 + 2 * NONCE_LEN)
        out.write(DOMAIN.toByteArray(Charsets.US_ASCII)); out.write(0)
        out.write(Protocol.VERSION)
        out.write(session.toByteArray(Charsets.US_ASCII)); out.write(0)
        out.write(devicePort ushr 8); out.write(devicePort and 0xFF)
        out.write(ROLE_ANDROID.toByteArray(Charsets.US_ASCII)); out.write(clientNonce)
        out.write(ROLE_HOST.toByteArray(Charsets.US_ASCII)); out.write(hostNonce)
        return out.toByteArray()
    }

    class Proofs(val host: ByteArray, val android: ByteArray)

    /**
     * Both proofs from one keyed MAC, so the key is expanded once and the copy the JCA key
     * holds is destroyed where the provider supports it. Copies inside the provider's MAC
     * state are beyond reach (§7.3 "where the runtime permits").
     */
    fun proofs(secret: ByteArray, transcript: ByteArray): Proofs {
        val key = SecretKeySpec(secret, "HmacSHA256")
        try {
            val mac = Mac.getInstance("HmacSHA256").apply { init(key) }
            fun proof(role: String): ByteArray {
                mac.update(role.toByteArray(Charsets.US_ASCII))
                return mac.doFinal(transcript)
            }
            return Proofs(host = proof(ROLE_HOST), android = proof(ROLE_ANDROID))
        } finally {
            try { key.destroy() } catch (_: DestroyFailedException) {}
        }
    }

    /** Constant-time comparison; a malformed [receivedHex] simply fails. */
    fun matches(expected: ByteArray, receivedHex: String): Boolean {
        val received = Hex.decode(receivedHex) ?: return false
        return received.size == PROOF_LEN && MessageDigest.isEqual(expected, received)
    }
}
