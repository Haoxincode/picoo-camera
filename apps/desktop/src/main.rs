//! Picoo Camera desktop Receiver — ARCH-PICOO-UI-001 shell.
//!
//! GPUI integration will be added in the Windows vertical slice step.

mod model;

use model::DesktopAppState;
use picoo_discovery::{
    generate_nonce, MdnsAdvertiser, QrConnectPayload, ReceiverAdvertisement, DEFAULT_QR_TTL_MS,
};
use picoo_receiver::{ReceiverIdentity, ReceiverSession};
use picoo_transport::Endpoint;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    if std::env::args().any(|arg| arg == "--loopback-demo") {
        run_loopback_demo();
        return;
    }

    if std::env::args().any(|arg| arg == "--serve") {
        run_serve_mode();
        return;
    }

    let state = DesktopAppState::default();
    println!(
        "Picoo Camera Desktop (stub) — status: {:?}",
        state.receiver_status
    );
    println!("Run with --loopback-demo to exercise QUIC → FrameHub on Linux CI.");
    println!("Run with --serve to listen, advertise mDNS, and print QR JSON.");
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

fn run_serve_mode() {
    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());

    if let Err(err) = receiver.attach_shared_ring("picoo-camera-v1") {
        eprintln!("Shared Frame Ring unavailable: {err}");
    }

    let bind = match receiver.listen(Endpoint {
        host: "0.0.0.0".into(),
        port: 0,
    }) {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("Failed to bind QUIC listener: {err}");
            std::process::exit(1);
        }
    };

    let mut mdns = match MdnsAdvertiser::new() {
        Ok(advertiser) => Some(advertiser),
        Err(err) => {
            eprintln!("mDNS unavailable: {err}");
            None
        }
    };

    let advertisement = ReceiverAdvertisement::new(
        identity.receiver_id.clone(),
        identity.display_name.clone(),
        bind.port(),
        "00000000",
    );

    if let Some(advertiser) = mdns.as_mut() {
        if let Err(err) = advertiser.register("127.0.0.1", &advertisement) {
            eprintln!("mDNS register failed: {err}");
        } else {
            println!(
                "mDNS advertising {} on port {}",
                advertisement.display_name,
                bind.port()
            );
        }
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let qr = QrConnectPayload::new(
        bind.ip().to_string(),
        bind.port(),
        identity.receiver_id,
        "00000000",
        generate_nonce(),
        now_ms,
        DEFAULT_QR_TTL_MS,
    );
    match qr.encode_json() {
        Ok(json) => println!("QR payload: {json}"),
        Err(err) => eprintln!("QR encode failed: {err}"),
    }

    println!(
        "Listening on {} — status {:?}. Ctrl+C to exit.",
        bind,
        receiver.status()
    );

    loop {
        if let Err(err) = receiver.pump() {
            eprintln!("Receiver pump error: {err}");
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
