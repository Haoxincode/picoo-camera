fn main() {
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
