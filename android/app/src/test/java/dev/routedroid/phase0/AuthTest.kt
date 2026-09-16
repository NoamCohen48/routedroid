package dev.routedroid.phase0

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pins the same vector as host/phase0-tunnel/src/auth.rs `test_vector_matches_kotlin`. */
class AuthTest {
    private val secret = ByteArray(32) { it.toByte() }
    private val client = ByteArray(32) { 0xaa.toByte() }
    private val host = ByteArray(32) { 0xbb.toByte() }

    @Test
    fun proofsMatchRustVector() {
        val t = Auth.transcript(0, "s1", 9000, client, host)
        assertEquals(11 + 1 + 2 + 1 + 2 + 7 + 32 + 4 + 32, t.size)
        val hp = Auth.toHex(Auth.proof(secret, "host", t))
        val ap = Auth.toHex(Auth.proof(secret, "android", t))
        assertEquals("9869086fdafc487465c1f9a92838c1e81859d9f79942b7209830c5e54a8029c7", hp)
        assertEquals("4d01e7857778797c5e4c30f59fcffd20a3f1ae5df07b176d4717293d294b938f", ap)
        assertTrue(Auth.verify(secret, "host", t, hp))
        assertFalse("reflection", Auth.verify(secret, "android", t, hp))
        assertFalse("wrong secret", Auth.verify(ByteArray(32) { 1 }, "host", t, hp))
        assertFalse(Auth.verify(secret, "host", t, "zz"))
        assertFalse(Auth.verify(secret, "host", t, hp.substring(0, 62)))
        assertFalse("port bound", Auth.verify(secret, "host", Auth.transcript(0, "s1", 9001, client, host), hp))
    }

    @Test
    fun recordParsing() {
        val r = ByteArray(80)
        "RDB0".toByteArray().copyInto(r, 0)
        "p0-abc".toByteArray().copyInto(r, 8)
        secret.copyInto(r, 48)
        val rec = Auth.parseRecord(r)
        assertNotNull(rec)
        assertEquals("p0-abc", rec!!.session)
        assertTrue(rec.secret.contentEquals(secret))
        assertNull(Auth.parseRecord(r.copyOf(79)))
        assertNull(Auth.parseRecord(r.copyOf().also { it[4] = 1 }))
        assertNull(Auth.parseRecord(r.copyOf().also { it[0] = 'X'.code.toByte() }))
        assertNull("empty session", Auth.parseRecord(r.copyOf().also { it.fill(0, 8, 48) }))
        assertNull("garbage after NUL", Auth.parseRecord(r.copyOf().also { it[20] = 1 }))
        rec.wipe()
        assertTrue(rec.secret.all { it.toInt() == 0 })
    }

    @Test
    fun hexRoundTrip() {
        assertEquals("00ff10", Auth.toHex(byteArrayOf(0, -1, 16)))
        assertTrue(Auth.fromHex("00ff10")!!.contentEquals(byteArrayOf(0, -1, 16)))
        assertNull(Auth.fromHex("0"))
        assertNull(Auth.fromHex("zz"))
        assertNull(Auth.nonce32("00"))
    }
}
