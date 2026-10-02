package dev.routedroid.protocol.json

/**
 * Writes one JSON object with members in call order and no whitespace. Strings are escaped
 * exactly as serde_json does (`"`, `\`, and control characters; never `/`), so a body the
 * app sends is byte-identical to the fixture the host is tested against.
 */
class JsonWriter {
    private val sb = StringBuilder("{")
    private var first = true

    /** [name] is always a literal from this module, so it needs no escaping. */
    private fun name(name: String): StringBuilder {
        if (!first) sb.append(',')
        first = false
        return sb.append('"').append(name).append("\":")
    }

    fun string(name: String, value: String) = apply { quote(name(name), value) }
    fun int(name: String, value: Long) = apply { name(name).append(value) }
    fun int(name: String, value: Int) = int(name, value.toLong())
    fun strings(name: String, values: List<String>) = list(name, values) { quote(sb, it) }
    fun ints(name: String, values: List<Int>) = list(name, values) { sb.append(it) }
    fun objects(name: String, values: List<JsonWriter>) = list(name, values) { sb.append(it.text()) }

    private fun <T> list(name: String, values: List<T>, item: (T) -> Unit) = apply {
        name(name).append('[')
        values.forEachIndexed { i, v -> if (i > 0) sb.append(','); item(v) }
        sb.append(']')
    }

    private fun text(): String = "$sb}"

    fun bytes(): ByteArray = text().toByteArray(Charsets.UTF_8)

    companion object {
        private const val HEX = "0123456789abcdef"

        fun quote(out: StringBuilder, s: String) {
            out.append('"')
            for (c in s) {
                when {
                    c == '"' -> out.append("\\\"")
                    c == '\\' -> out.append("\\\\")
                    c == '\n' -> out.append("\\n")
                    c == '\r' -> out.append("\\r")
                    c == '\t' -> out.append("\\t")
                    c == '\b' -> out.append("\\b")
                    c == '\u000C' -> out.append("\\f")
                    c < ' ' -> out.append("\\u00").append(HEX[c.code shr 4]).append(HEX[c.code and 0xF])
                    else -> out.append(c)
                }
            }
            out.append('"')
        }
    }
}
