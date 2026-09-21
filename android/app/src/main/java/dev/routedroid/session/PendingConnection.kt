package dev.routedroid.session

import java.nio.channels.SocketChannel

/**
 * Hands the authenticated host connection (state Negotiated) plus the negotiated MTU from
 * BootstrapActivity to the VPN service in the same process. Single slot, consumed once.
 */
object PendingConnection {
    class Handoff(val session: String, val channel: SocketChannel, val mtu: Int)

    private var slot: Handoff? = null
    private val lock = Any()

    fun put(h: Handoff) = synchronized(lock) {
        slot?.let { runCatching { it.channel.close() } }
        slot = h
    }

    fun take(session: String): Handoff? = synchronized(lock) {
        val h = slot ?: return null
        slot = null
        if (h.session != session) { runCatching { h.channel.close() }; return null }
        h
    }

    fun discard() = synchronized(lock) { slot?.let { runCatching { it.channel.close() } }; slot = null }
}
