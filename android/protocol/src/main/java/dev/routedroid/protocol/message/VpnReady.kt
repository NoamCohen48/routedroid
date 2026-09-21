package dev.routedroid.protocol.message

/** VPN_READY, Android → host (§4.5). */
data class VpnReady(val addresses: List<String>, val mtu: Int) {
    fun encode(): ByteArray = JsonOut().strList("addresses", addresses).num("mtu", mtu).bytes()

    companion object {
        fun decode(body: ByteArray): VpnReady {
            val o = Fields.parse(body)
            val list = o.optJSONArray("addresses") ?: Fields.fail("addresses", "required list")
            val addrs = (0 until list.length()).map { i ->
                val a = list.getString(i)
                val slash = a.indexOf('/')
                if (slash < 0) Fields.fail("addresses", "expected ip/prefix")
                Fields.ipv4("addresses", a.substring(0, slash))
                if (a.substring(slash + 1).toIntOrNull() !in 0..32) Fields.fail("addresses", "prefix must be 0-32")
                a
            }
            return VpnReady(addrs, Fields.mtu("mtu", Fields.int(o, "mtu")))
        }
    }
}
