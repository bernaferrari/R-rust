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

val productionBundleDirectory = layout.buildDirectory.dir(
    "kotlin-webpack/wasmJs/productionExecutable",
)

tasks.register("checkWasmProductionBundleSize") {
    group = "verification"
    description = "Builds the production web app and enforces release asset budgets."
    dependsOn("wasmJsBrowserProductionWebpack")

    val bundleDirectory = productionBundleDirectory
    inputs.dir(bundleDirectory)

    doLast {
        val directory = bundleDirectory.get().asFile
        val javascript = directory.resolve("r-workbench.js")
        val wasmFiles = directory.listFiles { file ->
            file.isFile && file.extension == "wasm"
        }?.toList().orEmpty()

        check(javascript.isFile) {
            "Production JavaScript bundle is missing: ${javascript.absolutePath}"
        }
        check(wasmFiles.size == 1) {
            "Expected exactly one production Wasm asset in ${directory.absolutePath}, " +
                "found ${wasmFiles.size}"
        }

        val wasm = wasmFiles.single()
        val javascriptBudget = 100L * 1024
        val wasmBudget = 350L * 1024
        val totalBudget = 450L * 1024
        val totalSize = javascript.length() + wasm.length()

        check(javascript.length() <= javascriptBudget) {
            "JavaScript bundle is ${javascript.length()} bytes; budget is $javascriptBudget bytes"
        }
        check(wasm.length() <= wasmBudget) {
            "Wasm bundle is ${wasm.length()} bytes; budget is $wasmBudget bytes"
        }
        check(totalSize <= totalBudget) {
            "Production bundle is $totalSize bytes; budget is $totalBudget bytes"
        }

        logger.lifecycle(
            "Production bundle: JS={} bytes, Wasm={} bytes, total={} bytes",
            javascript.length(),
            wasm.length(),
            totalSize,
        )
    }
}
