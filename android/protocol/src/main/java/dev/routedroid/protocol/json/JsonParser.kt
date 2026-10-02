package dev.routedroid.protocol.json

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

/**
 * A strict RFC 8259 parser, the same on the phone and in the JVM tests (the platform's
 * org.json is lenient: trailing data, duplicate names, single quotes, hex numbers).
 *
 * Refused: invalid UTF-8, a byte-order mark, anything after the value, duplicate member
 * names, raw control characters in strings, lone surrogate escapes, leading zeros, and
 * nesting deeper than [MAX_DEPTH]. `fixtures/bodies.json` pins these against serde_json.
 */
object JsonParser {
    const val MAX_DEPTH = 32

    fun parse(bytes: ByteArray): JsonValue {
        val cursor = JsonCursor(decodeUtf8(bytes))
        cursor.skipWhitespace()
        val value = cursor.value(0)
        cursor.skipWhitespace()
        if (!cursor.atEnd) cursor.fail("data after the value")
        return value
    }

    private fun decodeUtf8(bytes: ByteArray): String {
        if (bytes.size >= 3 && bytes[0] == 0xEF.toByte() && bytes[1] == 0xBB.toByte() && bytes[2] == 0xBF.toByte()) {
            throw JsonException("byte-order mark")
        }
        val decoder = Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
        return try {
            decoder.decode(ByteBuffer.wrap(bytes)).toString()
        } catch (_: CharacterCodingException) {
            throw JsonException("invalid UTF-8")
        }
    }
}
