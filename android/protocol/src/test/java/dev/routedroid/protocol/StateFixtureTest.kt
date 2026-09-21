package dev.routedroid.protocol

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
        val states = f.getJSONArray("states").let { a -> (0 until a.length()).map { a.getString(it) } }
        assertEquals(State.entries.map { it.name }, states)
        for ((role, key) in listOf(Role.HOST to "host_receives", Role.ANDROID to "android_receives")) {
            val table = f.getJSONObject(key)
            for (st in State.entries) {
                val want = table.getJSONArray(st.name).let { a -> (0 until a.length()).map { a.getString(it) } }.toSet()
                assertEquals("$role in $st", want, Allowlist.allowed(role, st).map { it.wireName }.toSet())
            }
        }
        val types = f.getJSONObject("types")
        assertEquals(MessageType.entries.size, types.length())
        for (t in MessageType.entries) assertEquals(t.wireName, t.code, types.getInt(t.wireName))
    }

    @Test fun packetsOnlyWhenActive() {
        for (role in Role.entries) {
            assertTrue(Allowlist.allowed(role, State.Closed).isEmpty())
            for (st in State.entries) assertEquals("$role $st", st == State.Active, Allowlist.isAllowed(role, st, MessageType.IP_PACKET))
        }
    }
}
