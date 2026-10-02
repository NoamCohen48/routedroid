// Wire protocol version 1 (protocol/version-1.md): framing, the frame reader, bodies with
// one strict JSON codec, the state allowlist, the bootstrap record and the mutual HMAC.
// A plain JVM module: the bytes it reads and writes in its tests are the bytes the phone
// reads and writes, because no platform library is involved.
plugins {
    alias(libs.plugins.kotlin.jvm)
}

kotlin { jvmToolchain(17) }

sourceSets.test { resources.srcDir("../../protocol/fixtures") }

dependencies {
    testImplementation(libs.junit)
}
