use gpui::*;
use gpui_component::*;

pub(super) fn reicon_svg(data: &'static [u8], color: Hsla) -> Svg {
    svg().data(data).size_4().text_color(color)
}

pub(super) fn reicon_named(name: &str, color: Hsla) -> Svg {
    let data: &'static [u8] = match name {
        "activity" => include_bytes!("../../../../assets/icons/reicon/activity.svg"),
        "camera" => include_bytes!("../../../../assets/icons/reicon/camera.svg"),
        "camera-rotate" => include_bytes!("../../../../assets/icons/reicon/camera_rotate.svg"),
        "check-circle-filled" => {
            include_bytes!("../../../../assets/icons/reicon/check_circle_filled.svg")
        }
        "clock" => include_bytes!("../../../../assets/icons/reicon/clock.svg"),
        "copy" => include_bytes!("../../../../assets/icons/reicon/copy.svg"),
        "desktop" => include_bytes!("../../../../assets/icons/reicon/desktop.svg"),
        "help" => include_bytes!("../../../../assets/icons/reicon/help.svg"),
        "help-circle" => include_bytes!("../../../../assets/icons/reicon/help_circle.svg"),
        "home" => include_bytes!("../../../../assets/icons/reicon/home.svg"),
        "info" => include_bytes!("../../../../assets/icons/reicon/info.svg"),
        "iphone" => include_bytes!("../../../../assets/icons/reicon/iphone.svg"),
        "key" => include_bytes!("../../../../assets/icons/reicon/key.svg"),
        "flip-horizontal" => include_bytes!("../../../../assets/icons/reicon/flip_horizontal.svg"),
        "mobile" => include_bytes!("../../../../assets/icons/reicon/mobile.svg"),
        "monitor" => include_bytes!("../../../../assets/icons/reicon/monitor.svg"),
        "monitor-camera" => {
            include_bytes!("../../../../assets/icons/reicon/monitor_camera.svg")
        }
        "monitor-phone" => include_bytes!("../../../../assets/icons/reicon/monitor_phone.svg"),
        "moon" => include_bytes!("../../../../assets/icons/reicon/moon.svg"),
        "more-horizontal" => include_bytes!("../../../../assets/icons/reicon/more_horizontal.svg"),
        "play-filled" => include_bytes!("../../../../assets/icons/reicon/play_filled.svg"),
        "radio" => include_bytes!("../../../../assets/icons/reicon/radio.svg"),
        "refresh" => include_bytes!("../../../../assets/icons/reicon/refresh.svg"),
        "rocket" => include_bytes!("../../../../assets/icons/reicon/rocket.svg"),
        "server" => include_bytes!("../../../../assets/icons/reicon/server.svg"),
        "settings" => include_bytes!("../../../../assets/icons/reicon/settings.svg"),
        "sidebar" => include_bytes!("../../../../assets/icons/reicon/sidebar.svg"),
        "sidebar-left" => include_bytes!("../../../../assets/icons/reicon/sidebar_left.svg"),
        "sidebar-right" => include_bytes!("../../../../assets/icons/reicon/sidebar_right.svg"),
        "shield" => include_bytes!("../../../../assets/icons/reicon/shield.svg"),
        "shield-check" => include_bytes!("../../../../assets/icons/reicon/shield_check.svg"),
        "sun" => include_bytes!("../../../../assets/icons/reicon/sun.svg"),
        "tuning" => include_bytes!("../../../../assets/icons/reicon/tuning.svg"),
        "wifi" => include_bytes!("../../../../assets/icons/reicon/wifi.svg"),
        "xmark" => include_bytes!("../../../../assets/icons/reicon/xmark.svg"),
        _ => include_bytes!("../../../../assets/icons/reicon/info.svg"),
    };
    reicon_svg(data, color)
}

pub(super) fn reicon_button_content(
    label: &'static str,
    icon: &'static str,
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
