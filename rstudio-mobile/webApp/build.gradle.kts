import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl
import org.jetbrains.kotlin.gradle.targets.js.nodejs.NodeJsEnvSpec

plugins {
    id("org.jetbrains.kotlin.multiplatform")
}

@OptIn(ExperimentalWasmDsl::class)
kotlin {
    wasmJs {
        browser {
            commonWebpackConfig {
                outputFileName = "r-workbench.js"
            }
        }
        nodejs()
        binaries.executable()
    }

    sourceSets {
        commonMain.dependencies {
            implementation(kotlin("stdlib"))
        }
        wasmJsMain.dependencies {
            implementation(project(":shared"))
            implementation("org.jetbrains.kotlinx:kotlinx-browser-wasm-js:0.3.1")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core-wasm-js:1.9.0")
            implementation(npm("webr", "0.6.0"))
        }
        wasmJsTest.dependencies {
            implementation(kotlin("test"))
        }
    }
}

extensions.configure<NodeJsEnvSpec>("kotlinNodeJsSpec") {
    download.set(false)
    command.set(System.getenv("NODE_BINARY") ?: "node")
}
