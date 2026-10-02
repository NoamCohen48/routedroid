package dev.routedroid.transport

import dev.routedroid.transport.Keepalive.Verdict
import org.junit.Assert.assertEquals
import org.junit.Test

class KeepaliveTest {
    @Test fun pingsAfterIdleBothWaysAndDiesAfterSilence() {
        val k = Keepalive(0)
        assertEquals(Verdict.NONE, k.check(9_999))
        assertEquals(10_000, k.nextCheckIn(0))
        assertEquals(Verdict.PING, k.check(10_000))
        k.sent(10_000)
        assertEquals(Verdict.NONE, k.check(15_000))
        assertEquals(5_000, k.nextCheckIn(15_000))
        assertEquals(Verdict.PING, k.check(20_000))
        k.sent(20_000)
        assertEquals(10_000, k.nextCheckIn(20_000))
        assertEquals(Verdict.DEAD, k.check(30_000))
    }

    @Test fun trafficEitherWayHoldsOffThePing() {
        val k = Keepalive(0)
        k.sent(8_000)
        assertEquals(Verdict.NONE, k.check(12_000))
        k.received(12_000)
        assertEquals(Verdict.NONE, k.check(21_000))
        assertEquals(Verdict.PING, k.check(22_000))
        assertEquals(10_000, k.nextCheckIn(22_000))
    }

    @Test fun keepsPingingWhileThePingIsStillQueued() {
        // Regression: asking at once, before the writer recorded the PING, slept to DEAD.
        val k = Keepalive(0)
        assertEquals(Verdict.PING, k.check(10_000))
        assertEquals(10_000, k.nextCheckIn(10_000))
        assertEquals(Verdict.PING, k.check(20_000))
        assertEquals(10_000, k.nextCheckIn(20_000))
        assertEquals(Verdict.DEAD, k.check(30_000))
    }

    @Test fun onlyReceivingKeepsItAlive() {
        val k = Keepalive(0)
        for (t in 5_000L..29_000L step 5_000) k.sent(t)
        assertEquals(Verdict.NONE, k.check(29_999))
        assertEquals(Verdict.DEAD, k.check(30_000))
    }
}
