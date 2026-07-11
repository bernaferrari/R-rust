pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    // Kotlin/Wasm's Node distribution plugin adds its own pinned Node repository.
    // Prefer the shared repositories while allowing that platform-specific source.
    repositoriesMode.set(RepositoriesMode.PREFER_SETTINGS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "rstudio-mobile"
include(":app")
include(":shared")
include(":webApp")
