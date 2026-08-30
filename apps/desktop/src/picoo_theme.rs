//! Picoo desktop semantic theme.
//!
//! The sRGB values below are the gamut-clamped equivalents of the OKLCH
//! tokens in `picoo-camera-receiver.html`. Product UI consumes the resolved
//! semantic roles through `cx.theme()`; raw palette values stay in this file.

use gpui::App;
use gpui_component::{Theme, ThemeMode, ThemeRegistry};

const DEFAULT_THEME_MODE: ThemeMode = ThemeMode::Light;

const PICOO_THEME_SET: &str = r##"
{
  "name": "Picoo Camera",
  "author": "Picoo Camera",
  "themes": [
    {
      "name": "Picoo Light",
      "mode": "light",
      "font.size": 16,
      "radius": 7,
      "radius.lg": 15,
      "shadow": true,
      "colors": {
        "background": "#FFFFFF",
        "foreground": "#0A0A0A",
        "border": "#E5E5E5",
        "input.border": "#E5E5E5",
        "group_box.background": "#FFFFFF",
        "group_box.foreground": "#0A0A0A",
        "popover.background": "#FFFFFF",
        "popover.foreground": "#0A0A0A",
        "primary.background": "#1447E6",
        "primary.foreground": "#EFF6FF",
        "secondary.background": "#F4F4F5",
        "secondary.foreground": "#18181B",
        "secondary.hover.background": "#E4E4E7",
        "secondary.active.background": "#F5F5F5",
        "muted.background": "#F5F5F5",
        "muted.foreground": "#737373",
        "accent.background": "#1447E6",
        "accent.foreground": "#EFF6FF",
        "danger.background": "#E7000B",
        "danger.foreground": "#FFFFFF",
        "success.background": "#00BC7D",
        "success.foreground": "#052E22",
        "ring": "#A1A1A1",
        "button.background": "#F4F4F5",
        "button.foreground": "#18181B",
        "button.hover.background": "#E4E4E7",
        "button.active.background": "#D4D4D8",
        "sidebar.background": "#FAFAFA",
        "sidebar.foreground": "#0A0A0A",
        "sidebar.primary.background": "#155DFC",
        "sidebar.primary.foreground": "#EFF6FF",
        "sidebar.accent.background": "#F5F5F5",
        "sidebar.accent.foreground": "#171717",
        "sidebar.border": "#E5E5E5",
        "title_bar.background": "#FAFAFA",
        "title_bar.border": "#E5E5E5",
        "list.background": "#FFFFFF",
        "list.hover.background": "#F5F5F5",
        "table.background": "#FFFFFF",
        "table.hover.background": "#F5F5F5",
        "scrollbar.background": "#00000000",
        "scrollbar.thumb.background": "#A1A1A1B3",
        "scrollbar.thumb.hover.background": "#737373",
        "selection.background": "#1447E64D",
        "overlay": "#00000066"
      }
    },
    {
      "name": "Picoo Dark",
      "mode": "dark",
      "font.size": 16,
      "radius": 7,
      "radius.lg": 15,
      "shadow": true,
      "colors": {
        "background": "#0A0A0A",
        "foreground": "#FAFAFA",
        "border": "#FFFFFF1A",
        "input.border": "#FFFFFF26",
        "group_box.background": "#171717",
        "group_box.foreground": "#FAFAFA",
        "popover.background": "#171717",
        "popover.foreground": "#FAFAFA",
        "primary.background": "#193CB8",
        "primary.foreground": "#EFF6FF",
        "secondary.background": "#27272A",
        "secondary.foreground": "#FAFAFA",
        "secondary.hover.background": "#3F3F46",
        "secondary.active.background": "#262626",
        "muted.background": "#262626",
        "muted.foreground": "#A1A1A1",
        "accent.background": "#193CB8",
        "accent.foreground": "#EFF6FF",
        "danger.background": "#FF6467",
        "danger.foreground": "#0A0A0A",
        "success.background": "#00BC7D",
        "success.foreground": "#052E22",
        "ring": "#737373",
        "button.background": "#27272A",
        "button.foreground": "#FAFAFA",
        "button.hover.background": "#3F3F46",
        "button.active.background": "#52525B",
        "sidebar.background": "#171717",
        "sidebar.foreground": "#FAFAFA",
        "sidebar.primary.background": "#2B7FFF",
        "sidebar.primary.foreground": "#EFF6FF",
        "sidebar.accent.background": "#262626",
        "sidebar.accent.foreground": "#FAFAFA",
        "sidebar.border": "#FFFFFF1A",
        "title_bar.background": "#171717",
        "title_bar.border": "#FFFFFF1A",
        "list.background": "#171717",
        "list.hover.background": "#262626",
        "table.background": "#171717",
        "table.hover.background": "#262626",
        "scrollbar.background": "#00000000",
        "scrollbar.thumb.background": "#737373B3",
        "scrollbar.thumb.hover.background": "#A1A1A1",
        "selection.background": "#2B7FFF4D",
        "overlay": "#00000099"
      }
    }
  ]
}
"##;

/// Install both product themes before the first Receiver window is created.
pub fn install(cx: &mut App) {
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(PICOO_THEME_SET)
        .expect("bundled Picoo theme must be valid");

    let light = ThemeRegistry::global(cx)
        .themes()
        .get("Picoo Light")
        .cloned()
        .expect("Picoo Light theme must be registered");
    let dark = ThemeRegistry::global(cx)
        .themes()
        .get("Picoo Dark")
        .cloned()
        .expect("Picoo Dark theme must be registered");

    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    Theme::change(DEFAULT_THEME_MODE, None, cx);
}

#[cfg(test)]
mod tests {
    use gpui_component::ThemeSet;

    use super::{DEFAULT_THEME_MODE, PICOO_THEME_SET};

    #[test]
    fn bundled_theme_contains_light_and_dark_variants() {
        let set: ThemeSet = serde_json::from_str(PICOO_THEME_SET).expect("valid theme JSON");
        assert_eq!(set.themes.len(), 2);
        assert!(set.themes.iter().any(|theme| theme.mode.is_dark()));
        assert!(set.themes.iter().any(|theme| !theme.mode.is_dark()));
    }

    #[test]
    fn desktop_defaults_to_light_theme() {
        assert!(!DEFAULT_THEME_MODE.is_dark());
    }
}
