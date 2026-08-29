fn main() {
    let proto = "../../proto/picoo_camera.proto";
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is unavailable");
    std::env::set_var("PROTOC", protoc);
    println!("cargo:rerun-if-changed={proto}");
    println!("cargo:rerun-if-changed=../../proto");
    prost_build::Config::new()
        .compile_protos(&[proto], &["../../proto"])
        .expect("failed to compile protos");
}
