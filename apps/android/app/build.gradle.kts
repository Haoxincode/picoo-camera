plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val workspaceRoot = rootProject.projectDir.parentFile.parentFile

android {
    namespace = "com.picoo.camera"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.picoo.camera"
        minSdk = 29
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }

        externalNativeBuild {
            cmake {
                cppFlags += listOf("-std=c++17", "-Wall")
                arguments += listOf("-DANDROID_STL=c++_shared")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
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
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    debugImplementation("androidx.compose.ui:ui-tooling")

    val cameraXVersion = "1.3.4"
    implementation("androidx.camera:camera-camera2:$cameraXVersion")
    implementation("androidx.camera:camera-lifecycle:$cameraXVersion")
    implementation("androidx.camera:camera-view:$cameraXVersion")
    implementation("com.google.mlkit:barcode-scanning:17.3.0")
}

val rustAbi = "arm64-v8a"
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")

tasks.register<Exec>("cargoBuildFfi") {
    group = "build"
    description = "Build picoo-ffi Rust cdylib for Android arm64"
    workingDir = workspaceRoot
    val ndkHome = System.getenv("ANDROID_NDK_HOME") ?: android.ndkDirectory.absolutePath
    doFirst {
        jniLibsDir.asFile.mkdirs()
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
    environment("ANDROID_NDK_HOME", ndkHome)
}

tasks.named("preBuild") {
    dependsOn("cargoBuildFfi")
}
