// A throwaway third-party app that attacks the product's exported surface; see Probes.kt.
plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "dev.routedroid.hostile"
    compileSdk = 36
    defaultConfig {
        applicationId = "dev.routedroid.hostile"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin { jvmToolchain(17) }
}

dependencies {
    // For the record layout and the provider URI only: an attacker could copy both.
    implementation(project(":protocol"))
}
