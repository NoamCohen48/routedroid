// Three modules: :protocol (pure Kotlin/JVM wire protocol), :app (the product) and
// :hostile (a test-only attacker app, see settings.gradle.kts).
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.jvm) apply false
}
