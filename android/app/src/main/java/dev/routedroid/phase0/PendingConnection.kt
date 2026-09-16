package dev.routedroid.phase0

import java.nio.channels.SocketChannel

/**
 * Hands the already-authenticated host connection from [BootstrapActivity] to
 * [Phase0VpnService] (same process). Single slot, consumed once.
 */
object PendingConnection {
    private var channel: SocketChannel? = null
    private var session: String? = null
    private val lock = Any()

    fun put(session: String, ch: SocketChannel) = synchronized(lock) {
        channel?.let { runCatching { it.close() } }
        channel = ch; this.session = session
    }

    fun take(session: String): SocketChannel? = synchronized(lock) {
        val ch = channel ?: return null
        channel = null
        if (this.session != session) { runCatching { ch.close() }; return null }
        ch
    }

    fun discard() = synchronized(lock) { channel?.let { runCatching { it.close() } }; channel = null }
}
