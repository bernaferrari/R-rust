import org.jetbrains.kotlin.gradle.targets.js.nodejs.NodeJsRootExtension
import org.jetbrains.kotlin.gradle.targets.js.yarn.YarnRootExtension

// Top-level build file
plugins {
    id("com.android.application") version "8.7.0" apply false
    id("org.jetbrains.kotlin.android") version "2.1.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.1.21" apply false
    id("org.jetbrains.kotlin.multiplatform") version "2.1.21" apply false
}

// Reuse a developer/CI Node installation for browser bundling. Kotlin/Wasm
// compilation itself does not require Node, but webpack and the dev server do.
gradle.projectsEvaluated {
    rootProject.extensions.findByType<NodeJsRootExtension>()?.apply {
        download = false
        nodeCommand = System.getenv("NODE_BINARY") ?: "node"
    }
    rootProject.extensions.findByType<YarnRootExtension>()?.apply {
        download = false
        command = System.getenv("YARN_BINARY") ?: "yarn"
    }
}
