package dev.routedroid.vpn

import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import dev.routedroid.link.SessionEnd
import dev.routedroid.link.SessionEnd.Ended
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.net.Ipv4Address
import java.net.InetAddress

/** A decoded CONFIGURE_VPN onto a VpnService.Builder; every value is already typed and valid. */
object VpnBuilderConfig {
    fun establish(builder: VpnService.Builder, config: ConfigureVpn): ParcelFileDescriptor {
        try {
            builder.setSession(config.sessionName)
            builder.setMtu(config.mtu)
            builder.addAddress(inet(config.address.address), config.address.length)
            for (r in config.routes) builder.addRoute(inet(r.address), r.length)
            for (d in config.dns) builder.addDnsServer(inet(d))
            builder.setBlocking(true)
            // The tunnel ends at the PC, not at the phone's Wi-Fi or cellular network, and
            // costs nothing. No IPv6 address or route is added and allowFamily() is not
            // called, so the platform blocks IPv6 while the VPN is up: this is an IPv4 host.
            // No allowBypass() either: architecture.md §7.
            builder.setUnderlyingNetworks(emptyArray())
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) builder.setMetered(false)
        } catch (e: IllegalArgumentException) {
            throw Ended(SessionEnd.ConfigRejected("VpnService.Builder: ${e.message}"))
        }
        val pfd = try {
            builder.establish()
        } catch (e: RuntimeException) {
            // SecurityException (not prepared), IllegalStateException, IllegalArgumentException.
            throw Ended(SessionEnd.EstablishFailed("establish(): $e"))
        }
        return pfd ?: throw Ended(SessionEnd.EstablishFailed("establish() returned null: consent missing or revoked"))
    }

    private fun inet(a: Ipv4Address): InetAddress = InetAddress.getByAddress(a.toBytes())
}
