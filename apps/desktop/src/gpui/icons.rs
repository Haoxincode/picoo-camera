use gpui_kit::component::*;
use gpui_kit::*;

/// Product semantic icon names used by the desktop UI.
///
/// Keeping this mapping typed makes a missing or misspelled Reicon a compile-time
/// error instead of silently rendering an unrelated fallback glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DesktopIcon {
    NetworkActivity,
    CameraPreview,
    SwitchCamera,
    Success,
    NavigateBack,
    Time,
    CopyEndpoint,
    ReceiverDevice,
    Overheat,
    Mirror,
    Help,
    HelpCenter,
    Home,
    Info,
    SenderDevice,
    InteractionLock,
    InteractionUnlock,
    PairingCode,
    MobileSender,
    Display,
    VirtualCamera,
    Connection,
    DarkMode,
    MoreActions,
    Disconnect,
    Start,
    Discovering,
    Refresh,
    Onboarding,
    Server,
    Settings,
    Security,
    SecureConnection,
    Sidebar,
    SidebarCollapse,
    SidebarExpand,
    StopStream,
    LightMode,
    Diagnostics,
    Network,
    Rejected,
    Remove,
}

pub(super) fn reicon_svg(data: &'static [u8], color: Hsla) -> Svg {
    svg().data(data).size_4().text_color(color)
}

pub(super) fn reicon_named(icon: DesktopIcon, color: Hsla) -> Svg {
    let data: &'static [u8] = match icon {
        DesktopIcon::NetworkActivity => {
            include_bytes!("../../../../assets/icons/reicon/activity.svg")
        }
        DesktopIcon::CameraPreview => include_bytes!("../../../../assets/icons/reicon/camera.svg"),
        DesktopIcon::SwitchCamera => {
            include_bytes!("../../../../assets/icons/reicon/camera_rotate.svg")
        }
        DesktopIcon::Success => {
            include_bytes!("../../../../assets/icons/reicon/check_circle_filled.svg")
        }
        DesktopIcon::NavigateBack => {
            include_bytes!("../../../../assets/icons/reicon/chevron_left.svg")
        }
        DesktopIcon::Time => include_bytes!("../../../../assets/icons/reicon/clock.svg"),
        DesktopIcon::CopyEndpoint => include_bytes!("../../../../assets/icons/reicon/copy.svg"),
        DesktopIcon::ReceiverDevice => {
            include_bytes!("../../../../assets/icons/reicon/desktop.svg")
        }
        DesktopIcon::Overheat => include_bytes!("../../../../assets/icons/reicon/flame.svg"),
        DesktopIcon::Mirror => {
            include_bytes!("../../../../assets/icons/reicon/flip_horizontal.svg")
        }
        DesktopIcon::Help => include_bytes!("../../../../assets/icons/reicon/help.svg"),
        DesktopIcon::HelpCenter => {
            include_bytes!("../../../../assets/icons/reicon/help_circle.svg")
        }
        DesktopIcon::Home => include_bytes!("../../../../assets/icons/reicon/home.svg"),
        DesktopIcon::Info => include_bytes!("../../../../assets/icons/reicon/info.svg"),
        DesktopIcon::SenderDevice => include_bytes!("../../../../assets/icons/reicon/iphone.svg"),
        DesktopIcon::InteractionLock => include_bytes!("../../../../assets/icons/reicon/lock.svg"),
        DesktopIcon::InteractionUnlock => {
            include_bytes!("../../../../assets/icons/reicon/unlock.svg")
        }
        DesktopIcon::PairingCode => include_bytes!("../../../../assets/icons/reicon/key.svg"),
        DesktopIcon::MobileSender => include_bytes!("../../../../assets/icons/reicon/mobile.svg"),
        DesktopIcon::Display => include_bytes!("../../../../assets/icons/reicon/monitor.svg"),
        DesktopIcon::VirtualCamera => {
            include_bytes!("../../../../assets/icons/reicon/monitor_camera.svg")
        }
        DesktopIcon::Connection => {
            include_bytes!("../../../../assets/icons/reicon/monitor_phone.svg")
        }
        DesktopIcon::DarkMode => include_bytes!("../../../../assets/icons/reicon/moon.svg"),
        DesktopIcon::MoreActions => {
            include_bytes!("../../../../assets/icons/reicon/more_horizontal.svg")
        }
        DesktopIcon::Disconnect => include_bytes!("../../../../assets/icons/reicon/phone_off.svg"),
        DesktopIcon::Start => {
            include_bytes!("../../../../assets/icons/reicon/play_filled.svg")
        }
        DesktopIcon::Discovering => include_bytes!("../../../../assets/icons/reicon/radio.svg"),
        DesktopIcon::Refresh => include_bytes!("../../../../assets/icons/reicon/refresh.svg"),
        DesktopIcon::Onboarding => include_bytes!("../../../../assets/icons/reicon/rocket.svg"),
        DesktopIcon::Server => include_bytes!("../../../../assets/icons/reicon/server.svg"),
        DesktopIcon::Settings => include_bytes!("../../../../assets/icons/reicon/settings.svg"),
        DesktopIcon::Security => include_bytes!("../../../../assets/icons/reicon/shield.svg"),
        DesktopIcon::SecureConnection => {
            include_bytes!("../../../../assets/icons/reicon/shield_check.svg")
        }
        DesktopIcon::Sidebar => include_bytes!("../../../../assets/icons/reicon/sidebar.svg"),
        DesktopIcon::SidebarCollapse => {
            include_bytes!("../../../../assets/icons/reicon/sidebar_left.svg")
        }
        DesktopIcon::SidebarExpand => {
            include_bytes!("../../../../assets/icons/reicon/sidebar_right.svg")
        }
        DesktopIcon::StopStream => include_bytes!("../../../../assets/icons/reicon/stop.svg"),
        DesktopIcon::LightMode => include_bytes!("../../../../assets/icons/reicon/sun.svg"),
        DesktopIcon::Diagnostics => include_bytes!("../../../../assets/icons/reicon/tuning.svg"),
        DesktopIcon::Network => include_bytes!("../../../../assets/icons/reicon/wifi.svg"),
        DesktopIcon::Rejected | DesktopIcon::Remove => {
            include_bytes!("../../../../assets/icons/reicon/xmark.svg")
        }
    };
    reicon_svg(data, color)
}

pub(super) fn reicon_button_content(
    label: &'static str,
    icon: DesktopIcon,
    color: Hsla,
) -> impl IntoElement {
    div()
        .h_flex()
        .gap_2()
        .child(reicon_named(icon, color))
        .child(label)
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_semantic_reicons_are_valid_svg_assets() {
        for (name, asset) in [
            (
                "activity",
                include_bytes!("../../../../assets/icons/reicon/activity.svg").as_slice(),
            ),
            (
                "camera",
                include_bytes!("../../../../assets/icons/reicon/camera.svg").as_slice(),
            ),
            (
                "check-circle-filled",
                include_bytes!("../../../../assets/icons/reicon/check_circle_filled.svg")
                    .as_slice(),
            ),
            (
                "clock",
                include_bytes!("../../../../assets/icons/reicon/clock.svg").as_slice(),
            ),
            (
                "help-circle",
                include_bytes!("../../../../assets/icons/reicon/help_circle.svg").as_slice(),
            ),
            (
                "iphone",
                include_bytes!("../../../../assets/icons/reicon/iphone.svg").as_slice(),
            ),
            (
                "key",
                include_bytes!("../../../../assets/icons/reicon/key.svg").as_slice(),
            ),
            (
                "mobile",
                include_bytes!("../../../../assets/icons/reicon/mobile.svg").as_slice(),
            ),
            (
                "monitor",
                include_bytes!("../../../../assets/icons/reicon/monitor.svg").as_slice(),
            ),
            (
                "monitor-camera",
                include_bytes!("../../../../assets/icons/reicon/monitor_camera.svg").as_slice(),
            ),
            (
                "monitor-phone",
                include_bytes!("../../../../assets/icons/reicon/monitor_phone.svg").as_slice(),
            ),
            (
                "moon",
                include_bytes!("../../../../assets/icons/reicon/moon.svg").as_slice(),
            ),
            (
                "more-horizontal",
                include_bytes!("../../../../assets/icons/reicon/more_horizontal.svg").as_slice(),
            ),
            (
                "play-filled",
                include_bytes!("../../../../assets/icons/reicon/play_filled.svg").as_slice(),
            ),
            (
                "radio",
                include_bytes!("../../../../assets/icons/reicon/radio.svg").as_slice(),
            ),
            (
                "rocket",
                include_bytes!("../../../../assets/icons/reicon/rocket.svg").as_slice(),
            ),
            (
                "server",
                include_bytes!("../../../../assets/icons/reicon/server.svg").as_slice(),
            ),
            (
                "sidebar",
                include_bytes!("../../../../assets/icons/reicon/sidebar.svg").as_slice(),
            ),
            (
                "sidebar-left",
                include_bytes!("../../../../assets/icons/reicon/sidebar_left.svg").as_slice(),
            ),
            (
                "sidebar-right",
                include_bytes!("../../../../assets/icons/reicon/sidebar_right.svg").as_slice(),
            ),
            (
                "shield",
                include_bytes!("../../../../assets/icons/reicon/shield.svg").as_slice(),
            ),
            (
                "shield-check",
                include_bytes!("../../../../assets/icons/reicon/shield_check.svg").as_slice(),
            ),
        ] {
            let svg =
                std::str::from_utf8(asset).unwrap_or_else(|_| panic!("{name} should be UTF-8 SVG"));
            assert!(
                svg.starts_with("<svg "),
                "{name} should start with an SVG root"
            );
            assert!(svg.contains("viewBox=\"0 0 24 24\""));
            assert!(svg.contains("currentColor"));
        }
    }
}
