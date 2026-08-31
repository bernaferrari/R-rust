pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    // Keep toolchain repositories explicit. PREFER_SETTINGS ignores the transient
    // project-level Ivy repository that Kotlin creates for Binaryen, so declare the
    // same pinned distribution layout here and restrict it to that one module.
    repositoriesMode.set(RepositoriesMode.PREFER_SETTINGS)
    repositories {
        google()
        mavenCentral()
        ivy {
            name = "KotlinBinaryen"
            url = uri("https://github.com/WebAssembly/binaryen/releases/download")
            patternLayout {
                artifact("version_[revision]/binaryen-version_[revision]-[classifier].[ext]")
            }
            metadataSources {
                artifact()
            }
            content {
                includeModule("com.github.webassembly", "binaryen")
            }
        }
    }
}

rootProject.name = "rstudio-mobile"
include(":app")
include(":shared")
include(":webApp")
