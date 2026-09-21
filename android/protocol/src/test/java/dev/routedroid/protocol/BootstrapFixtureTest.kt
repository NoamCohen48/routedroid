package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.auth.BootstrapRecord
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class BootstrapFixtureTest {
    @Test fun vectors() {
        val f = Fixtures.load("bootstrap.json")
        assertEquals(BootstrapRecord.LENGTH, f.getInt("length"))
        assertEquals(BootstrapRecord.PROVIDER_URI, f.getString("provider_uri"))
        assertArrayEquals(BootstrapRecord.MAGIC, f.getString("magic").toByteArray())
        for (v in f.getJSONArray("vectors").objects()) {
            val name = v.getString("name")
            val rec = BootstrapRecord(v.getString("session"), Fixtures.hex(v.getString("secret_hex")))
            val want = Fixtures.hex(v.getString("record_hex"))
            assertArrayEquals(name, want, rec.encode())
            assertEquals(name, rec, BootstrapRecord.decode(want))
        }
        for (v in f.getJSONArray("invalid").objects())
            assertNull(v.getString("name"), BootstrapRecord.decode(Fixtures.hex(v.getString("record_hex"))))
    }

    @Test fun invalidSessionRefusedOnEncode() {
        val secret = ByteArray(32) { 1 }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord("", secret).encode() }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord("s".repeat(41), secret).encode() }
        BootstrapRecord("s".repeat(40), secret).encode()
    }
}
