import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val workspaceRoot = rootProject.projectDir.parentFile.parentFile
val picooToolchain = Properties().apply {
    rootProject.file("toolchain.properties").inputStream().use(::load)
}
val picooNdkVersion = picooToolchain.getProperty("PICOO_ANDROID_NDK_VERSION")
    ?: error("PICOO_ANDROID_NDK_VERSION is missing from toolchain.properties")
val picooBuildToolsVersion = picooToolchain.getProperty("PICOO_ANDROID_BUILD_TOOLS")
    ?: error("PICOO_ANDROID_BUILD_TOOLS is missing from toolchain.properties")
val picooCompileSdk = picooToolchain.getProperty("PICOO_ANDROID_PLATFORM")
    ?.removePrefix("android-")
    ?.toIntOrNull()
    ?: error("PICOO_ANDROID_PLATFORM must use the android-<api> form")

android {
    namespace = "com.picoo.camera"
    compileSdk = picooCompileSdk
    buildToolsVersion = picooBuildToolsVersion
    // NDK r28+ ships 16 KB-aligned libc++ and honors flexible page sizes (Xiaomi 15 / Android 15).
    ndkVersion = picooNdkVersion

    defaultConfig {
        applicationId = "com.picoo.camera"
        minSdk = 29
        targetSdk = picooCompileSdk
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }

    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Pre-signing local/CI artifacts (PRD §19 — 签名前可用).
            signingConfig = signingConfigs.getByName("debug")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
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

    packaging {
        jniLibs {
            // Extract .so so 16 KB-page devices can mmap with correct ELF p_align.
            useLegacyPackaging = true
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.6")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}

val rustAbi = "arm64-v8a"
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")

val checkAndroidRustToolchain by tasks.registering(Exec::class) {
    group = "verification"
    description = "Verify the pinned cargo-ndk and Android NDK toolchain"
    workingDir = workspaceRoot
    commandLine("bash", workspaceRoot.resolve("scripts/check_android_toolchain.sh"))
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
}

tasks.register<Exec>("cargoBuildFfi") {
    group = "build"
    description = "Build picoo-ffi Rust cdylib for Android arm64"
    workingDir = workspaceRoot
    dependsOn(checkAndroidRustToolchain)
    doFirst {
        jniLibsDir.asFile.mkdirs()
        environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    }
    commandLine(
        "cargo",
        "ndk",
        "-t",
        rustAbi,
        "-o",
        jniLibsDir.asFile.absolutePath,
        "build",
        "--release",
        "-p",
        "picoo-ffi",
    )
    // Ensure the single Rust/JNI cdylib has 16 KB-aligned LOAD segments.
    environment(
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
        "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-soname,libpicoo_ffi.so",
    )
}

// Only APK/AAB JNI merge tasks need the Rust shared library. Pure JVM unit tests
// stay independent from the NDK and cargo-ndk toolchain.
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
    dependsOn("cargoBuildFfi")
}
