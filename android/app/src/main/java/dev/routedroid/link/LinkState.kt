package dev.routedroid.link

import dev.routedroid.protocol.message.ConfigureVpn

/** What the phone's link to the PC is doing; [DeviceLink.state] is the only source. */
sealed interface LinkState {
    /** No session; [last] says how the previous one ended, null before the first. */
    data class Idle(val last: SessionEnd?) : LinkState
    data object Authenticating : LinkState
    data object AwaitingConsent : LinkState
    data object Configuring : LinkState
    data class Active(val config: ConfigureVpn) : LinkState
    data object Stopping : LinkState

    /** Whether a user Stop has anything to stop. */
    val canStop: Boolean get() = this !is Idle && this != Stopping
}
