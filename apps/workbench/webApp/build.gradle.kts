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
    "dist/wasmJs/productionExecutable",
)

tasks.register("checkWasmProductionBundleSize") {
    group = "verification"
    description = "Builds the production web app and enforces release asset budgets."
    dependsOn("wasmJsBrowserDistribution")

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

// Ship the Rust interpreter beside the Kotlin UI, with its own asset budget.
val rustRuntimeResources = layout.buildDirectory.dir("generated/rustRuntimeResources")
val buildRustRuntime by tasks.registering(Exec::class) {
    val repository = rootProject.projectDir.parentFile.parentFile
    workingDir(repository)
    inputs.files(fileTree(repository.resolve("crates")) { include("**/*.rs", "**/*.R", "**/*.ttf", "**/Cargo.toml"); exclude("**/target/**") })
    inputs.files(repository.resolve("Cargo.lock"), repository.resolve("Cargo.toml"), repository.resolve("scripts/build_wasm_runtime.sh"))
    outputs.dir(rustRuntimeResources)
    commandLine("bash", "scripts/build_wasm_runtime.sh", "--target", "web", "--release",
        "--out-dir", rustRuntimeResources.get().dir("rust-runtime").asFile.absolutePath)
}
kotlin.sourceSets.named("wasmJsMain") {
    resources.srcDir(rustRuntimeResources)
}
tasks.named("wasmJsProcessResources") { dependsOn(buildRustRuntime) }
tasks.named("checkWasmProductionBundleSize") {
    doLast {
        val runtime = productionBundleDirectory.get().dir("rust-runtime").asFile
        val wasm = runtime.resolve("r_wasm_bg.wasm")
        check(wasm.isFile) { "Rust interpreter WASM is missing from the release bundle" }
        val total = runtime.walkTopDown().filter { it.isFile }.sumOf { it.length() }
        check(total <= 25L * 1024 * 1024) { "Rust runtime assets exceed the 25 MiB budget: $total bytes" }
        logger.lifecycle("Rust interpreter assets: {} bytes (separate from Kotlin UI budget)", total)
    }
}
