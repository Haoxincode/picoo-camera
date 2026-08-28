//! Picoo Dark Slate — REQ-PICOO-UI-0001 / REQ-PICOO-UI-010.
//!
//! Hex lives here only. Application views must read `cx.theme()`.

use gpui::{rgb, rgba, App, Hsla};
use gpui_component::{Theme, ThemeMode};

/// Apply Dark mode, then stamp prototype tokens onto the global theme.
pub fn apply_picoo_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    {
        let theme = Theme::global_mut(cx);
        let colors = &mut theme.colors;

        let panel = hex(0x14171f);
        let panel_2 = hex(0x1b202c);
        let panel_3 = hex(0x242b3b);
        let text = hex(0xf4f2ed);
        let muted = hex(0x959dae);
        let accent = hex(0xff6a3d);
        let accent_hover = hex(0xff7d54);
        let accent_2 = hex(0xffb347);
        let ready = hex(0x3ecf8e);
        let danger = hex(0xff5c6c);
        let danger_soft = hex(0xff9da8);
        let warn = hex(0xf0c14a);
        let on_accent = hex(0x110704);
        let live_stage = hex(0x07090d);
        let live_canvas = hex(0x020305);
        let line = rgba(0x14ffffff).into();

        colors.background = panel;
        colors.foreground = text;
        colors.muted = panel_3;
        colors.muted_foreground = muted;
        colors.border = line;
        colors.title_bar = panel;
        colors.title_bar_border = line;
        colors.primary = accent;
        colors.primary_hover = accent_hover;
        colors.primary_active = accent;
        colors.primary_foreground = on_accent;
        colors.button_primary = accent;
        colors.button_primary_hover = accent_hover;
        colors.button_primary_active = accent;
        colors.button_primary_foreground = on_accent;
        colors.accent = accent;
        colors.accent_foreground = on_accent;
        colors.secondary = panel_2;
        colors.secondary_foreground = text;
        colors.secondary_hover = panel_3;
        colors.button = panel_2;
        colors.button_foreground = text;
        colors.button_hover = panel_3;
        colors.group_box = panel;
        colors.group_box_foreground = text;
        colors.popover = panel_2;
        colors.popover_foreground = text;
        colors.input = line;
        colors.success = ready;
        colors.success_foreground = ready;
        colors.danger = danger;
        colors.danger_foreground = danger_soft;
        colors.warning = accent_2;
        colors.warning_foreground = accent_2;
        colors.yellow = warn;
        colors.overlay = rgba(0xa6000000).into();
        colors.sidebar = live_stage;
        colors.tiles = live_canvas;
        colors.tab_bar = panel;
        colors.list = panel;
        colors.table = panel;
        colors.ring = accent;
        colors.link = accent_2;
        colors.window_border = line;
    }

    Theme::sync_base(cx);
}

fn hex(rgb_u24: u32) -> Hsla {
    rgb(rgb_u24).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_tokens_are_dark_slate() {
        let panel: Hsla = hex(0x14171f);
        let accent: Hsla = hex(0xff6a3d);
        let text: Hsla = hex(0xf4f2ed);
        assert!(panel.l < 0.2);
        assert!(accent.s > 0.4);
        assert!(text.l > 0.8);
    }
}
