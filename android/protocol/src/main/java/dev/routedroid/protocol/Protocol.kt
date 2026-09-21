package dev.routedroid.protocol

/** Constants from protocol/version-1.md §2–§4. */
object Protocol {
    const val VERSION = 1
    const val HEADER_LEN = 8
    const val MAX_CONTROL_BODY = 65_536
    const val MIN_PACKET_BODY = 21
    const val MAX_PACKET_BODY = 65_535
    const val MIN_MTU = 576
    const val DEFAULT_MTU = 1400
    const val MAX_SESSION_LEN = 40

    /** §4.1: 1–40 characters from `A-Z a-z 0-9 . _ -`. */
    fun validSession(s: String): Boolean =
        s.isNotEmpty() && s.length <= MAX_SESSION_LEN &&
            s.all { it in 'A'..'Z' || it in 'a'..'z' || it in '0'..'9' || it == '.' || it == '_' || it == '-' }
}
