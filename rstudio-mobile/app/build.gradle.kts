plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val repositoryRoot = rootProject.projectDir.parentFile
val generatedDebugJni = layout.buildDirectory.dir("generated/jniLibs/debug")
val generatedReleaseJni = layout.buildDirectory.dir("generated/jniLibs/release")

fun registerRustAndroidTask(name: String, release: Boolean, outputDir: Provider<Directory>) =
    tasks.register<Exec>(name) {
        group = "build"
        description = "Build the Rust R runtime for Android (${if (release) "release" else "debug"})"
        workingDir(repositoryRoot)
        val arguments = mutableListOf(
            "ndk",
            "-t", "arm64-v8a",
            "-o", outputDir.get().asFile.absolutePath,
            "build",
            "-p", "r-uniffi",
        )
        if (release) arguments += "--release"
        commandLine("cargo", *arguments.toTypedArray())
        inputs.files(
            fileTree(repositoryRoot.resolve("crates")) { include("**/*.rs", "**/Cargo.toml") },
            fileTree(repositoryRoot.resolve("rmath-rs")) { include("**/*.rs", "**/Cargo.toml") },
            repositoryRoot.resolve("Cargo.toml"),
            repositoryRoot.resolve("Cargo.lock"),
        )
        outputs.files(
            outputDir.map { it.file("arm64-v8a/libr_uniffi.so") },
        )
        doLast {
            outputDir.get().asFile.walkTopDown()
                .filter { it.isFile && it.extension == "so" && it.name != "libr_uniffi.so" }
                .forEach { extraLibrary ->
                    check(extraLibrary.delete()) { "Could not remove unrelated native library ${extraLibrary.absolutePath}" }
                }
            outputs.files.files.forEach { library ->
                check(library.isFile && library.length() > 0L) {
                    "Rust Android library was not produced: ${library.absolutePath}"
                }
            }
        }
    }

val buildRustAndroidDebug = registerRustAndroidTask(
    name = "buildRustAndroidDebug",
    release = false,
    outputDir = generatedDebugJni,
)
val buildRustAndroidRelease = registerRustAndroidTask(
    name = "buildRustAndroidRelease",
    release = true,
    outputDir = generatedReleaseJni,
)

android {
    namespace = "com.rstudio.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.rstudio.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters += "arm64-v8a"
        }

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables {
            useSupportLibrary = true
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/java", "generated/kotlin")
            jniLibs.srcDirs("src/main/jniLibs")
        }
        getByName("debug").jniLibs.srcDir(generatedDebugJni)
        getByName("release").jniLibs.srcDir(generatedReleaseJni)
    }
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

tasks.matching { it.name in setOf("mergeDebugJniLibFolders", "mergeDebugNativeLibs") }.configureEach {
    dependsOn(buildRustAndroidDebug)
}
tasks.matching { it.name in setOf("mergeReleaseJniLibFolders", "mergeReleaseNativeLibs") }.configureEach {
    dependsOn(buildRustAndroidRelease)
}

dependencies {
    implementation(project(":shared"))
    // Core
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.activity:activity-compose:1.9.3")

    // Compose BOM
    implementation(platform("androidx.compose:compose-bom:2025.03.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material3:material3-window-size-class")

    implementation("androidx.annotation:annotation:1.9.1")
    implementation("androidx.documentfile:documentfile:1.1.0")
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    // Icons
    implementation("androidx.compose.material:material-icons-extended:1.7.5")

    // Test
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.03.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
