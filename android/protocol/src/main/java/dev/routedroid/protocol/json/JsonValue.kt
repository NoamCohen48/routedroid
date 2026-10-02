package dev.routedroid.protocol.json

/** A parsed RFC 8259 value. Objects keep member order; a name occurs at most once. */
sealed interface JsonValue

data class JsonObject(val members: Map<String, JsonValue>) : JsonValue {
    operator fun get(name: String): JsonValue? = members[name]
}

data class JsonArray(val items: List<JsonValue>) : JsonValue

data class JsonString(val value: String) : JsonValue

/**
 * A number, kept as its literal text so that `1`, `1.0` and `1e0` stay distinguishable:
 * the protocol's integer fields accept only the first.
 */
data class JsonNumber(val text: String) : JsonValue {
    /** The value of an integer literal that fits a Long; null for a fraction, exponent or overflow. */
    fun longOrNull(): Long? =
        if (text.any { it == '.' || it == 'e' || it == 'E' }) null else text.toLongOrNull()
}

data class JsonBoolean(val value: Boolean) : JsonValue

data object JsonNull : JsonValue

/** The bytes are not one strict RFC 8259 JSON value. */
class JsonException(message: String) : Exception(message)
