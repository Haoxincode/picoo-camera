#[path = "../../../build-support/windows_resource.rs"]
mod windows_resource;

fn main() {
    println!("cargo:rerun-if-changed=../../../build-support/windows_resource.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::env::var("HOST").is_ok_and(|host| host.contains("windows")) {
        println!("cargo:warning=skipping Windows resources during non-Windows cross-check");
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    windows_resource::apply_package_version(&mut resource, 1);
    resource
        .set("ProductName", "Picoo Camera")
        .set("FileDescription", "Picoo Camera Shared Frame Ring Reader")
        .set("OriginalFilename", "picoo-vcam-ring-reader.exe")
        .compile()
        .expect("compile Picoo ring-reader resources");
}
