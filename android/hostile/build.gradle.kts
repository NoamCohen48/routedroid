plugins {
    id("com.android.application")
}

android {
    namespace = "dev.routedroid.hostile"
    compileSdk = 36
    defaultConfig {
        applicationId = "dev.routedroid.hostile"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.0-phase0"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin { jvmToolchain(17) }
}
