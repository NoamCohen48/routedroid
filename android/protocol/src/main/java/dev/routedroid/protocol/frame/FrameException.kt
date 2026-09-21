package dev.routedroid.protocol.frame

/** Header rejection (§2). [code] is the name used by fixtures/frames.json. */
class FrameException(val code: String, message: String) : Exception(message) {
    companion object {
        const val UNSUPPORTED_VERSION = "unsupported_version"
        const val NONZERO_FLAGS = "nonzero_flags"
        const val UNKNOWN_TYPE = "unknown_type"
        const val CONTROL_BODY_TOO_LARGE = "control_body_too_large"
        const val UNEXPECTED_BODY = "unexpected_body"
        const val EMPTY_BODY = "empty_body"
        const val PACKET_BODY_OUT_OF_RANGE = "packet_body_out_of_range"
        const val TRUNCATED = "truncated"
    }
}
