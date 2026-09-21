// Wire protocol version 1 (protocol/version-1.md): framing, bodies, state
// allowlist, bootstrap record, mutual HMAC. Pure Kotlin, no Android APIs, so
// it is unit-tested on the JVM against protocol/fixtures/.
plugins {
    id("com.android.library")
}

android {
    namespace = "dev.routedroid.protocol"
    compileSdk = 36
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin { jvmToolchain(17) }
    sourceSets.getByName("test").resources.srcDir("../../protocol/fixtures")
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    // The real org.json for JVM tests; on device the platform copy is used.
    testImplementation("org.json:json:20250517")
}
