package dev.routedroid.vpn

import dev.routedroid.protocol.frame.FrameException
import dev.routedroid.protocol.frame.MessageType
import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.ErrorCode
import dev.routedroid.protocol.session.Allowlist
import dev.routedroid.protocol.session.Role
import dev.routedroid.protocol.session.State
import dev.routedroid.transport.ChannelInput

/** Negotiated → Configuring: wait for CONFIGURE_VPN and validate it (§5 step 5). */
object Configure {
    sealed class Outcome {
        class Config(val config: ConfigureVpn) : Outcome()
        object HostStop : Outcome()
        class HostError(val body: ErrorBody) : Outcome()
    }

    /** Blocking. Throws [VpnFailure] for anything the app must report as VPN_ERROR. */
    fun await(input: ChannelInput, mtu: Int): Outcome {
        val frame = try {
            input.readFrame(mtu) ?: throw VpnFailure(ErrorCode.PROTOCOL_ERROR, "host closed before CONFIGURE_VPN")
        } catch (e: FrameException) {
            throw VpnFailure(ErrorCode.PROTOCOL_ERROR, "bad frame: ${e.message}")
        }
        if (!Allowlist.isAllowed(Role.ANDROID, State.Negotiated, frame.type)) {
            throw VpnFailure(ErrorCode.PROTOCOL_ERROR, "${frame.type.wireName} is not legal while Negotiated")
        }
        return when (frame.type) {
            MessageType.STOP -> Outcome.HostStop
            MessageType.ERROR -> Outcome.HostError(decodeError(frame.body))
            else -> {
                val config = try {
                    ConfigureVpn.decode(frame.body)
                } catch (e: BodyException) {
                    throw VpnFailure(ErrorCode.CONFIG_REJECTED, "CONFIGURE_VPN: ${e.message}")
                }
                VpnConfigurator.check(config, mtu)
                Outcome.Config(config)
            }
        }
    }

    private fun decodeError(body: ByteArray): ErrorBody = try {
        ErrorBody.decode(body)
    } catch (e: BodyException) {
        throw VpnFailure(ErrorCode.PROTOCOL_ERROR, "ERROR body: ${e.message}")
    }
}
