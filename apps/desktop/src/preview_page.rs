//! Linux / 开发机 GPUI 预览页覆盖 — REQ-PICOO-UI-010。
//!
//! 只改变打开哪一页，不改变产品 Receiver 范围，也不注册虚拟摄像头。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPreviewPage {
    FirstLaunch,
    Waiting,
    Live,
    Settings,
}

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
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
}
