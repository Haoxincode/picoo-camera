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

impl DesktopIcon {
    /// The complete semantic surface. Keep this exhaustive when adding a
    /// product icon so asset validation covers every callable variant.
    const ALL: [Self; 42] = [
        Self::NetworkActivity,
        Self::CameraPreview,
        Self::SwitchCamera,
        Self::Success,
        Self::NavigateBack,
        Self::Time,
        Self::CopyEndpoint,
        Self::ReceiverDevice,
        Self::Overheat,
        Self::Mirror,
        Self::Help,
        Self::HelpCenter,
        Self::Home,
        Self::Info,
        Self::SenderDevice,
        Self::InteractionLock,
        Self::InteractionUnlock,
        Self::PairingCode,
        Self::MobileSender,
        Self::Display,
        Self::VirtualCamera,
        Self::Connection,
        Self::DarkMode,
        Self::MoreActions,
        Self::Disconnect,
        Self::Start,
        Self::Discovering,
        Self::Refresh,
        Self::Onboarding,
        Self::Server,
        Self::Settings,
        Self::Security,
        Self::SecureConnection,
        Self::Sidebar,
        Self::SidebarCollapse,
        Self::SidebarExpand,
        Self::StopStream,
        Self::LightMode,
        Self::Diagnostics,
        Self::Network,
        Self::Rejected,
        Self::Remove,
    ];

    fn asset(self) -> &'static [u8] {
        match self {
            Self::NetworkActivity => include_bytes!("../../../../assets/icons/reicon/activity.svg"),
            Self::CameraPreview => include_bytes!("../../../../assets/icons/reicon/camera.svg"),
            Self::SwitchCamera => {
                include_bytes!("../../../../assets/icons/reicon/camera_rotate.svg")
            }
            Self::Success => {
                include_bytes!("../../../../assets/icons/reicon/check_circle_filled.svg")
            }
            Self::NavigateBack => {
                include_bytes!("../../../../assets/icons/reicon/chevron_left.svg")
            }
            Self::Time => include_bytes!("../../../../assets/icons/reicon/clock.svg"),
            Self::CopyEndpoint => include_bytes!("../../../../assets/icons/reicon/copy.svg"),
            Self::ReceiverDevice => include_bytes!("../../../../assets/icons/reicon/desktop.svg"),
            Self::Overheat => include_bytes!("../../../../assets/icons/reicon/flame.svg"),
            Self::Mirror => include_bytes!("../../../../assets/icons/reicon/flip_horizontal.svg"),
            Self::Help => include_bytes!("../../../../assets/icons/reicon/help.svg"),
            Self::HelpCenter => include_bytes!("../../../../assets/icons/reicon/help_circle.svg"),
            Self::Home => include_bytes!("../../../../assets/icons/reicon/home.svg"),
            Self::Info => include_bytes!("../../../../assets/icons/reicon/info.svg"),
            Self::SenderDevice => include_bytes!("../../../../assets/icons/reicon/iphone.svg"),
            Self::InteractionLock => include_bytes!("../../../../assets/icons/reicon/lock.svg"),
            Self::InteractionUnlock => include_bytes!("../../../../assets/icons/reicon/unlock.svg"),
            Self::PairingCode => include_bytes!("../../../../assets/icons/reicon/key.svg"),
            Self::MobileSender => include_bytes!("../../../../assets/icons/reicon/mobile.svg"),
            Self::Display => include_bytes!("../../../../assets/icons/reicon/monitor.svg"),
            Self::VirtualCamera => {
                include_bytes!("../../../../assets/icons/reicon/monitor_camera.svg")
            }
            Self::Connection => include_bytes!("../../../../assets/icons/reicon/monitor_phone.svg"),
            Self::DarkMode => include_bytes!("../../../../assets/icons/reicon/moon.svg"),
            Self::MoreActions => {
                include_bytes!("../../../../assets/icons/reicon/more_horizontal.svg")
            }
            Self::Disconnect => include_bytes!("../../../../assets/icons/reicon/phone_off.svg"),
            Self::Start => include_bytes!("../../../../assets/icons/reicon/play_filled.svg"),
            Self::Discovering => include_bytes!("../../../../assets/icons/reicon/radio.svg"),
            Self::Refresh => include_bytes!("../../../../assets/icons/reicon/refresh.svg"),
            Self::Onboarding => include_bytes!("../../../../assets/icons/reicon/rocket.svg"),
            Self::Server => include_bytes!("../../../../assets/icons/reicon/server.svg"),
            Self::Settings => include_bytes!("../../../../assets/icons/reicon/settings.svg"),
            Self::Security => include_bytes!("../../../../assets/icons/reicon/shield.svg"),
            Self::SecureConnection => {
                include_bytes!("../../../../assets/icons/reicon/shield_check.svg")
            }
            Self::Sidebar => include_bytes!("../../../../assets/icons/reicon/sidebar.svg"),
            Self::SidebarCollapse => {
                include_bytes!("../../../../assets/icons/reicon/sidebar_left.svg")
            }
            Self::SidebarExpand => {
                include_bytes!("../../../../assets/icons/reicon/sidebar_right.svg")
            }
            Self::StopStream => include_bytes!("../../../../assets/icons/reicon/stop.svg"),
            Self::LightMode => include_bytes!("../../../../assets/icons/reicon/sun.svg"),
            Self::Diagnostics => include_bytes!("../../../../assets/icons/reicon/tuning.svg"),
            Self::Network => include_bytes!("../../../../assets/icons/reicon/wifi.svg"),
            Self::Rejected | Self::Remove => {
                include_bytes!("../../../../assets/icons/reicon/xmark.svg")
            }
        }
    }
}

pub(super) fn reicon_svg(data: &'static [u8], color: Hsla) -> Svg {
    svg().data(data).size_4().text_color(color)
}

pub(super) fn reicon_named(icon: DesktopIcon, color: Hsla) -> Svg {
    reicon_svg(icon.asset(), color)
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
    fn every_desktop_semantic_reicon_is_a_valid_svg_asset() {
        for icon in DesktopIcon::ALL {
            let asset = match icon {
                DesktopIcon::NetworkActivity => {
                    include_bytes!("../../../../assets/icons/reicon/activity.svg").as_slice()
                }
                DesktopIcon::CameraPreview => {
                    include_bytes!("../../../../assets/icons/reicon/camera.svg").as_slice()
                }
                DesktopIcon::SwitchCamera => {
                    include_bytes!("../../../../assets/icons/reicon/camera_rotate.svg").as_slice()
                }
                DesktopIcon::Success => {
                    include_bytes!("../../../../assets/icons/reicon/check_circle_filled.svg")
                        .as_slice()
                }
                DesktopIcon::NavigateBack => {
                    include_bytes!("../../../../assets/icons/reicon/chevron_left.svg").as_slice()
                }
                DesktopIcon::Time => {
                    include_bytes!("../../../../assets/icons/reicon/clock.svg").as_slice()
                }
                DesktopIcon::CopyEndpoint => {
                    include_bytes!("../../../../assets/icons/reicon/copy.svg").as_slice()
                }
                DesktopIcon::ReceiverDevice => {
                    include_bytes!("../../../../assets/icons/reicon/desktop.svg").as_slice()
                }
                DesktopIcon::Overheat => {
                    include_bytes!("../../../../assets/icons/reicon/flame.svg").as_slice()
                }
                DesktopIcon::Mirror => {
                    include_bytes!("../../../../assets/icons/reicon/flip_horizontal.svg").as_slice()
                }
                DesktopIcon::Help => {
                    include_bytes!("../../../../assets/icons/reicon/help.svg").as_slice()
                }
                DesktopIcon::HelpCenter => {
                    include_bytes!("../../../../assets/icons/reicon/help_circle.svg").as_slice()
                }
                DesktopIcon::Home => {
                    include_bytes!("../../../../assets/icons/reicon/home.svg").as_slice()
                }
                DesktopIcon::Info => {
                    include_bytes!("../../../../assets/icons/reicon/info.svg").as_slice()
                }
                DesktopIcon::SenderDevice => {
                    include_bytes!("../../../../assets/icons/reicon/iphone.svg").as_slice()
                }
                DesktopIcon::InteractionLock => {
                    include_bytes!("../../../../assets/icons/reicon/lock.svg").as_slice()
                }
                DesktopIcon::InteractionUnlock => {
                    include_bytes!("../../../../assets/icons/reicon/unlock.svg").as_slice()
                }
                DesktopIcon::PairingCode => {
                    include_bytes!("../../../../assets/icons/reicon/key.svg").as_slice()
                }
                DesktopIcon::MobileSender => {
                    include_bytes!("../../../../assets/icons/reicon/mobile.svg").as_slice()
                }
                DesktopIcon::Display => {
                    include_bytes!("../../../../assets/icons/reicon/monitor.svg").as_slice()
                }
                DesktopIcon::VirtualCamera => {
                    include_bytes!("../../../../assets/icons/reicon/monitor_camera.svg").as_slice()
                }
                DesktopIcon::Connection => {
                    include_bytes!("../../../../assets/icons/reicon/monitor_phone.svg").as_slice()
                }
                DesktopIcon::DarkMode => {
                    include_bytes!("../../../../assets/icons/reicon/moon.svg").as_slice()
                }
                DesktopIcon::MoreActions => {
                    include_bytes!("../../../../assets/icons/reicon/more_horizontal.svg").as_slice()
                }
                DesktopIcon::Disconnect => {
                    include_bytes!("../../../../assets/icons/reicon/phone_off.svg").as_slice()
                }
                DesktopIcon::Start => {
                    include_bytes!("../../../../assets/icons/reicon/play_filled.svg").as_slice()
                }
                DesktopIcon::Discovering => {
                    include_bytes!("../../../../assets/icons/reicon/radio.svg").as_slice()
                }
                DesktopIcon::Refresh => {
                    include_bytes!("../../../../assets/icons/reicon/refresh.svg").as_slice()
                }
                DesktopIcon::Onboarding => {
                    include_bytes!("../../../../assets/icons/reicon/rocket.svg").as_slice()
                }
                DesktopIcon::Server => {
                    include_bytes!("../../../../assets/icons/reicon/server.svg").as_slice()
                }
                DesktopIcon::Settings => {
                    include_bytes!("../../../../assets/icons/reicon/settings.svg").as_slice()
                }
                DesktopIcon::Security => {
                    include_bytes!("../../../../assets/icons/reicon/shield.svg").as_slice()
                }
                DesktopIcon::SecureConnection => {
                    include_bytes!("../../../../assets/icons/reicon/shield_check.svg").as_slice()
                }
                DesktopIcon::Sidebar => {
                    include_bytes!("../../../../assets/icons/reicon/sidebar.svg").as_slice()
                }
                DesktopIcon::SidebarCollapse => {
                    include_bytes!("../../../../assets/icons/reicon/sidebar_left.svg").as_slice()
                }
                DesktopIcon::SidebarExpand => {
                    include_bytes!("../../../../assets/icons/reicon/sidebar_right.svg").as_slice()
                }
                DesktopIcon::StopStream => {
                    include_bytes!("../../../../assets/icons/reicon/stop.svg").as_slice()
                }
                DesktopIcon::LightMode => {
                    include_bytes!("../../../../assets/icons/reicon/sun.svg").as_slice()
                }
                DesktopIcon::Diagnostics => {
                    include_bytes!("../../../../assets/icons/reicon/tuning.svg").as_slice()
                }
                DesktopIcon::Network => {
                    include_bytes!("../../../../assets/icons/reicon/wifi.svg").as_slice()
                }
                DesktopIcon::Rejected | DesktopIcon::Remove => {
                    include_bytes!("../../../../assets/icons/reicon/xmark.svg").as_slice()
                }
            };
            assert_eq!(
                asset,
                icon.asset(),
                "test asset must match production mapping"
            );
            let name = format!("{icon:?}");
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
