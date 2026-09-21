package dev.routedroid.protocol.message

import org.json.JSONException
import org.json.JSONObject

/** A body failed §4 field rules or was not valid JSON. */
class BodyException(val field: String?, message: String) : Exception(message)

/**
 * Minimal ordered JSON writer. org.json does not guarantee key order, and the
 * fixtures are byte-exact, so senders build bodies with this instead.
 */
internal class JsonOut {
    private val sb = StringBuilder("{")
    private var first = true

    private fun key(k: String) {
        if (!first) sb.append(',')
        first = false
        sb.append('"').append(k).append("\":")
    }

    fun str(k: String, v: String) = apply { key(k); sb.append(JSONObject.quote(v)) }
    fun num(k: String, v: Number) = apply { key(k); sb.append(v.toString()) }
    fun raw(k: String, v: String) = apply { key(k); sb.append(v) }
    fun strList(k: String, v: List<String>) = raw(k, v.joinToString(",", "[", "]") { JSONObject.quote(it) })
    fun numList(k: String, v: List<Int>) = raw(k, v.joinToString(",", "[", "]"))
    fun objList(k: String, v: List<String>) = raw(k, v.joinToString(",", "[", "]"))

    fun bytes(): ByteArray = sb.append('}').toString().toByteArray(Charsets.UTF_8)
}

internal object Fields {
    fun parse(body: ByteArray): JSONObject = try {
        JSONObject(String(body, Charsets.UTF_8))
    } catch (e: Exception) {
        throw BodyException(null, "invalid JSON body: ${e.message}")
    }

    fun fail(field: String, reason: String): Nothing = throw BodyException(field, "field `$field`: $reason")

    /** Runs a decoder; any org.json failure on the way becomes a [BodyException]. */
    fun <T> decoding(block: () -> T): T = try {
        block()
    } catch (e: JSONException) {
        throw BodyException(null, "malformed body: ${e.message}")
    }

    fun str(o: JSONObject, k: String): String =
        if (o.has(k) && o.opt(k) is String) o.getString(k) else fail(k, "required string")

    fun int(o: JSONObject, k: String): Int = when (val v = o.opt(k)) {
        is Int -> v
        is Long -> if (v in Int.MIN_VALUE..Int.MAX_VALUE) v.toInt() else fail(k, "integer out of range")
        else -> fail(k, "required integer")
    }

    fun hex(k: String, v: String, len: Int): String {
        if (v.length != len || !v.all { it in '0'..'9' || it in 'a'..'f' }) fail(k, "expected $len lowercase hex characters")
        return v
    }

    fun ipv4(k: String, v: String): String {
        val parts = v.split('.')
        if (parts.size != 4 || parts.any { p -> p.isEmpty() || p.length > 3 || !p.all(Char::isDigit) || p.toInt() > 255 || (p.length > 1 && p[0] == '0') })
            fail(k, "expected dotted-quad IPv4")
        return v
    }

    fun mtu(k: String, v: Int): Int {
        if (v < dev.routedroid.protocol.Protocol.MIN_MTU || v > dev.routedroid.protocol.Protocol.MAX_PACKET_BODY) fail(k, "must be 576-65535")
        return v
    }
}
