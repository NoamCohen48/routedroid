package dev.routedroid.protocol.session

import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.frame.MessageType.AUTH
import dev.routedroid.protocol.frame.MessageType.CONFIGURE_VPN
import dev.routedroid.protocol.frame.MessageType.ERROR
import dev.routedroid.protocol.frame.MessageType.HELLO
import dev.routedroid.protocol.frame.MessageType.HELLO_ACK
import dev.routedroid.protocol.frame.MessageType.IP_PACKET
import dev.routedroid.protocol.frame.MessageType.PING
import dev.routedroid.protocol.frame.MessageType.PONG
import dev.routedroid.protocol.frame.MessageType.STOP
import dev.routedroid.protocol.frame.MessageType.VPN_ERROR
import dev.routedroid.protocol.frame.MessageType.VPN_READY
import java.util.EnumSet

/** Session states (§5). */
enum class State { Connected, Authenticating, Negotiated, Configuring, Active, Closed }

enum class Role { HOST, ANDROID }

/** Message types a role may *receive* in each state (§5); checked against fixtures/states.json. */
object Allowlist {
    private val host = mapOf(
        State.Connected to EnumSet.of(HELLO, STOP),
        State.Authenticating to EnumSet.of(AUTH, STOP),
        State.Negotiated to EnumSet.of(STOP),
        State.Configuring to EnumSet.of(VPN_READY, VPN_ERROR, STOP),
        State.Active to EnumSet.of(IP_PACKET, PING, PONG, STOP, VPN_ERROR),
        State.Closed to EnumSet.noneOf(MessageType::class.java),
    )
    private val android = mapOf(
        State.Connected to EnumSet.of(STOP),
        State.Authenticating to EnumSet.of(HELLO_ACK, ERROR, STOP),
        State.Negotiated to EnumSet.of(CONFIGURE_VPN, ERROR, STOP),
        State.Configuring to EnumSet.of(ERROR, STOP),
        State.Active to EnumSet.of(IP_PACKET, PING, PONG, STOP, ERROR),
        State.Closed to EnumSet.noneOf(MessageType::class.java),
    )

    fun allowed(role: Role, state: State): Set<MessageType> = (if (role == Role.HOST) host else android).getValue(state)

    fun isAllowed(role: Role, state: State, type: MessageType) = type in allowed(role, state)
}
