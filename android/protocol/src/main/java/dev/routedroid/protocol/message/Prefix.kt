package dev.routedroid.protocol.message

import org.json.JSONObject

/** `{"address":"a.b.c.d","prefix":n}` used by CONFIGURE_VPN. */
data class Prefix(val address: String, val prefix: Int) {
    internal fun json(): String = JsonOut().str("address", address).num("prefix", prefix).let { String(it.bytes()) }

    companion object {
        internal fun decode(field: String, o: JSONObject): Prefix {
            val p = Fields.int(o, "prefix")
            if (p !in 0..32) Fields.fail(field, "prefix must be 0-32")
            return Prefix(Fields.ipv4(field, Fields.str(o, "address")), p)
        }
    }
}
