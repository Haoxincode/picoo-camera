plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val workspaceRoot = rootProject.projectDir.parentFile.parentFile

// Default remains arm64-v8a for devices. Cloud / Linux emulator uses x86_64:
//   PICOO_ANDROID_ABIS=x86_64 ./apps/android/gradlew -p apps/android assembleDebug
val rustAbis =
    (System.getenv("PICOO_ANDROID_ABIS") ?: "arm64-v8a")
        .split(',')
        .map { it.trim() }
        .filter { it.isNotEmpty() }

android {
    namespace = "com.picoo.camera"
    compileSdk = 34
    // NDK r28+ ships 16 KB-aligned libc++ and honors flexible page sizes (Xiaomi 15 / Android 15).
    ndkVersion = "28.0.12674087"

    defaultConfig {
        applicationId = "com.picoo.camera"
        minSdk = 29
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += rustAbis
        }

        externalNativeBuild {
            cmake {
                cppFlags += listOf("-std=c++17", "-Wall")
                // Static STL avoids shipping a 4 KB-aligned libc++_shared from older NDKs;
                // picoo_jni is the only consumer.
                arguments +=
                    listOf(
                        "-DANDROID_STL=c++_static",
                        "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON",
                    )
            }
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

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
        }
    }

    packaging {
        jniLibs {
            // Extract .so so 16 KB-page devices can mmap with correct ELF p_align.
            useLegacyPackaging = true
            // cargo-ndk may drop intermediate libquiche-*.so into jniLibs; do not ship them.
            excludes += listOf("**/libquiche-*.so")
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
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")

    val cameraXVersion = "1.4.2"
    implementation("androidx.camera:camera-camera2:$cameraXVersion")
    implementation("androidx.camera:camera-lifecycle:$cameraXVersion")
    implementation("androidx.camera:camera-view:$cameraXVersion")
    implementation("com.google.mlkit:barcode-scanning:17.3.0")

    testImplementation("junit:junit:4.13.2")
}

val rustPageAlignFlags = "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-soname,libpicoo_ffi.so"
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")

tasks.register<Exec>("cargoBuildFfi") {
    group = "build"
    description = "Build picoo-ffi Rust cdylib for configured Android ABIs"
    workingDir = workspaceRoot
    val ndkHome = System.getenv("ANDROID_NDK_HOME") ?: android.ndkDirectory.absolutePath
    doFirst {
        jniLibsDir.asFile.mkdirs()
        // Drop stale hashed quiche intermediates from prior cargo-ndk runs.
        jniLibsDir.asFile.walkTopDown()
            .filter { it.isFile && it.name.startsWith("libquiche-") && it.extension == "so" }
            .forEach { it.delete() }
    }
    val cargoNdkArgs = mutableListOf("cargo", "ndk")
    rustAbis.forEach { abi ->
        cargoNdkArgs += listOf("-t", abi)
    }
    cargoNdkArgs +=
        listOf(
            "-o",
            jniLibsDir.asFile.absolutePath,
            "build",
            "--release",
            "-p",
            "picoo-ffi",
        )
    commandLine(cargoNdkArgs)
    environment("ANDROID_NDK_HOME", ndkHome)
    // Ensure Rust cdylib LOAD segments are 16 KB aligned and expose a stable SONAME
    // so libpicoo_jni DT_NEEDED is "libpicoo_ffi.so" (not a host absolute path).
    environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS", rustPageAlignFlags)
    environment("CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS", rustPageAlignFlags)
    doLast {
        jniLibsDir.asFile.walkTopDown()
            .filter { it.isFile && it.name.startsWith("libquiche-") && it.extension == "so" }
            .forEach { it.delete() }
    }
}

tasks.named("preBuild") {
    dependsOn("cargoBuildFfi")
}
