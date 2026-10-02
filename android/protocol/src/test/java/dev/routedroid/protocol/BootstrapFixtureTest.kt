package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.hex
import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.objects
import dev.routedroid.protocol.Fixtures.str
import dev.routedroid.protocol.auth.BootstrapRecord
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class BootstrapFixtureTest {
    @Test fun vectors() {
        val f = Fixtures.load("bootstrap.json")
        assertEquals(BootstrapRecord.LENGTH, f.int("length"))
        assertEquals(BootstrapRecord.PROVIDER_URI, f.str("provider_uri"))
        assertArrayEquals(BootstrapRecord.MAGIC, f.str("magic").toByteArray())
        for (v in f.objects("vectors")) {
            val name = v.str("name")
            val want = v.hex("record_hex")
            assertArrayEquals(name, want, BootstrapRecord.of(v.str("session"), v.int("device_port"), v.hex("secret_hex")).encode())
            val r = BootstrapRecord.decode(want)!!
            assertEquals(name, v.str("session"), r.session)
            assertEquals(name, v.int("device_port"), r.devicePort)
            r.withSecret { assertArrayEquals(name, v.hex("secret_hex"), it) }
        }
        for (v in f.objects("invalid")) assertNull(v.str("name"), BootstrapRecord.decode(v.hex("record_hex")))
    }

    @Test fun wipeClearsTheSecretAndToStringHidesIt() {
        val r = BootstrapRecord.of("s1", 9000, ByteArray(32) { 5 })
        assertFalse(r.toString().contains("5"))
        r.wipe()
        r.withSecret { assertArrayEquals(ByteArray(32), it) }
    }

    @Test fun invalidInputsRefused() {
        val secret = ByteArray(32) { 1 }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord.of("", 1, secret) }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord.of("s".repeat(41), 1, secret) }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord.of("s", 0, secret) }
        assertThrows(IllegalArgumentException::class.java) { BootstrapRecord.of("s", 1, ByteArray(31)) }
    }
}
