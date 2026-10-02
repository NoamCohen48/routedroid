package dev.routedroid.protocol

/** Constants from protocol/version-1.md. */
object Protocol {
    const val VERSION = 1
    const val HEADER_LEN = 8
    const val MAX_CONTROL_BODY = 65_536
    const val MIN_PACKET_BODY = 20
    const val MAX_PACKET_BODY = 65_535
    const val MIN_MTU = 576
    const val MAX_SESSION_LEN = 40

    /** §5 step 5: the host gives up this long after AUTH; the app must not establish later. */
    const val CONFIGURE_DEADLINE_MS = 120_000L

    /** §4.1: 1–40 characters from `A-Z a-z 0-9 . _ -`. */
    fun validSession(s: String): Boolean =
        s.isNotEmpty() && s.length <= MAX_SESSION_LEN &&
            s.all { it in 'A'..'Z' || it in 'a'..'z' || it in '0'..'9' || it == '.' || it == '_' || it == '-' }
}
