package dev.routedroid.link

import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.transport.PacketDevice
import java.net.Socket

/** What a session needs from the running VpnService; JVM tests supply a fake. */
interface VpnHost {
    /** Keeps [socket] off the VPN once it exists (VpnService.protect). */
    fun protect(socket: Socket): Boolean

    /** Applies [config] and returns the interface; throws [SessionEnd.Ended] on failure. */
    fun establish(config: ConfigureVpn): PacketDevice

    /** The session using this host is over: leave the foreground and stop. */
    fun release()
}
