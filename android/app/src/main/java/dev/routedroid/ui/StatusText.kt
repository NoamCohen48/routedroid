package dev.routedroid.ui

import android.content.Context
import dev.routedroid.R
import dev.routedroid.link.LinkState
import dev.routedroid.link.SessionEnd
import dev.routedroid.protocol.message.ErrorCode

/** [LinkState] and [SessionEnd] as user-facing text, all from string resources. */
class StatusText(private val context: Context) {
    fun headline(state: LinkState): String = context.getString(
        when (state) {
            is LinkState.Idle -> if (state.last?.isFailure == true) R.string.state_disconnected else R.string.state_idle
            LinkState.Authenticating -> R.string.state_authenticating
            LinkState.AwaitingConsent -> R.string.state_awaiting_consent
            LinkState.Configuring -> R.string.state_configuring
            is LinkState.Active -> R.string.state_active
            LinkState.Stopping -> R.string.state_stopping
        },
    )

    fun detail(state: LinkState): String = when (state) {
        is LinkState.Idle -> state.last?.let(::reason) ?: context.getString(R.string.detail_idle)
        is LinkState.Active -> context.getString(R.string.detail_active, state.config.address.address.toString())
        else -> ""
    }

    /** What to do next, for an end the user can act on. */
    fun hint(state: LinkState): String? =
        if (state is LinkState.Idle && state.last?.isFailure == true) context.getString(R.string.hint_retry) else null

    fun reason(end: SessionEnd): String = when (end) {
        SessionEnd.UserStopped -> context.getString(R.string.end_user_stopped)
        SessionEnd.ConsentDenied -> context.getString(R.string.end_consent_denied)
        SessionEnd.ConsentExpired -> context.getString(R.string.end_consent_expired)
        SessionEnd.Revoked -> context.getString(R.string.end_revoked)
        SessionEnd.Superseded -> context.getString(R.string.end_superseded)
        is SessionEnd.ConfigRejected -> context.getString(R.string.end_config_rejected, end.detail)
        is SessionEnd.EstablishFailed -> context.getString(R.string.end_establish_failed, end.detail)
        is SessionEnd.Failed -> context.getString(R.string.end_failed, end.detail)
        SessionEnd.HostUnreachable -> context.getString(R.string.end_host_unreachable)
        SessionEnd.AuthFailed -> context.getString(R.string.end_auth_failed)
        SessionEnd.HostStopped -> context.getString(R.string.end_host_stopped)
        SessionEnd.HostClosed -> context.getString(R.string.end_host_closed)
        SessionEnd.HostSilent -> context.getString(R.string.end_host_silent)
        // consent_timeout is the host's side of the same deadline as ConsentExpired.
        is SessionEnd.HostRefused ->
            if (end.code == ErrorCode.CONSENT_TIMEOUT.wire) context.getString(R.string.end_consent_expired)
            else context.getString(R.string.end_host_refused, end.code)
        is SessionEnd.TimedOut -> context.getString(R.string.end_timed_out, end.waitingFor)
        is SessionEnd.Violation -> context.getString(R.string.end_violation, end.detail)
    }
}

/** Ends nobody needs to act on: someone chose them. */
val SessionEnd.isFailure: Boolean
    get() = this != SessionEnd.UserStopped && this != SessionEnd.HostStopped &&
        this != SessionEnd.ConsentDenied && this != SessionEnd.Superseded
