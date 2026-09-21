package dev.routedroid.protocol.message

/** CONFIGURE_VPN, host → Android (§4.4). Version 1: exactly one address. */
data class ConfigureVpn(
    val mtu: Int,
    val addresses: List<Prefix>,
    val routes: List<Prefix>,
    val dns: List<String>,
    val sessionName: String,
) {
    fun encode(): ByteArray = JsonOut()
        .num("mtu", mtu)
        .objList("addresses", addresses.map { it.json() })
        .objList("routes", routes.map { it.json() })
        .strList("dns", dns)
        .str("session_name", sessionName)
        .bytes()

    companion object {
        const val MAX_SESSION_NAME_LEN = 64

        fun decode(body: ByteArray): ConfigureVpn {
            val o = Fields.parse(body)
            val mtu = Fields.mtu("mtu", Fields.int(o, "mtu"))
            val addrs = o.optJSONArray("addresses") ?: Fields.fail("addresses", "required list")
            if (addrs.length() != 1) Fields.fail("addresses", "exactly one address in version 1")
            val routes = o.optJSONArray("routes") ?: Fields.fail("routes", "required list")
            if (routes.length() == 0) Fields.fail("routes", "at least one route")
            val dns = o.optJSONArray("dns") ?: Fields.fail("dns", "required list")
            val name = Fields.str(o, "session_name")
            if (name.length > MAX_SESSION_NAME_LEN) Fields.fail("session_name", "at most 64 characters")
            return ConfigureVpn(
                mtu,
                (0 until addrs.length()).map { Prefix.decode("addresses", addrs.getJSONObject(it)) },
                (0 until routes.length()).map { Prefix.decode("routes", routes.getJSONObject(it)) },
                (0 until dns.length()).map { Fields.ipv4("dns", dns.getString(it)) },
                name,
            )
        }
    }
}
