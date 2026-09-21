package dev.routedroid.vpn

import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode

/** A failure the app reports to the host as VPN_ERROR (§4.6) before closing. */
class VpnFailure(val code: ErrorCode, message: String) : Exception(message) {
    fun body() = ErrorBody(code, (message ?: code.wire).take(ErrorBody.MAX_MESSAGE_LEN))
}
