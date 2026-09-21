package dev.routedroid.ui

import dev.routedroid.session.StatusStore

/** Monospace status block for [MainActivity]. */
object StatusText {
    fun render(s: StatusStore.Status): String = buildString {
        appendLine("state:      ${s.state}")
        appendLine("session:    ${s.session ?: "-"}")
        appendLine("port:       ${s.devicePort ?: "-"}")
        appendLine("mtu:        ${s.mtu ?: "-"}")
        appendLine("addresses:  ${s.addresses.ifEmpty { listOf("-") }.joinToString(" ")}")
        appendLine("routes:     ${s.routes.ifEmpty { listOf("-") }.joinToString(" ")}")
        appendLine("dns:        ${s.dns.ifEmpty { listOf("-") }.joinToString(" ")}")
        appendLine("pkts out:   ${StatusStore.packetsOut.get()} (${StatusStore.bytesOut.get()} B)")
        appendLine("pkts in:    ${StatusStore.packetsIn.get()} (${StatusStore.bytesIn.get()} B)")
        appendLine("dropped:    out ${StatusStore.droppedOut.get()} / in ${StatusStore.droppedIn.get()}")
        appendLine("last error: ${s.lastError ?: "-"}")
    }
}
