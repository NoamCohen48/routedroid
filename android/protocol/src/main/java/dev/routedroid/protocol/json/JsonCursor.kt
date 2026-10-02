package dev.routedroid.protocol.json

/** One pass over the decoded text of a body; see [JsonParser] for what it refuses. */
internal class JsonCursor(private val s: String) {
    private var pos = 0
    val atEnd get() = pos == s.length

    fun fail(why: String): Nothing = throw JsonException("$why at offset $pos")

    fun skipWhitespace() {
        while (pos < s.length && (s[pos] == ' ' || s[pos] == '\t' || s[pos] == '\n' || s[pos] == '\r')) pos++
    }

    private fun peek(): Char = if (pos < s.length) s[pos] else fail("unexpected end")

    private fun expect(c: Char) {
        if (peek() != c) fail("expected '$c'")
        pos++
    }

    /** [depth] counts the containers already open; at most [JsonParser.MAX_DEPTH] may be. */
    fun value(depth: Int): JsonValue {
        return when (peek()) {
            '{' -> obj(depth)
            '[' -> array(depth)
            '"' -> JsonString(string())
            't' -> literal("true", JsonBoolean(true))
            'f' -> literal("false", JsonBoolean(false))
            'n' -> literal("null", JsonNull)
            else -> number()
        }
    }

    private fun literal(word: String, v: JsonValue): JsonValue {
        if (!s.startsWith(word, pos)) fail("unexpected character")
        pos += word.length
        return v
    }

    private fun enter(depth: Int) {
        if (depth >= JsonParser.MAX_DEPTH) fail("nested deeper than ${JsonParser.MAX_DEPTH}")
    }

    private fun obj(depth: Int): JsonObject {
        enter(depth)
        expect('{')
        val members = LinkedHashMap<String, JsonValue>()
        skipWhitespace()
        if (peek() == '}') { pos++; return JsonObject(members) }
        while (true) {
            skipWhitespace()
            if (peek() != '"') fail("expected a member name")
            val name = string()
            skipWhitespace(); expect(':'); skipWhitespace()
            if (members.put(name, value(depth + 1)) != null) fail("duplicate member \"$name\"")
            skipWhitespace()
            when (peek()) {
                ',' -> pos++
                '}' -> { pos++; return JsonObject(members) }
                else -> fail("expected ',' or '}'")
            }
        }
    }

    private fun array(depth: Int): JsonArray {
        enter(depth)
        expect('[')
        val items = ArrayList<JsonValue>()
        skipWhitespace()
        if (peek() == ']') { pos++; return JsonArray(items) }
        while (true) {
            skipWhitespace()
            items += value(depth + 1)
            skipWhitespace()
            when (peek()) {
                ',' -> pos++
                ']' -> { pos++; return JsonArray(items) }
                else -> fail("expected ',' or ']'")
            }
        }
    }

    private fun string(): String {
        expect('"')
        val out = StringBuilder()
        while (true) {
            val c = peek()
            pos++
            when {
                c == '"' -> return out.toString()
                c == '\\' -> escape(out)
                c < ' ' -> fail("raw control character in a string")
                else -> out.append(c)
            }
        }
    }

    private fun escape(out: StringBuilder) {
        when (val e = peek().also { pos++ }) {
            '"', '\\', '/' -> out.append(e)
            'b' -> out.append('\b')
            'f' -> out.append('\u000C')
            'n' -> out.append('\n')
            'r' -> out.append('\r')
            't' -> out.append('\t')
            'u' -> {
                val unit = hex4()
                when {
                    Character.isLowSurrogate(unit) -> fail("lone low surrogate")
                    Character.isHighSurrogate(unit) -> {
                        if (!s.startsWith("\\u", pos)) fail("lone high surrogate")
                        pos += 2
                        val low = hex4()
                        if (!Character.isLowSurrogate(low)) fail("lone high surrogate")
                        out.append(unit).append(low)
                    }
                    else -> out.append(unit)
                }
            }
            else -> fail("bad escape")
        }
    }

    private fun hex4(): Char {
        if (pos + 4 > s.length) fail("short \\u escape")
        var v = 0
        repeat(4) {
            val d = Character.digit(s[pos], 16)
            if (d < 0 || s[pos] > 'f') fail("bad \\u escape")
            v = v * 16 + d
            pos++
        }
        return v.toChar()
    }

    private fun number(): JsonNumber {
        val start = pos
        if (peek() == '-') pos++
        when {
            pos < s.length && s[pos] == '0' -> pos++
            pos < s.length && s[pos] in '1'..'9' -> digits()
            else -> fail("unexpected character")
        }
        if (pos < s.length && s[pos] == '.') { pos++; requireDigits() }
        if (pos < s.length && (s[pos] == 'e' || s[pos] == 'E')) {
            pos++
            if (pos < s.length && (s[pos] == '+' || s[pos] == '-')) pos++
            requireDigits()
        }
        return JsonNumber(s.substring(start, pos))
    }

    private fun digits() { while (pos < s.length && s[pos] in '0'..'9') pos++ }

    private fun requireDigits() {
        if (pos >= s.length || s[pos] !in '0'..'9') fail("expected a digit")
        digits()
    }
}
