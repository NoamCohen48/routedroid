package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.auth.Auth
import dev.routedroid.protocol.auth.Hex
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthFixtureTest {
    @Test fun vectors() {
        val f = Fixtures.load("auth.json")
        assertEquals(Auth.DOMAIN, f.getString("domain"))
        val vectors = f.getJSONArray("vectors").objects()
        assertTrue(vectors.size >= 3)
        for (v in vectors) {
            val name = v.getString("name")
            val secret = Fixtures.hex(v.getString("secret_hex"))
            val t = Auth.transcript(v.getString("session"), v.getInt("device_port"),
                Fixtures.hex(v.getString("client_nonce_hex")), Fixtures.hex(v.getString("host_nonce_hex")))
            assertArrayEquals(name, Fixtures.hex(v.getString("transcript_hex")), t)
            val hp = v.getString("host_proof_hex"); val ap = v.getString("android_proof_hex")
            assertEquals(name, hp, Hex.encode(Auth.proof(secret, Auth.ROLE_HOST, t)))
            assertEquals(name, ap, Hex.encode(Auth.proof(secret, Auth.ROLE_ANDROID, t)))
            assertTrue(Auth.verify(secret, Auth.ROLE_HOST, t, hp))
            assertTrue(Auth.verify(secret, Auth.ROLE_ANDROID, t, ap))
            assertFalse("$name reflection", Auth.verify(secret, Auth.ROLE_ANDROID, t, hp))
            assertFalse("$name reflection", Auth.verify(secret, Auth.ROLE_HOST, t, ap))
        }
    }

    @Test fun proofsBindEveryField() {
        val secret = ByteArray(32) { 7 }
        val c = ByteArray(32) { 1 }; val h = ByteArray(32) { 2 }; val x = ByteArray(32) { 3 }
        val base = Auth.transcript("s1", 9000, c, h)
        val p = Hex.encode(Auth.proof(secret, Auth.ROLE_HOST, base))
        for (other in listOf(Auth.transcript("s2", 9000, c, h), Auth.transcript("s1", 9001, c, h),
            Auth.transcript("s1", 9000, x, h), Auth.transcript("s1", 9000, c, x)))
            assertFalse(Auth.verify(secret, Auth.ROLE_HOST, other, p))
        assertFalse(Auth.verify(ByteArray(32) { 8 }, Auth.ROLE_HOST, base, p))
        assertFalse("malformed hex", Auth.verify(secret, Auth.ROLE_HOST, base, "zz"))
        assertFalse("uppercase hex", Auth.verify(secret, Auth.ROLE_HOST, base, p.uppercase()))
    }
}
