// Routedroid Android app: the VpnService end of the version-1 wire protocol. The wire
// itself lives in :protocol; this module is the session owner (link/), the packet path
// (transport/) and the Android shells around them (bootstrap/, vpn/, ui/).
plugins {
    alias(libs.plugins.android.application)
}

// One product version for the host and the phone: [workspace.package] in host/Cargo.toml.
// HELLO's `app` field carries it, so the host can tell an outdated app apart.
val productVersion: String = rootDir.resolve("../host/Cargo.toml").readLines()
    .dropWhile { it.trim() != "[workspace.package]" }
    .firstNotNullOfOrNull { Regex("""^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"""").find(it)?.groupValues?.drop(1) }
    ?.joinToString(".")
    ?: error("no [workspace.package] version in host/Cargo.toml")

android {
    namespace = "dev.routedroid"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.routedroid"
        minSdk = 26
        targetSdk = 36
        val (major, minor, patch) = productVersion.split('.').map(String::toInt)
        versionCode = major * 1_000_000 + minor * 1_000 + patch
        versionName = productVersion
    }

    buildFeatures { buildConfig = true }

    // Release signing is opt-in: with no key configured, assembleRelease still builds an
    // unsigned APK to sign elsewhere. Each value comes from a Gradle property
    // (-Proutedroid.signing.storeFile=…, or ~/.gradle/gradle.properties) or the matching
    // ROUTEDROID_SIGNING_* environment variable, never from the repository.
    fun signing(name: String): String? = providers.gradleProperty("routedroid.signing.$name")
        .orElse(providers.environmentVariable("ROUTEDROID_SIGNING_" + name.replace(Regex("([A-Z])"), "_$1").uppercase()))
        .orNull
    val storeFile = signing("storeFile")
    if (storeFile != null) {
        signingConfigs.create("release") {
            this.storeFile = file(storeFile)
            storePassword = signing("storePassword") ?: error("routedroid.signing.storePassword is not set")
            keyAlias = signing("keyAlias") ?: error("routedroid.signing.keyAlias is not set")
            keyPassword = signing("keyPassword") ?: storePassword
        }
    }

    buildTypes {
        release {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin { jvmToolchain(17) }

    lint {
        abortOnError = true
        warningsAsErrors = true
        checkReleaseBuilds = true
        // Version currency is reviewed deliberately, not whenever a new release appears.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion", "OldTargetApi")
    }

    // link/ and transport/ are plain JVM code tested over real loopback sockets; the few
    // android.util.Log calls they reach return defaults there.
    testOptions { unitTests.isReturnDefaultValues = true }
}

dependencies {
    implementation(project(":protocol"))
    implementation(libs.androidx.core)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.activity)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.material)
    implementation(libs.coroutines.android)

    testImplementation(libs.junit)
}
