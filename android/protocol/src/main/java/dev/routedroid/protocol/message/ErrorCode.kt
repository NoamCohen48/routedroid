package dev.routedroid.protocol.message

/** §4.6 error codes. Unknown codes from a newer peer stay strings in [ErrorBody]. */
enum class ErrorCode(val wire: String) {
    PROTOCOL_UNSUPPORTED("protocol_unsupported"),
    PROTOCOL_ERROR("protocol_error"),
    AUTH_FAILED("auth_failed"),
    SESSION_MISMATCH("session_mismatch"),
    TRANSPORT_UNSUPPORTED("transport_unsupported"),
    VPN_PERMISSION_DENIED("vpn_permission_denied"),
    VPN_ESTABLISH_FAILED("vpn_establish_failed"),
    CONFIG_REJECTED("config_rejected"),
    CONSENT_TIMEOUT("consent_timeout"),
    INTERNAL("internal");

    companion object {
        fun parse(s: String): ErrorCode? = entries.firstOrNull { it.wire == s }
    }
}
