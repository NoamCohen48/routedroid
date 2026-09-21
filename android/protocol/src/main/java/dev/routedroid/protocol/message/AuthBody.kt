package dev.routedroid.protocol.message

/** AUTH, Android → host (§4.3). */
data class AuthBody(val androidProof: String) {
    fun encode(): ByteArray = JsonOut().str("android_proof", androidProof).bytes()

    companion object {
        fun decode(body: ByteArray): AuthBody = Fields.decoding {
            AuthBody(Fields.hex("android_proof", Fields.str(Fields.parse(body), "android_proof"), 64))
        }
    }
}
