//! 桌面壳初始页 — REQ-PICOO-UI-002 / REQ-PICOO-UI-010。
//!
//! `PICOO_UI_PREVIEW_PAGE` 只覆盖打开哪一页，不改变产品 Receiver 范围。
//! VCam 不适用时默认 Waiting（HTML `#d-view-idle`），不把 First Launch 当主视觉。
//!
//! rust-and-docs CI 编 bin 时不开 `gpui-ui`，这些符号只在 GPUI 壳和单测里用。
#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPreviewPage {
    FirstLaunch,
    Waiting,
    Live,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialDesktopPage {
    FirstLaunch,
    Waiting,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialShell {
    pub page: InitialDesktopPage,
    pub settings_open: bool,
}

pub fn preview_page_from_env() -> Option<UiPreviewPage> {
    parse_ui_preview_page(std::env::var("PICOO_UI_PREVIEW_PAGE").ok().as_deref())
}

pub fn parse_ui_preview_page(raw: Option<&str>) -> Option<UiPreviewPage> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "first-launch" | "first_launch" => Some(UiPreviewPage::FirstLaunch),
        "waiting" => Some(UiPreviewPage::Waiting),
        "live" => Some(UiPreviewPage::Live),
        "settings" => Some(UiPreviewPage::Settings),
        _ => None,
    }
}

/// Resolve the first painted desktop shell.
///
/// Linux verification (VCam unsupported) defaults to Waiting so the loop
/// matches the HTML prototype, not the Windows VCam install gate.
pub fn resolve_initial_shell(
    preview: Option<UiPreviewPage>,
    first_launch_completed: bool,
    vcam_unsupported: bool,
) -> InitialShell {
    match preview {
        Some(UiPreviewPage::FirstLaunch) => InitialShell {
            page: InitialDesktopPage::FirstLaunch,
            settings_open: false,
        },
        Some(UiPreviewPage::Waiting) => InitialShell {
            page: InitialDesktopPage::Waiting,
            settings_open: false,
        },
        Some(UiPreviewPage::Live) => InitialShell {
            page: InitialDesktopPage::Live,
            settings_open: false,
        },
        Some(UiPreviewPage::Settings) => InitialShell {
            page: InitialDesktopPage::Waiting,
            settings_open: true,
        },
        None if vcam_unsupported || first_launch_completed => InitialShell {
            page: InitialDesktopPage::Waiting,
            settings_open: false,
        },
        None => InitialShell {
            page: InitialDesktopPage::FirstLaunch,
            settings_open: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_pages() {
        assert_eq!(
            parse_ui_preview_page(Some("first-launch")),
            Some(UiPreviewPage::FirstLaunch)
        );
        assert_eq!(
            parse_ui_preview_page(Some("WAITING")),
            Some(UiPreviewPage::Waiting)
        );
        assert_eq!(
            parse_ui_preview_page(Some("settings")),
            Some(UiPreviewPage::Settings)
        );
        assert_eq!(parse_ui_preview_page(Some("nope")), None);
        assert_eq!(parse_ui_preview_page(None), None);
    }

    #[test]
    fn linux_verification_defaults_to_waiting() {
        let shell = resolve_initial_shell(None, false, true);
        assert_eq!(
            shell,
            InitialShell {
                page: InitialDesktopPage::Waiting,
                settings_open: false,
            }
        );
    }

    #[test]
    fn windows_first_launch_gate_stays_until_completed() {
        let shell = resolve_initial_shell(None, false, false);
        assert_eq!(shell.page, InitialDesktopPage::FirstLaunch);
        let done = resolve_initial_shell(None, true, false);
        assert_eq!(done.page, InitialDesktopPage::Waiting);
    }

    #[test]
    fn preview_override_can_open_settings_on_waiting() {
        let shell = resolve_initial_shell(Some(UiPreviewPage::Settings), false, true);
        assert_eq!(shell.page, InitialDesktopPage::Waiting);
        assert!(shell.settings_open);
    }
}
