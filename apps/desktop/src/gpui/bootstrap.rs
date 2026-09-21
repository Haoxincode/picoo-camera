use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::component::*;
use gpui_kit::*;
use picoo_receiver::ReceiverError;

use crate::prefs::load_prefs;

use super::receiver_startup::ReceiverStartupView;

const DEVICE_FRAME_ASSETS: [(&str, &[u8]); 4] = [
    (
        "device-frames/generic-phone.svg",
        include_bytes!("../../../../assets/device-frames/generic-phone.svg"),
    ),
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

// REQ-PICOO-UI-0001 / AC-D-LAYOUT-01 / AC-D-LIVE-01: the product window is
// intentionally large enough to keep the complete Live toolbar visible.
const PRODUCT_WINDOW_SIZE: gpui_kit::Size<Pixels> = size(px(1440.), px(900.));

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
    // REQ-PICOO-UI-007: startup registration is changed only by the explicit
    // settings toggle. Do not re-enable a login item that the user disabled in
    // Windows/macOS system settings while the persisted preference is `true`.
    // GPUI's Windows platform calls OleInitialize (STA). It must own the UI
    // thread apartment before ReceiverRuntime creates the Media Foundation
    // decoder; otherwise an earlier MTA init makes platform construction panic.
    // Open the product window before starting the Receiver. Receiver startup
    // is dispatched by ReceiverStartupView so platform/network initialization
    // cannot block the GPUI message pump.
    #[cfg(all(windows, feature = "gpui-ui"))]
    crate::windows_startup::append_log("run_gpui_app: constructing GPUI application");
    let app = gpui_kit::application().with_assets(PicooAssets);
    #[cfg(all(windows, feature = "gpui-ui"))]
    crate::windows_startup::append_log("run_gpui_app: entering app.run before vcam probe");

    let prefs_for_window = prefs.clone();
    app.run(move |cx| {
        gpui_kit::init(cx);
        crate::picoo_theme::install(cx);
        cx.set_app_identity("com.picoo.camera", "Picoo Camera");
        cx.activate(true);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(100.), px(100.)),
                    size: PRODUCT_WINDOW_SIZE,
                })),
                window_min_size: Some(PRODUCT_WINDOW_SIZE),
                titlebar: Some(TitlebarOptions {
                    title: Some("Picoo Camera".into()),
                    ..TitleBar::title_bar_options()
                }),
                show: true,
                focus: true,
                ..TitleBar::window_options()
            },
            move |window, cx| {
                let window_handle = window.window_handle();
                window.set_window_title("Picoo Camera");
                window.activate_window();
                #[cfg(all(windows, feature = "windows-vcam"))]
                {
                    crate::tray::force_show_product_window();
                    crate::tray::ensure_tray_icon(&crate::tray::tip_for_status(
                        picoo_session::ReceiverStatus::Discovering,
                    ));
                    crate::windows_startup::append_log(
                        "run_gpui_app: product HWND created; receiver startup dispatched",
                    );
                }
                let startup_view =
                    cx.new(|_| ReceiverStartupView::new(prefs_for_window, window_handle));
                startup_view.update(cx, |this, cx| this.start(cx));
                // REQ-PICOO-UI-008: the close policy is available before the
                // Receiver is ready, so a slow startup still has a usable
                // tray/quit path.
                let tray_view = startup_view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let outcome = tray_view.read(cx).close_outcome(cx);
                    if outcome.hide_to_background {
                        #[cfg(all(windows, feature = "windows-vcam"))]
                        {
                            let status = tray_view.read(cx).tray_status(cx);
                            let tip = crate::tray::tip_for_status(status);
                            crate::tray::note_hidden_to_tray_with_tip(&tip);
                        }
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
                cx.new(|cx| Root::new(startup_view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PicooAssets, PRODUCT_WINDOW_SIZE};
    use gpui_kit::{px, AssetSource};

    #[test]
    fn product_window_preserves_the_full_live_toolbar() {
        assert_eq!(PRODUCT_WINDOW_SIZE.width, px(1440.));
        assert_eq!(PRODUCT_WINDOW_SIZE.height, px(900.));
    }

    #[test]
    fn gpui_platform_initializes_before_receiver_runtime() {
        let source = include_str!("bootstrap.rs");
        let start = source
            .find("pub fn run_gpui_app()")
            .expect("run_gpui_app source");
        let end = source[start..]
            .find("#[cfg(test)]")
            .map(|offset| start + offset)
            .unwrap_or(source.len());
        let body = &source[start..end];
        let platform = body
            .find("let app = gpui_kit::application()")
            .expect("GPUI platform initialization");
        let receiver = body
            .find("ReceiverStartupView::new")
            .expect("receiver startup view");
        assert!(
            platform < receiver,
            "Windows OLE/STA must be initialized before Media Foundation"
        );
        let open_window = body.find("cx.open_window(").expect("open_window");
        assert!(
            open_window < receiver,
            "product HWND must exist before Receiver startup is dispatched"
        );
        assert!(
            body.contains("show: true"),
            "Windows CreateWindowExW must request a visible product window"
        );
        assert!(
            !body.contains("start_from_prefs"),
            "Receiver startup factory must not block the open_window callback"
        );
    }

    #[test]
    fn device_frame_svgs_are_embedded_with_intrinsic_dimensions() {
        for path in [
            "device-frames/generic-phone.svg",
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
