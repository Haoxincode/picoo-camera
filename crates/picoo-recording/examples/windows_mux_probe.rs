//! Synthetic platform mux research — REQ-PICOO-NEXT-018/022.
//! This is not a production recorder and does not initialize a camera.
#[cfg(windows)]
#[path = "windows_mux/probe.rs"]
mod probe;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows MF mux probe requires a native Windows host");
    std::process::exit(1);
}
