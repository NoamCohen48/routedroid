package dev.routedroid.protocol.message

import dev.routedroid.protocol.json.JsonValue
import dev.routedroid.protocol.json.JsonWriter
import dev.routedroid.protocol.net.Ipv4Address
import dev.routedroid.protocol.net.Ipv4Prefix

// CONFIGURE_VPN and VPN_READY (§4.4–4.5).

/** CONFIGURE_VPN, decoded into what VpnService.Builder takes, after every §4.4 rule passed. */
data class ConfigureVpn(
    val mtu: Int,
    /** Version 1 has exactly one: a unicast host address with prefix 32. */
    val address: Ipv4Prefix,
    val routes: List<Ipv4Prefix>,
    val dns: List<Ipv4Address>,
    val sessionName: String,
) {
    fun encode(): ByteArray = JsonWriter()
        .int("mtu", mtu)
        .objects("addresses", listOf(prefix(address)))
        .objects("routes", routes.map(::prefix))
        .strings("dns", dns.map { it.toString() })
        .string("session_name", sessionName)
        .bytes()

    private fun prefix(p: Ipv4Prefix) = JsonWriter().string("address", p.address.toString()).int("prefix", p.length)

    companion object {
        const val MAX_SESSION_NAME_LEN = 64

        /** [negotiatedMtu] is HELLO_ACK's; CONFIGURE_VPN must repeat it. */
        fun decode(body: ByteArray, negotiatedMtu: Int): ConfigureVpn {
            val f = Fields.parse(body)
            val mtu = f.mtu("mtu")
            if (mtu != negotiatedMtu) Fields.fail("mtu", "$mtu is not the negotiated $negotiatedMtu")
            val address = f.list("addresses").singleOrNull()?.let { prefix("addresses", it) }
                ?: Fields.fail("addresses", "exactly one address in version 1")
            if (address.length != 32 || !address.address.isUnicastHost) Fields.fail("addresses", "a unicast host address with prefix 32")
            val routes = f.list("routes").map { prefix("routes", it) }
            if (routes.isEmpty()) Fields.fail("routes", "at least one route")
            if (!routes.all { it.isCanonical }) Fields.fail("routes", "no address bits past the prefix")
            val dns = f.list("dns").map { Fields.ipv4("dns", it) }
            if (!dns.all { it.isUnicastHost }) Fields.fail("dns", "unicast addresses")
            val name = f.string("session_name")
            if (Fields.codePoints(name) > MAX_SESSION_NAME_LEN) Fields.fail("session_name", "at most $MAX_SESSION_NAME_LEN characters")
            return ConfigureVpn(mtu, address, routes, dns, name)
        }

        private fun prefix(name: String, v: JsonValue): Ipv4Prefix {
            val f = Fields.of(name, v)
            return Ipv4Prefix(f.ipv4("address"), f.int("prefix", 0..32))
        }
    }
}

/** VPN_READY, Android → host: what was actually configured. */
data class VpnReady(val addresses: List<Ipv4Prefix>, val mtu: Int) {
    fun encode(): ByteArray = JsonWriter().strings("addresses", addresses.map { it.toString() }).int("mtu", mtu).bytes()

    companion object {
        fun decode(body: ByteArray): VpnReady {
            val f = Fields.parse(body)
            val addresses = f.list("addresses").map { v ->
                val text = Fields.string("addresses", v)
                val ip = Ipv4Address.parse(text.substringBefore('/', "")) ?: Fields.fail("addresses", "expected ip/prefix")
                val bits = text.substringAfter('/')
                if (bits.isEmpty() || !bits.all { it in '0'..'9' } || bits.trimStart('0').length > 2) Fields.fail("addresses", "prefix must be 0-32")
                val length = bits.toInt()
                if (length > 32) Fields.fail("addresses", "prefix must be 0-32")
                Ipv4Prefix(ip, length)
            }
            return VpnReady(addresses, f.mtu("mtu"))
        }
    }
}
