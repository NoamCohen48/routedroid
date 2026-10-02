package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.auth.Auth
import dev.routedroid.protocol.auth.Hex
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthFixtureTest {
    @Test fun vectors() {
        val f = Fixtures.load("auth.json")
        assertEquals(Auth.DOMAIN, f.str("domain"))
        val vectors = f.objects("vectors")
        assertTrue(vectors.size >= 3)
        for (v in vectors) {
            val name = v.str("name")
            val t = Auth.transcript(v.str("session"), v.int("device_port"), v.hex("client_nonce_hex"), v.hex("host_nonce_hex"))
            assertArrayEquals(name, v.hex("transcript_hex"), t)
            val proofs = Auth.proofs(v.hex("secret_hex"), t)
            assertEquals(name, v.str("host_proof_hex"), Hex.encode(proofs.host))
            assertEquals(name, v.str("android_proof_hex"), Hex.encode(proofs.android))
            assertTrue(Auth.matches(proofs.host, v.str("host_proof_hex")))
            assertFalse("$name: reflection", Auth.matches(proofs.host, v.str("android_proof_hex")))
        }
    }

    @Test fun proofsBindEveryField() {
        val secret = ByteArray(32) { 7 }
        val c = ByteArray(32) { 1 }; val h = ByteArray(32) { 2 }; val x = ByteArray(32) { 3 }
        val base = Auth.proofs(secret, Auth.transcript("s1", 9000, c, h)).host
        for (other in listOf(Auth.transcript("s2", 9000, c, h), Auth.transcript("s1", 9001, c, h),
            Auth.transcript("s1", 9000, x, h), Auth.transcript("s1", 9000, c, x)))
            assertFalse(Auth.matches(Auth.proofs(secret, other).host, Hex.encode(base)))
        assertFalse(Auth.matches(Auth.proofs(ByteArray(32) { 8 }, Auth.transcript("s1", 9000, c, h)).host, Hex.encode(base)))
        assertFalse("malformed hex", Auth.matches(base, "zz"))
        assertFalse("uppercase hex", Auth.matches(base, Hex.encode(base).uppercase()))
        assertFalse("short", Auth.matches(base, Hex.encode(base).dropLast(2)))
    }

    @Test fun transcriptRefusesWhatTheWireCannotCarry() {
        val n = ByteArray(32)
        assertThrows(IllegalArgumentException::class.java) { Auth.transcript("s", 0, n, n) }
        assertThrows(IllegalArgumentException::class.java) { Auth.transcript("s", 65536, n, n) }
        assertThrows(IllegalArgumentException::class.java) { Auth.transcript("é", 1, n, n) }
        assertThrows(IllegalArgumentException::class.java) { Auth.transcript("s", 1, n, ByteArray(31)) }
    }
}
