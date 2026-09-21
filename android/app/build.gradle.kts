// Routedroid Android app: VpnService end of the version-1 wire protocol.
// All protocol parsing lives in :protocol so it is fixture-tested on the JVM.
plugins {
    id("com.android.application")
}

android {
    namespace = "dev.routedroid"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.routedroid"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1-phase1"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        jvmToolchain(17)
    }
}

dependencies {
    implementation(project(":protocol"))
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250517")
}
