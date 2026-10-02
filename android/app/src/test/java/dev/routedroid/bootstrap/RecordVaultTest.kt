package dev.routedroid.bootstrap

import dev.routedroid.protocol.auth.BootstrapRecord
import dev.routedroid.transport.Clock
import java.util.concurrent.Executors
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class RecordVaultTest {
    @Volatile private var now = 0L
    private val timers = Executors.newSingleThreadScheduledExecutor()
    private val vault = RecordVault(Clock { now }, timers, ttlMs = 200)

    private fun record(session: String) = BootstrapRecord.of(session, 9000, ByteArray(32) { 1 })

    private fun assertWiped(r: BootstrapRecord) = r.withSecret { assertArrayEquals(ByteArray(32), it) }

    @Test fun takenOnceAndOnlyForItsSession() {
        val r = record("a")
        vault.put(r)
        assertNull(vault.take("b"))
        assertSame(r, vault.take("a"))
        assertNull(vault.take("a"))
    }

    @Test fun aNewRecordReplacesAndWipesThePendingOne() {
        val old = record("a")
        vault.put(old)
        vault.put(record("b"))
        assertWiped(old)
        assertNull(vault.take("a"))
    }

    @Test fun expiredOnTheClock() {
        val r = record("a")
        vault.put(r)
        now = 200
        assertNull(vault.take("a"))
        assertWiped(r)
    }

    @Test fun wipedByTheTimerWithoutAnotherLook() {
        val r = record("a")
        vault.put(r)
        Thread.sleep(500)
        assertWiped(r)
    }

    @Test fun aTakenRecordIsNotWipedByItsTimer() {
        val r = record("a")
        vault.put(r)
        vault.take("a")
        Thread.sleep(500)
        r.withSecret { assertArrayEquals(ByteArray(32) { 1 }, it) }
    }
}
