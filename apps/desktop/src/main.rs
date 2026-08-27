//! Picoo Camera desktop Receiver — ARCH-PICOO-UI-001 shell.

mod diagnostics_export;
mod model;
mod prefs;
mod qr_display;
mod receiver_runtime;

#[cfg(feature = "gpui-ui")]
mod gpui_app;
#[cfg(all(windows, feature = "windows-vcam"))]
mod vcam_register;
#[cfg(feature = "gpui-ui")]
mod vcam_status;
#[cfg(feature = "gpui-ui")]
mod video_surface;

use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;

use diagnostics_export::export_diagnostics_json;
use model::DesktopAppState;
use picoo_pairing::TrustedDeviceStore;
use picoo_receiver::ReceiverSession;
use receiver_runtime::{default_trusted_store_path, ReceiverRuntime, ReceiverRuntimeConfig};
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

    if args.iter().any(|arg| arg == "--gpui") {
        #[cfg(feature = "gpui-ui")]
        {
            if let Err(err) = gpui_app::run_gpui_app() {
                eprintln!("GPUI app failed: {err}");
                std::process::exit(1);
            }
            return;
        }
        #[cfg(not(feature = "gpui-ui"))]
        {
            eprintln!("Rebuild with --features gpui-ui to launch the desktop UI.");
            std::process::exit(1);
        }
    }

    if let Some(index) = args.iter().position(|arg| arg == "--export-diagnostics") {
        let out_path = args.get(index + 1).map(String::as_str);
        run_export_diagnostics(out_path);
        return;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if args.iter().any(|arg| arg == "--register-vcam") {
        run_register_vcam();
        return;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if args.iter().any(|arg| arg == "--unregister-vcam") {
        run_unregister_vcam();
        return;
    }

    #[cfg(all(feature = "gpui-ui", any(target_os = "windows", target_os = "macos")))]
    if args.len() <= 1 {
        if let Err(err) = gpui_app::run_gpui_app() {
            eprintln!("GPUI app failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let state = DesktopAppState::default();
    println!(
        "Picoo Camera Desktop (stub) — status: {:?}",
        state.receiver_status
    );
    println!("Run with --loopback-demo to exercise QUIC → FrameHub on Linux CI.");
    println!("Run with --serve to listen, advertise mDNS, and print QR JSON.");
    println!("Run with --gpui for the GPUI desktop shell (requires gpui-ui feature).");
    println!("Run with --list-paired / --remove-paired <id> to manage trusted devices.");
    println!("Run with --export-diagnostics [path] to export redacted diagnostics JSON.");
    println!("Run on windows-latest for GPUI + MF + Virtual Camera build.");
    #[cfg(all(windows, feature = "windows-vcam"))]
    println!("Run with --register-vcam / --unregister-vcam on Windows 11 for MF virtual camera.");
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn run_register_vcam() {
    match vcam_register::VirtualCameraRegistration::register_and_start() {
        Ok(registration) => {
            println!("Picoo Camera virtual camera registered and started.");
            println!("Press Enter to remove the virtual camera and exit.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if let Err(err) = registration.remove() {
                eprintln!("Failed to remove virtual camera: {err}");
                std::process::exit(1);
            }
            println!("Virtual camera removed.");
        }
        Err(err) => {
            eprintln!("Virtual camera registration failed: {err}");
            eprintln!("Ensure PicooVirtualCameraSource.dll is registered (regsvr32) and reachable by Frame Server.");
            std::process::exit(1);
        }
    }
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn run_unregister_vcam() {
    match vcam_register::VirtualCameraRegistration::register_and_start() {
        Ok(registration) => match registration.remove() {
            Ok(()) => println!("Virtual camera removed."),
            Err(err) => {
                eprintln!("Failed to remove virtual camera: {err}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("Could not attach to virtual camera for removal: {err}");
            std::process::exit(1);
        }
    }
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
                    picoo_diagnostics::redact_device_name(&device.device_name),
                    picoo_diagnostics::redact_fingerprint(&device.certificate_fingerprint),
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
    let json = match export_diagnostics_json(
        picoo_session::ReceiverStatus::Disconnected,
        Default::default(),
    ) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("{err}");
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
    match picoo_receiver::run_paired_loopback_access_unit(b"desktop-loopback-au") {
        Ok(frame) => {
            println!(
                "Paired loopback OK — FrameHub received {} bytes (pairing path, no unpaired bypass)",
                frame.len()
            );
        }
        Err(err) => {
            eprintln!("Paired loopback demo failed: {err}");
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
                    device.device_id,
                    picoo_diagnostics::redact_device_name(&device.device_name),
                    picoo_diagnostics::redact_fingerprint(&device.certificate_fingerprint)
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
            match export_diagnostics_json(receiver.status(), stats) {
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
    let config = ReceiverRuntimeConfig::default();
    let trusted_path = config.trusted_store_path.clone();
    let mut runtime = match ReceiverRuntime::start(config) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("Failed to start receiver: {err}");
            std::process::exit(1);
        }
    };

    if let Some(json) = &runtime.snapshot().qr_json {
        println!("QR payload: {json}");
        if let Some(art) = &runtime.snapshot().qr_ascii {
            println!("Scan QR Code (PUC-003):\n{art}");
        }
    }

    if let Some(bind) = runtime.snapshot().bind_addr {
        println!(
            "Listening on {bind} — status {:?}. Trusted store: {}",
            runtime.snapshot().status,
            trusted_path.display()
        );
    }
    println!("Type `confirm` when pairing code matches, `list`, `remove <device_id>`, or `export-diagnostics`.");

    let stdin_rx = spawn_stdin_commands();
    let mut last_pairing_hint = String::new();

    loop {
        while let Ok(line) = stdin_rx.try_recv() {
            handle_console_command(runtime.receiver_mut(), &line);
        }

        if let Err(err) = runtime.pump() {
            eprintln!("Receiver pump error: {err}");
        }
        if let Some(code) = runtime.receiver().pairing_short_code() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_trusted_store_path_is_non_empty() {
        let path = default_trusted_store_path();
        assert!(!path.as_os_str().is_empty());
    }
}
