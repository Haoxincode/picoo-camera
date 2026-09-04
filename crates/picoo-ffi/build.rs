fn main() {
    // `picoo-transport` calls Android's public multinetwork NDK API through `ndk-sys`.
    // Keep libandroid on the final cdylib link line: native-library metadata from a Rust
    // dependency can otherwise be dropped by the Android linker under `--as-needed`, leaving
    // an unresolved `android_setsocknetwork` that fails only when System.loadLibrary runs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,--no-as-needed");
        println!("cargo:rustc-link-arg=-landroid");
        println!("cargo:rustc-link-arg=-Wl,--as-needed");
    }

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config {
        language: cbindgen::Language::C,
        ..Default::default()
    };

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed")
        .write_to_file("picoo_camera.h");

    // Ensure C++ consumers can include the generated header.
    let header_path = std::path::Path::new(&crate_dir).join("picoo_camera.h");
    let header = std::fs::read_to_string(&header_path).expect("read header");
    if !header.contains("extern \"C\"") {
        let wrapped = format!(
            "#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n{header}\n#ifdef __cplusplus\n}}\n#endif\n"
        );
        std::fs::write(header_path, wrapped).expect("write wrapped header");
    }
}
