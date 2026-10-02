pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "routedroid"
include(":app", ":protocol")

// The hostile app attacks the product's exported surface on a test device. It is never
// part of a release: build it with `./gradlew -Proutedroid.hostile :hostile:assembleDebug`.
if (providers.gradleProperty("routedroid.hostile").isPresent) {
    include(":hostile")
    project(":hostile").projectDir = file("testing/hostile")
}
