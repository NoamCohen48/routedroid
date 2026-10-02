package dev.routedroid.protocol

import dev.routedroid.protocol.Fixtures.int
import dev.routedroid.protocol.Fixtures.obj
import dev.routedroid.protocol.Fixtures.strings
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class StateFixtureTest {
    @Test fun allowlistMatchesFixture() {
        val f = Fixtures.load("states.json")
        assertEquals(State.entries.map { it.name }, f.strings("states"))
        for ((role, key) in listOf(Role.HOST to "host_receives", Role.ANDROID to "android_receives")) {
            val table = f.obj(key)
            for (st in State.entries) {
                assertEquals("$role in $st", table.strings(st.name).toSet(), Allowlist.allowed(role, st).map { it.name }.toSet())
            }
        }
        val types = f.obj("types")
        assertEquals(MessageType.entries.size, types.members.size)
        for (t in MessageType.entries) assertEquals(t.name, t.code, types.int(t.name))
    }

    @Test fun packetsOnlyWhenActive() {
        for (role in Role.entries) {
            assertTrue(Allowlist.allowed(role, State.Closed).isEmpty())
            for (st in State.entries) assertEquals("$role $st", st == State.Active, Allowlist.isAllowed(role, st, MessageType.IP_PACKET))
        }
    }
}
