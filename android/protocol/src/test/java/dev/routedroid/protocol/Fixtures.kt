package dev.routedroid.protocol

import dev.routedroid.protocol.auth.Hex
import org.json.JSONArray
import org.json.JSONObject

/** Loads the protocol fixtures (exposed as test resources by build.gradle.kts). */
object Fixtures {
    fun load(name: String): JSONObject {
        val text = Fixtures::class.java.classLoader!!.getResourceAsStream(name)
            ?.bufferedReader()?.readText() ?: error("missing fixture $name")
        return JSONObject(text)
    }

    fun hex(s: String): ByteArray = Hex.decode(s) ?: error("bad fixture hex")

    fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
}
