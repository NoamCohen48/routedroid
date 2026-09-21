package dev.routedroid.vpn

import android.net.VpnService
import android.os.ParcelFileDescriptor
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorCode
import java.net.InetAddress

/**
 * Turns a CONFIGURE_VPN body into an established VPN interface. The body already passed the
 * §4.4 field rules in `ConfigureVpn.decode`; what remains is the MTU equality rule and the
 * builder itself.
 */
object VpnConfigurator {
    fun check(config: ConfigureVpn, negotiatedMtu: Int) {
        if (config.mtu != negotiatedMtu) {
            throw VpnFailure(ErrorCode.CONFIG_REJECTED, "CONFIGURE_VPN mtu ${config.mtu} != negotiated $negotiatedMtu")
        }
    }

    fun establish(builder: VpnService.Builder, config: ConfigureVpn): ParcelFileDescriptor {
        try {
            builder.setSession(config.sessionName)
            builder.setMtu(config.mtu)
            for (p in config.addresses) builder.addAddress(ipv4(p.address), p.prefix)
            for (p in config.routes) builder.addRoute(ipv4(p.address), p.prefix)
            for (d in config.dns) builder.addDnsServer(ipv4(d))
            builder.setBlocking(true)
            // Deliberately no allowBypass(): architecture.md §7.
        } catch (e: IllegalArgumentException) {
            throw VpnFailure(ErrorCode.CONFIG_REJECTED, "builder rejected config: ${e.message}")
        }
        val pfd = try {
            builder.establish()
        } catch (e: SecurityException) {
            throw VpnFailure(ErrorCode.VPN_PERMISSION_DENIED, "establish: ${e.message}")
        } catch (e: IllegalStateException) {
            throw VpnFailure(ErrorCode.VPN_ESTABLISH_FAILED, "establish: ${e.message}")
        }
        return pfd ?: throw VpnFailure(ErrorCode.VPN_PERMISSION_DENIED, "establish() returned null (not prepared or revoked)")
    }

    /** Already validated as dotted-quad by the protocol layer; never resolves names. */
    private fun ipv4(text: String): InetAddress =
        InetAddress.getByAddress(text.split('.').map { it.toInt().toByte() }.toByteArray())
}
