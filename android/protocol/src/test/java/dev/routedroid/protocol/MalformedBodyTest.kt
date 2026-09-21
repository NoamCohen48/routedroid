package dev.routedroid.protocol

import dev.routedroid.protocol.message.BodyException
import dev.routedroid.protocol.message.ConfigureVpn
import dev.routedroid.protocol.message.ErrorBody
import dev.routedroid.protocol.message.HelloAck
import org.junit.Assert.assertThrows
import org.junit.Test

/** Every decoder failure must be a [BodyException], never a raw org.json exception. */
class MalformedBodyTest {
    private fun bytes(s: String) = s.toByteArray(Charsets.UTF_8)

    @Test fun nonIntegerElementsAreBodyExceptions() {
        assertThrows(BodyException::class.java) { ErrorBody.decode(bytes("""{"code":"x","message":"m","supported":["one"]}""")) }
        assertThrows(BodyException::class.java) {
            ConfigureVpn.decode(bytes("""{"mtu":1400,"addresses":[1],"routes":[{"address":"0.0.0.0","prefix":0}],"dns":[],"session_name":"s"}"""))
        }
        assertThrows(BodyException::class.java) {
            ConfigureVpn.decode(bytes("""{"mtu":1400,"addresses":[{"address":"10.0.0.2","prefix":32}],"routes":[{"address":"0.0.0.0","prefix":0}],"dns":[7],"session_name":"s"}"""))
        }
    }

    @Test fun integersMustBeIntegral() {
        assertThrows(BodyException::class.java) { HelloAck.decode(bytes("""{"protocol":1.9,"mtu":1400,"host_nonce":"${"a".repeat(64)}","host_proof":"${"b".repeat(64)}"}""")) }
        assertThrows(BodyException::class.java) { HelloAck.decode(bytes("""{"protocol":1,"mtu":99999999999,"host_nonce":"${"a".repeat(64)}","host_proof":"${"b".repeat(64)}"}""")) }
    }
}
