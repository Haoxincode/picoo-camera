//! Picoo Camera desktop Receiver — ARCH-PICOO-UI-001 shell.
//!
//! GPUI integration will be added in the Windows vertical slice step.

mod model;

use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use model::DesktopAppState;
use picoo_diagnostics::{build_report, export_json, DiagnosticInput, DiagnosticSessionSnapshot};
use picoo_discovery::{
    generate_nonce, MdnsAdvertiser, QrConnectPayload, ReceiverAdvertisement, DEFAULT_QR_TTL_MS,
};
use picoo_pairing::TrustedDeviceStore;
use picoo_receiver::{ReceiverIdentity, ReceiverSession};
use picoo_transport::Endpoint;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|arg| arg == "--loopback-demo") {
        run_loopback_demo();
        return;
    }

    if args.iter().any(|arg| arg == "--list-paired") {
        run_list_paired();
        return;
    }

    if let Some(index) = args.iter().position(|arg| arg == "--remove-paired") {
        let device_id = args.get(index + 1).map(String::as_str).unwrap_or("");
        if device_id.is_empty() {
            eprintln!("Usage: picoo-desktop --remove-paired <device_id>");
            std::process::exit(1);
        }
        run_remove_paired(device_id);
        return;
    }

    if args.iter().any(|arg| arg == "--serve") {
        run_serve_mode();
        return;
    }

    if let Some(index) = args.iter().position(|arg| arg == "--export-diagnostics") {
        let out_path = args.get(index + 1).map(String::as_str);
        run_export_diagnostics(out_path);
        return;
    }

    let state = DesktopAppState::default();
    println!(
        "Picoo Camera Desktop (stub) — status: {:?}",
        state.receiver_status
    );
    println!("Run with --loopback-demo to exercise QUIC → FrameHub on Linux CI.");
    println!("Run with --serve to listen, advertise mDNS, and print QR JSON.");
    println!("Run with --list-paired / --remove-paired <id> to manage trusted devices.");
    println!("Run with --export-diagnostics [path] to export redacted diagnostics JSON.");
    println!("Run on windows-latest for GPUI + MF + Virtual Camera build.");
}

fn default_trusted_store_path() -> PathBuf {
    std::env::var("PICOO_TRUSTED_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".config")
                .join("picoo-camera")
                .join("trusted_devices.json")
        })
}

fn run_list_paired() {
    let path = default_trusted_store_path();
    match TrustedDeviceStore::load_from_path(&path) {
        Ok(store) => {
            let mut listed = false;
            for device in store.list() {
                listed = true;
                println!(
                    "{} | {} | fp={} | last={:?}",
                    device.device_id,
                    device.device_name,
                    device.certificate_fingerprint,
                    device.last_connected_at_ms
                );
            }
            if !listed {
                println!("No paired devices ({})", path.display());
            }
        }
        Err(err) => {
            eprintln!("Failed to load trusted store {}: {err}", path.display());
            std::process::exit(1);
        }
    }
}

fn run_remove_paired(device_id: &str) {
    let path = default_trusted_store_path();
    let mut store = match TrustedDeviceStore::load_from_path(&path) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("Failed to load trusted store: {err}");
            std::process::exit(1);
        }
    };
    if store.remove(device_id) {
        if let Err(err) = store.save_to_path(&path) {
            eprintln!("Failed to save trusted store: {err}");
            std::process::exit(1);
        }
        println!("Removed paired device {device_id}");
    } else {
        eprintln!("Device not found: {device_id}");
        std::process::exit(1);
    }
}

fn run_export_diagnostics(out_path: Option<&str>) {
    let trusted_path = default_trusted_store_path();
    let store = match TrustedDeviceStore::load_from_path(&trusted_path) {
        Ok(store) => store,
        Err(err) => {
            eprintln!(
                "Failed to load trusted store {}: {err}",
                trusted_path.display()
            );
            std::process::exit(1);
        }
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let report = build_report(DiagnosticInput {
        platform: std::env::consts::OS.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        exported_at_ms: now_ms,
        trusted_devices: store.list().cloned().collect(),
        ..Default::default()
    });

    let json = match export_json(&report) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("Failed to serialize diagnostics: {err}");
            std::process::exit(1);
        }
    };

    if let Some(path) = out_path {
        if let Err(err) = std::fs::write(path, &json) {
            eprintln!("Failed to write diagnostics to {path}: {err}");
            std::process::exit(1);
        }
        println!("Diagnostics exported to {path} (redacted, no video)");
    } else {
        println!("{json}");
    }
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

fn spawn_stdin_commands() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

fn handle_console_command(receiver: &mut ReceiverSession, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    match line {
        "confirm" | "confirm-pairing" => {
            receiver.confirm_pairing_locally();
            println!("Desktop confirmed pairing locally.");
        }
        "list" | "list-paired" => {
            for device in receiver.trusted_devices().list() {
                println!(
                    "{} | {} | fp={}",
                    device.device_id, device.device_name, device.certificate_fingerprint
                );
            }
        }
        cmd if cmd.starts_with("remove ") => {
            let device_id = cmd.trim_start_matches("remove ").trim();
            match receiver.remove_trusted_device(device_id) {
                Ok(true) => println!("Removed paired device {device_id}"),
                Ok(false) => println!("Device not found: {device_id}"),
                Err(err) => eprintln!("Remove failed: {err}"),
            }
        }
        "export-diagnostics" | "export" => {
            let stats = receiver.ingress_stats();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let report = build_report(DiagnosticInput {
                platform: std::env::consts::OS.into(),
                app_version: env!("CARGO_PKG_VERSION").into(),
                exported_at_ms: now_ms,
                session: Some(DiagnosticSessionSnapshot {
                    role: "receiver".into(),
                    status: format!("{:?}", receiver.status()),
                    ingress_access_units: stats.access_units,
                    ingress_packets_received: stats.packets_received,
                    ingress_packets_dropped_unpaired: stats.packets_dropped_unpaired,
                }),
                trusted_devices: receiver.trusted_devices().list().cloned().collect(),
                ..Default::default()
            });
            match export_json(&report) {
                Ok(json) => println!("{json}"),
                Err(err) => eprintln!("Export failed: {err}"),
            }
        }
        "help" => {
            println!("Commands: confirm | list | remove <device_id> | export-diagnostics | help");
        }
        other => println!("Unknown command: {other} (type help)"),
    }
}

fn run_serve_mode() {
    let identity = ReceiverIdentity::default();
    let trusted_path = default_trusted_store_path();
    let mut receiver = match ReceiverSession::new()
        .with_identity(identity.clone())
        .with_trusted_store(&trusted_path)
    {
        Ok(session) => session,
        Err(err) => {
            eprintln!(
                "Failed to load trusted store {}: {err}",
                trusted_path.display()
            );
            std::process::exit(1);
        }
    };

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
        "Listening on {} — status {:?}. Trusted store: {}",
        bind,
        receiver.status(),
        trusted_path.display()
    );
    println!("Type `confirm` when pairing code matches, `list`, `remove <device_id>`, or `export-diagnostics`.");

    let stdin_rx = spawn_stdin_commands();
    let mut last_pairing_hint = String::new();

    loop {
        while let Ok(line) = stdin_rx.try_recv() {
            handle_console_command(&mut receiver, &line);
        }

        if let Err(err) = receiver.pump() {
            eprintln!("Receiver pump error: {err}");
        }
        if let Some(code) = receiver.pairing_short_code() {
            let hint = format!("Pairing code: {code} — type `confirm` on desktop to approve");
            if hint != last_pairing_hint {
                println!("{hint}");
                last_pairing_hint = hint;
            }
        } else {
            last_pairing_hint.clear();
        }
        thread::sleep(std::time::Duration::from_millis(16));
    }
}
