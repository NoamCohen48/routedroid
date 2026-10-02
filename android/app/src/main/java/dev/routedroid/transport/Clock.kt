package dev.routedroid.transport

/** Milliseconds on a monotonic clock; injected so deadlines and keepalive are testable. */
fun interface Clock {
    fun now(): Long

    companion object {
        /**
         * CLOCK_MONOTONIC, which stops while the phone is suspended: a device that slept
         * through the host's PINGs has not seen the host go quiet (§5.1).
         */
        val UPTIME = Clock { System.nanoTime() / 1_000_000 }
    }
}
