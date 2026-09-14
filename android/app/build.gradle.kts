plugins {
    id("com.android.application")
}

android {
    namespace = "dev.routedroid.phase0"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.routedroid.phase0"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.0-phase0"
    }

    buildTypes {
        release {
            // Throwaway probe: never shipped, so no shrinking/signing config.
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

    testOptions {
        unitTests.isReturnDefaultValues = false
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")

    testImplementation("junit:junit:4.13.2")
}
