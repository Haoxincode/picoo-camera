fn main() {
    prost_build::Config::new()
        .compile_protos(&["../../proto/picoo_camera.proto"], &["../../proto"])
        .expect("failed to compile protos");
}
