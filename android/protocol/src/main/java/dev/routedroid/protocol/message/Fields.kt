package dev.routedroid.protocol.message

import dev.routedroid.protocol.Protocol
import dev.routedroid.protocol.json.JsonArray
import dev.routedroid.protocol.json.JsonException
import dev.routedroid.protocol.json.JsonNull
import dev.routedroid.protocol.json.JsonNumber
import dev.routedroid.protocol.json.JsonObject
import dev.routedroid.protocol.json.JsonParser
import dev.routedroid.protocol.json.JsonString
import dev.routedroid.protocol.json.JsonValue
import dev.routedroid.protocol.net.Ipv4Address

/**
 * Typed access to one body's members with the §4 rules: a missing or wrongly typed required
 * member is an error, unknown members are ignored, nothing is coerced, lengths count code
 * points (as Rust's `chars().count()` does).
 */
internal class Fields private constructor(private val o: JsonObject) {
    private fun member(name: String): JsonValue? = o[name]?.takeUnless { it == JsonNull }

    fun string(name: String): String =
        (member(name) as? JsonString)?.value ?: fail(name, "required string")

    fun optionalString(name: String): String? = when (val v = member(name)) {
        null -> null
        is JsonString -> v.value
        else -> fail(name, "must be a string")
    }

    fun int(name: String, range: IntRange): Int = int(name, member(name), range)

    fun list(name: String): List<JsonValue> = (member(name) as? JsonArray)?.items ?: fail(name, "required list")

    fun optionalList(name: String): List<JsonValue>? = when (val v = member(name)) {
        null -> null
        is JsonArray -> v.items
        else -> fail(name, "must be a list")
    }

    fun ipv4(name: String): Ipv4Address = ipv4(name, member(name) ?: fail(name, "required address"))

    fun mtu(name: String): Int = int(name, Protocol.MIN_MTU..Protocol.MAX_PACKET_BODY)

    fun hex(name: String, length: Int): String {
        val v = string(name)
        if (v.length != length || !v.all { it in '0'..'9' || it in 'a'..'f' }) fail(name, "expected $length lowercase hex characters")
        return v
    }

    companion object {
        fun parse(body: ByteArray): Fields = try {
            Fields(JsonParser.parse(body) as? JsonObject ?: throw BodyException("body is not a JSON object"))
        } catch (e: JsonException) {
            throw BodyException("invalid JSON body: ${e.message}")
        }

        fun of(name: String, v: JsonValue): Fields = Fields(v as? JsonObject ?: fail(name, "expected an object"))

        fun fail(name: String, reason: String): Nothing = throw BodyException("field `$name`: $reason")

        fun int(name: String, v: JsonValue?, range: IntRange): Int {
            val n = (v as? JsonNumber)?.longOrNull() ?: fail(name, "required integer")
            if (n !in range.first..range.last) fail(name, "must be ${range.first}-${range.last}")
            return n.toInt()
        }

        fun string(name: String, v: JsonValue): String = (v as? JsonString)?.value ?: fail(name, "expected a string")

        fun ipv4(name: String, v: JsonValue): Ipv4Address =
            (v as? JsonString)?.value?.let(Ipv4Address::parse) ?: fail(name, "expected dotted-quad IPv4")

        fun codePoints(s: String): Int = s.codePointCount(0, s.length)
    }
}
