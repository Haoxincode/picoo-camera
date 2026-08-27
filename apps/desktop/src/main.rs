//! Desktop Receiver application shell — REQ-PICOO-UI-001.
//!
//! GPUI integration will be added in the Windows vertical slice step.

mod model;

use model::DesktopAppState;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    if std::env::args().any(|arg| arg == "--loopback-demo") {
        run_loopback_demo();
        return;
    }

    let state = DesktopAppState::default();
    println!(
        "Picoo Camera Desktop (stub) — status: {:?}",
        state.receiver_status
    );
    println!("Run with --loopback-demo to exercise QUIC → FrameHub on Linux CI.");
    println!("Run on windows-latest for GPUI + MF + Virtual Camera build.");
}

fn run_loopback_demo() {
    match picoo_receiver::run_loopback_access_unit(b"desktop-loopback-au") {
        Ok(frame) => {
            println!(
                "Loopback OK — FrameHub received {} bytes, status=Streaming path verified",
                frame.len()
            );
        }
        Err(err) => {
            eprintln!("Loopback demo failed: {err}");
            std::process::exit(1);
        }
    }
}
