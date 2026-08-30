// REQ-PICOO-UI-013: embed the full-color app icon and the compact tray symbol.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/windows/PicooCamera.ico");
    println!("cargo:rerun-if-changed=../../assets/brand/windows/PicooCameraTray.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // ARCH-PICOO-STACK-001 builds the distributable Windows product only on a
    // Windows runner. Allow non-linking cross-target `cargo check` on macOS/Linux
    // without requiring rc.exe; native Windows builds must compile the resources.
    if !std::env::var("HOST").is_ok_and(|host| host.contains("windows")) {
        println!("cargo:warning=skipping Windows resources during non-Windows cross-check");
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../assets/brand/windows/PicooCamera.ico")
        .set_icon_with_id("../../assets/brand/windows/PicooCameraTray.ico", "2")
        .set("ProductName", "Picoo Camera")
        .set("FileDescription", "Picoo Camera Desktop Receiver")
        .set("OriginalFilename", "picoo-desktop.exe");
    resource
        .compile()
        .expect("compile Picoo Camera Windows icon resources");
}
