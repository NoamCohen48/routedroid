package dev.routedroid.protocol

import dev.routedroid.protocol.auth.Hex
import dev.routedroid.protocol.frame.ByteSource
import dev.routedroid.protocol.json.JsonArray
import dev.routedroid.protocol.json.JsonNumber
import dev.routedroid.protocol.json.JsonObject
import dev.routedroid.protocol.json.JsonParser
import dev.routedroid.protocol.json.JsonString

/** Loads protocol/fixtures (test resources, see build.gradle.kts) with the module's own parser. */
object Fixtures {
    fun load(name: String): JsonObject {
        val bytes = Fixtures::class.java.classLoader!!.getResourceAsStream(name)?.readBytes() ?: error("missing fixture $name")
        return JsonParser.parse(bytes) as JsonObject
    }

    fun unhex(s: String): ByteArray = Hex.decode(s) ?: error("bad fixture hex")

    fun JsonObject.str(name: String): String = (this[name] as JsonString).value
    fun JsonObject.int(name: String): Int = (this[name] as JsonNumber).longOrNull()!!.toInt()
    fun JsonObject.hex(name: String): ByteArray = unhex(str(name))
    fun JsonObject.obj(name: String): JsonObject = this[name] as JsonObject
    fun JsonObject.objects(name: String): List<JsonObject> = (this[name] as JsonArray).items.map { it as JsonObject }
    fun JsonObject.strings(name: String): List<String> = (this[name] as JsonArray).items.map { (it as JsonString).value }

    /** A stream over [bytes] that hands out at most [chunk] bytes per read, as a socket may. */
    fun source(bytes: ByteArray, chunk: Int = Int.MAX_VALUE): ByteSource {
        var at = 0
        return ByteSource { dst ->
            if (at == bytes.size) return@ByteSource -1
            val n = minOf(dst.remaining(), bytes.size - at, chunk)
            dst.put(bytes, at, n); at += n; n
        }
    }
}
