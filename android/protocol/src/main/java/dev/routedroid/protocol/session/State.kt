package dev.routedroid.protocol.session

/** Session states (§5). */
enum class State { Connected, Authenticating, Negotiated, Configuring, Active, Closed }

enum class Role { HOST, ANDROID }
