fn main() {
    let proto = "../../proto/picoo_camera.proto";
    println!("cargo:rerun-if-changed={proto}");
    println!("cargo:rerun-if-changed=../../proto");
    prost_build::Config::new()
        .compile_protos(&[proto], &["../../proto"])
        .expect("failed to compile protos");
}
