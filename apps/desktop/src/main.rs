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

    let state = DesktopAppState::default();
    println!("Picoo Camera Desktop (stub) — status: {:?}", state.receiver_status);
    println!("Run on windows-latest for GPUI + MF + Virtual Camera build.");
}
