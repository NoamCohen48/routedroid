package dev.routedroid.protocol.session

import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.frame.MessageType.*

/** Message types a role may *receive* in each state (§5); checked against fixtures/states.json. */
object Allowlist {
    private val host: Map<State, Set<MessageType>> = mapOf(
        State.Connected to setOf(HELLO, STOP),
        State.Authenticating to setOf(AUTH, STOP),
        State.Negotiated to setOf(STOP),
        State.Configuring to setOf(VPN_READY, VPN_ERROR, STOP),
        State.Active to setOf(IP_PACKET, PING, PONG, STOP, VPN_ERROR),
        State.Closed to emptySet(),
    )
    private val android: Map<State, Set<MessageType>> = mapOf(
        State.Connected to setOf(STOP),
        State.Authenticating to setOf(HELLO_ACK, ERROR, STOP),
        State.Negotiated to setOf(CONFIGURE_VPN, ERROR, STOP),
        State.Configuring to setOf(ERROR, STOP),
        State.Active to setOf(IP_PACKET, PING, PONG, STOP, ERROR),
        State.Closed to emptySet(),
    )

    fun allowed(role: Role, state: State): Set<MessageType> =
        (if (role == Role.HOST) host else android).getValue(state)

    fun isAllowed(role: Role, state: State, type: MessageType) = type in allowed(role, state)
}
