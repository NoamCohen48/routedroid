package dev.routedroid.link

import dev.routedroid.protocol.frame.Frame
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode

/**
 * Why a session ended: the one verdict every phase produces and the UI renders. The first
 * end recorded wins; later ones (the socket error a local stop causes) are consequences.
 */
sealed interface SessionEnd {
    // Ended on this phone.
    data object UserStopped : SessionEnd
    data object ConsentDenied : SessionEnd
    /** The VPN consent came too late for the host's §5 step 5 deadline, or never. */
    data object ConsentExpired : SessionEnd
    data object Revoked : SessionEnd
    /** A newer launch from the PC replaced this session. */
    data object Superseded : SessionEnd
    data class ConfigRejected(val detail: String) : SessionEnd
    data class EstablishFailed(val detail: String) : SessionEnd
    data class Failed(val detail: String) : SessionEnd

    // Ended by, or because of, the host.
    data object HostUnreachable : SessionEnd
    /** The host's proof did not verify: wrong or stale secret, or not our host at all. */
    data object AuthFailed : SessionEnd
    data object HostStopped : SessionEnd
    data object HostClosed : SessionEnd
    /** §5.1: nothing received for 30 s. */
    data object HostSilent : SessionEnd
    data class HostRefused(val code: String, val message: String) : SessionEnd
    data class TimedOut(val waitingFor: String) : SessionEnd
    data class Violation(val detail: String) : SessionEnd

    /** Thrown to end a session from inside a phase; the only exception type the link defines. */
    class Ended(val end: SessionEnd) : Exception(end.toString(), null, false, false)

    companion object {
        /**
         * The last frame Android sends for [end] to an authenticated host (§5 steps 6–7):
         * STOP for a user stop, nothing when the host ended it, VPN_ERROR for the rest.
         */
        fun lastFrame(end: SessionEnd): Frame? = when (end) {
            UserStopped -> Frame(MessageType.STOP)
            HostStopped, HostClosed, is HostRefused, HostUnreachable, AuthFailed -> null
            ConsentDenied -> error(ErrorCode.VPN_PERMISSION_DENIED, "user declined the VPN consent")
            Revoked -> error(ErrorCode.VPN_PERMISSION_DENIED, "VPN permission revoked")
            ConsentExpired -> error(ErrorCode.INTERNAL, "VPN consent came after the configure deadline")
            Superseded -> error(ErrorCode.INTERNAL, "replaced by a newer launch")
            HostSilent -> error(ErrorCode.INTERNAL, "no frame from the host for 30 s")
            is ConfigRejected -> error(ErrorCode.CONFIG_REJECTED, end.detail)
            is EstablishFailed -> error(ErrorCode.VPN_ESTABLISH_FAILED, end.detail)
            is Failed -> error(ErrorCode.INTERNAL, end.detail)
            is TimedOut -> error(ErrorCode.PROTOCOL_ERROR, "timed out waiting for ${end.waitingFor}")
            is Violation -> error(ErrorCode.PROTOCOL_ERROR, end.detail)
        }

        private fun error(code: ErrorCode, message: String) =
            Frame(MessageType.VPN_ERROR, ErrorBody.of(code, message).encode())
    }
}
