package dev.routedroid.protocol

import dev.routedroid.protocol.json.JsonArray
import dev.routedroid.protocol.json.JsonBoolean
import dev.routedroid.protocol.json.JsonException
import dev.routedroid.protocol.json.JsonNull
import dev.routedroid.protocol.json.JsonNumber
import dev.routedroid.protocol.json.JsonObject
import dev.routedroid.protocol.json.JsonParser
import dev.routedroid.protocol.json.JsonString
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

/** Parser edges the shared fixtures do not reach. */
class JsonParserTest {
    private fun parse(s: String) = JsonParser.parse(s.toByteArray(Charsets.UTF_8))

    private fun refused(s: String) {
        try {
            parse(s); fail("accepted: $s")
        } catch (_: JsonException) {
        }
    }

    @Test fun values() {
        val o = parse(""" {"a":[1,-2,true,false,null,"x"],"b":{}} """) as JsonObject
        val a = (o["a"] as JsonArray).items
        assertEquals(listOf(1L, -2L), a.take(2).map { (it as JsonNumber).longOrNull() })
        assertEquals(JsonBoolean(true), a[2])
        assertEquals(JsonNull, a[4])
        assertEquals(JsonString("x"), a[5])
        assertEquals(JsonObject(emptyMap()), o["b"])
        assertEquals(JsonString("\uD83D\uDE00"), parse("\"\\ud83d\\ude00\""))
    }

    @Test fun numbersOutsideIntegersAreNotLongs() {
        assertNull((parse("1.5") as JsonNumber).longOrNull())
        assertNull((parse("1e3") as JsonNumber).longOrNull())
        assertNull((parse("99999999999999999999") as JsonNumber).longOrNull())
        assertEquals(0L, (parse("-0") as JsonNumber).longOrNull())
    }

    @Test fun refusals() {
        for (s in listOf("", " ", "{", "[1,]", "{\"a\":1,}", "01", "-", "1.", ".5", "+1", "tru", "nul",
            "\"\\x\"", "\"\\u12\"", "\"\\u12G4\"", "\"\\ude00\"", "\"\\ud83d\"", "\"\\ud83dx\"", "\"a\tb\"",
            "{\"a\":1}{}", "[1] 2", "NaN", "Infinity", "\"unterminated"))
            refused(s)
    }

    @Test fun depthIsBounded() {
        parse("[".repeat(JsonParser.MAX_DEPTH) + "]".repeat(JsonParser.MAX_DEPTH))
        refused("[".repeat(JsonParser.MAX_DEPTH + 1) + "]".repeat(JsonParser.MAX_DEPTH + 1))
    }
}
