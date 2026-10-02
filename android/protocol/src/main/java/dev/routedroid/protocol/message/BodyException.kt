package dev.routedroid.protocol.message

/** A control body that is not strict JSON or breaks a §4 field rule. */
class BodyException(message: String) : Exception(message)
