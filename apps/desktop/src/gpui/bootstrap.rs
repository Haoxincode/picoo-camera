use std::borrow::Cow;

use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use picoo_receiver::ReceiverError;

use crate::model::VirtualCameraStatus;
use crate::prefs::load_prefs;
use crate::receiver_runtime::ReceiverRuntime;
use crate::vcam_status::detect_vcam_status;

use super::PicooDesktopApp;

const DEVICE_FRAME_ASSETS: [(&str, &[u8]); 3] = [
    (
        "device-frames/iphone-16-max.svg",
        include_bytes!("../../../../assets/device-frames/iphone-16-max.svg"),
    ),
    (
        "device-frames/macbook-pro-light.svg",
        include_bytes!("../../../../assets/device-frames/macbook-pro-light.svg"),
    ),
    (
        "device-frames/macbook-pro-dark.svg",
        include_bytes!("../../../../assets/device-frames/macbook-pro-dark.svg"),
    ),
];

struct PicooAssets;

impl AssetSource for PicooAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = DEVICE_FRAME_ASSETS
            .iter()
            .find(|(asset_path, _)| *asset_path == path)
        {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = Assets.list(path)?;
        assets.extend(
            DEVICE_FRAME_ASSETS
                .iter()
                .filter(|(asset_path, _)| asset_path.starts_with(path))
                .map(|(asset_path, _)| SharedString::from(*asset_path)),
        );
        Ok(assets)
    }
}

pub fn run_gpui_app() -> Result<(), ReceiverError> {
    let prefs = load_prefs();
    // Ensure subscriber exists even if main skipped prefs-aware init paths.
    crate::logging::init_logging(prefs.log_level.env_filter());
    std::env::set_var("RUST_LOG", prefs.log_level.env_filter());
    let _ = crate::logging::reload_filter(prefs.log_level.env_filter());
    // REQ-PICOO-UI-007: apply persisted startup preference at launch.
    if let Err(err) = crate::startup::sync_launch_at_startup(prefs.launch_at_startup) {
        tracing::warn!("startup sync on launch: {err}");
    }
    // GPUI's Windows platform calls OleInitialize (STA). It must own the UI
    // thread apartment before ReceiverRuntime creates the Media Foundation
    // decoder; otherwise an earlier MTA init makes platform construction panic.
    let app = gpui_platform::application().with_assets(PicooAssets);
    let vcam_status = detect_vcam_status();
    let runtime = ReceiverRuntime::from_prefs(&prefs)?;
    let mut runtime = runtime;
    runtime.set_virtual_camera_status(vcam_status);

    // REQ-PICOO-VCAM-002: keep Session-lifetime MF virtual camera for the desktop process.
    #[cfg(all(windows, feature = "windows-vcam"))]
    let (vcam_status, _vcam_registration) = if should_auto_start_vcam(vcam_status) {
        match crate::vcam_register::start_registered_on_worker() {
            Ok(reg) => {
                tracing::info!("Picoo Camera virtual camera started for this session");
                runtime.set_virtual_camera_status(VirtualCameraStatus::Active);
                (VirtualCameraStatus::Active, Some(reg))
            }
            Err(err) => {
                tracing::warn!(
                    "MF virtual camera start deferred: {err} (open 虚拟摄像头 → 安装或修复…)"
                );
                (vcam_status, None)
            }
        }
    } else {
        tracing::info!(
            "virtual camera is not installed; skip privileged startup repair and keep the explicit repair action available"
        );
        (vcam_status, None)
    };

    let prefs_for_window = prefs.clone();
    app.run(move |cx| {
        gpui_component::init(cx);
        crate::picoo_theme::install(cx);
        cx.set_app_identity("com.picoo.camera", "Picoo Camera");
        cx.activate(true);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(100.), px(100.)),
                    size: size(px(1920.), px(1080.)),
                })),
                window_min_size: Some(size(px(1180.), px(720.))),
                ..TitleBar::window_options()
            },
            move |window, cx| {
                let window_handle = window.window_handle();
                let view = cx.new(|cx| {
                    PicooDesktopApp::new(
                        runtime,
                        prefs_for_window,
                        vcam_status,
                        window_handle,
                        window,
                        cx,
                    )
                });
                // Start frame/tray pump after the view exists — not inside Render.
                view.update(cx, |this, cx| {
                    this.ensure_pump_loop(cx);
                    #[cfg(target_os = "macos")]
                    this.refresh_vcam_status(cx);
                });
                // REQ-PICOO-UI-008: Windows closes to tray when enabled; macOS
                // keeps the app in Dock/background without a fake tray icon.
                let tray_view = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let outcome = tray_view.read(cx).close_outcome();
                    if outcome.hide_to_background {
                        #[cfg(all(windows, feature = "windows-vcam"))]
                        {
                            let status = tray_view.read(cx).runtime.snapshot().status;
                            let tip = crate::tray::tip_for_status(status);
                            crate::tray::note_hidden_to_tray_with_tip(&tip);
                        }
                        // App-level hide keeps the process; minimize covers hosts
                        // where hide() is a no-op.
                        cx.hide();
                        window.minimize_window();
                        return false;
                    }
                    #[cfg(all(windows, feature = "windows-vcam"))]
                    crate::tray::note_tray_cleared();
                    outcome.allow_close
                });
                #[cfg(all(windows, feature = "windows-vcam"))]
                crate::tray::pump_win32_tray_messages();
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}

#[cfg(any(test, all(windows, feature = "windows-vcam")))]
fn should_auto_start_vcam(status: VirtualCameraStatus) -> bool {
    matches!(
        status,
        VirtualCameraStatus::Installed | VirtualCameraStatus::Active
    )
}


#[cfg(test)]
mod tests {
    use super::{should_auto_start_vcam, PicooAssets};
    use crate::model::VirtualCameraStatus;
    use gpui::AssetSource;

    #[test]
    fn gpui_platform_initializes_before_receiver_runtime() {
        let source = include_str!("bootstrap.rs");
        let start = source
            .find("pub fn run_gpui_app()")
            .expect("run_gpui_app source");
        let body = &source[start..];
        let platform = body
            .find("let app = gpui_platform::application()")
            .expect("GPUI platform initialization");
        let receiver = body
            .find("ReceiverRuntime::from_prefs")
            .expect("receiver runtime initialization");
        assert!(
            platform < receiver,
            "Windows OLE/STA must be initialized before Media Foundation"
        );
    }

    #[test]
    fn only_an_installed_virtual_camera_is_started_automatically() {
        assert!(should_auto_start_vcam(VirtualCameraStatus::Installed));
        assert!(should_auto_start_vcam(VirtualCameraStatus::Active));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::NotInstalled));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::Bundled));
        assert!(!should_auto_start_vcam(
            VirtualCameraStatus::AwaitingApproval
        ));
        assert!(!should_auto_start_vcam(
            VirtualCameraStatus::RestartRequired
        ));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::Uninstalling));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::Unknown));
    }

    #[test]
    fn hardware_topology_svgs_are_embedded_with_intrinsic_dimensions() {
        for path in [
            "device-frames/iphone-16-max.svg",
            "device-frames/macbook-pro-light.svg",
            "device-frames/macbook-pro-dark.svg",
        ] {
            let asset = PicooAssets
                .load(path)
                .expect("device asset lookup should succeed")
                .expect("device asset should be embedded");
            let svg = std::str::from_utf8(&asset).expect("device asset should be UTF-8 SVG");
            assert!(svg.contains("<svg width=\""));
            assert!(svg.contains(" height=\""));
            assert!(svg.contains(" viewBox=\""));
        }

        let iphone = PicooAssets
            .load("device-frames/iphone-16-max.svg")
            .expect("iPhone asset lookup should succeed")
            .expect("iPhone asset should be embedded");
        let iphone = std::str::from_utf8(&iphone).expect("iPhone asset should be UTF-8 SVG");
        assert!(iphone.contains("<linearGradient"));
        assert!(iphone.contains("<clipPath"));
        assert!(iphone.contains("<feGaussianBlur"));
        assert!(!iphone.contains("<lineargradient"));
        assert!(!iphone.contains("<clippath"));
        assert!(iphone.contains("M415 343"), "right-side control is missing");
        assert!(iphone.contains("M0 151"), "left-side controls are missing");
    }

}
