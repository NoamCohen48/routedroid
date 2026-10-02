package dev.routedroid.transport

import java.util.concurrent.atomic.AtomicLong

/** Per-session counters; each is written by one packet-path thread and read by the UI. */
class Traffic {
    val packetsOut = AtomicLong()
    val bytesOut = AtomicLong()
    val packetsIn = AtomicLong()
    val bytesIn = AtomicLong()
    /** Read from the VPN interface but not IPv4 (IPv6, stray ND): never sent. */
    val droppedOut = AtomicLong()
    /** Read from the VPN interface but larger than the negotiated MTU: never sent. */
    val oversizeOut = AtomicLong()
    /** Received from the host but failing the §6 checks: never injected. */
    val droppedIn = AtomicLong()
}
