package com.picoo.camera.jni

/**
 * JNI bridge to Rust Core C ABI (REQ-PICOO-STACK-003).
 *
 * Kotlin → JNI (libpicoo_jni.so) → C ABI (libpicoo_ffi.so) → Rust
 */
object PicooNative {
    init {
        System.loadLibrary("picoo_ffi")
        System.loadLibrary("picoo_jni")
    }

    /** Returns PCP/1 protocol version from Rust Core. */
    external fun getProtocolVersion(): String
}
