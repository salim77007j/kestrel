//! Visual design system.
//!
//! The palette and metrics here are transcribed from the product design so the
//! rendered UI matches the reference rather than approximating it. Values live
//! in one place so a theme change never has to be hunted across the widget code.

use kestrel_core::store::Theme;

/// Spacing scale in points. Everything in the UI snaps to this.
pub mod space {
    pub const XS: f32 = 2.0;
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const XXL: f32 = 24.0;
}

/// Fixed component metrics taken from the design.
pub mod metrics {
    /// Height of the tab strip.
    pub const TAB_STRIP: f32 = 44.0;
    /// Height of the navigation toolbar.
    pub const TOOLBAR: f32 = 52.0;
    /// Height of the bookmarks bar.
    pub const BOOKMARKS_BAR: f32 = 40.0;
    /// Omnibox height in the toolbar.
    pub const OMNIBOX_HEIGHT: f32 = 36.0;
    /// Tab corner radius; the active tab is a rounded-top "browser tab" shape.
    pub const TAB_RADIUS: f32 = 12.0;
    /// Omnibox corner radius (pill shape in the design).
    pub const OMNIBOX_RADIUS: f32 = 18.0;
    /// Search field on the new tab page.
    pub const SEARCH_HEIGHT: f32 = 52.0;
    /// Diameter of a new-tab shortcut tile.
    pub const SHORTCUT_TILE: f32 = 64.0;
    /// Tab icon size.
    pub const TAB_ICON: f32 = 16.0;
    /// Toolbar icon button size.
    pub const ICON_BUTTON: f32 = 32.0;
}

/// A resolved colour set for one appearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: egui::Color32,
    pub surface: egui::Color32,
    pub surface_raised: egui::Color32,
    pub surface_hover: egui::Color32,
    pub border: egui::Color32,
    pub border_strong: egui::Color32,
    pub text_primary: egui::Color32,
    pub text_secondary: egui::Color32,
    pub text_muted: egui::Color32,
    pub accent: egui::Color32,
    pub accent_hover: egui::Color32,
    pub tab_active: egui::Color32,
    pub tab_inactive: egui::Color32,
    pub tab_hover: egui::Color32,
    pub omnibox_bg: egui::Color32,
    pub omnibox_focus: egui::Color32,
    pub danger: egui::Color32,
    pub success: egui::Color32,
    pub warning: egui::Color32,
    pub shadow: egui::Color32,
    pub is_dark: bool,
}

const fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// Light palette, matching the design image: near-white chrome, soft grey
/// borders, a single blue accent, generous whitespace.
pub const LIGHT: Palette = Palette {
    background: rgb(255, 255, 255),
    surface: rgb(255, 255, 255),
    surface_raised: rgb(255, 255, 255),
    surface_hover: rgb(241, 243, 244),
    border: rgb(218, 220, 224),
    border_strong: rgb(174, 180, 186),
    text_primary: rgb(32, 33, 36),
    text_secondary: rgb(95, 99, 104),
    text_muted: rgb(138, 143, 149),
    accent: rgb(26, 115, 232),
    accent_hover: rgb(20, 97, 205),
    tab_active: rgb(255, 255, 255),
    tab_inactive: rgb(241, 243, 244),
    tab_hover: rgb(232, 234, 237),
    omnibox_bg: rgb(240, 242, 244),
    omnibox_focus: rgb(255, 255, 255),
    danger: rgb(196, 43, 28),
    success: rgb(26, 145, 86),
    warning: rgb(190, 130, 20),
    shadow: egui::Color32::from_black_alpha(24),
    is_dark: false,
};

/// Dark palette. Not a naive inversion: dark UIs need lower-contrast borders
/// and slightly desaturated accents or they vibrate against the background.
pub const DARK: Palette = Palette {
    background: rgb(32, 33, 36),
    surface: rgb(32, 33, 36),
    surface_raised: rgb(46, 48, 52),
    surface_hover: rgb(58, 61, 66),
    border: rgb(60, 63, 68),
    border_strong: rgb(95, 99, 104),
    text_primary: rgb(232, 234, 237),
    text_secondary: rgb(174, 179, 185),
    text_muted: rgb(138, 143, 149),
    accent: rgb(138, 178, 250),
    accent_hover: rgb(160, 192, 252),
    tab_active: rgb(46, 48, 52),
    tab_inactive: rgb(38, 40, 43),
    tab_hover: rgb(52, 55, 60),
    omnibox_bg: rgb(52, 55, 60),
    omnibox_focus: rgb(60, 63, 68),
    danger: rgb(234, 121, 111),
    success: rgb(52, 168, 120),
    warning: rgb(232, 184, 92),
    shadow: egui::Color32::from_black_alpha(120),
    is_dark: true,
};

/// Resolve a theme setting against the OS preference.
pub fn resolve(theme: Theme) -> Palette {
    match theme {
        Theme::Light => LIGHT,
        Theme::Dark => DARK,
        Theme::System => {
            if prefer_dark() {
                DARK
            } else {
                LIGHT
            }
        }
    }
}

fn prefer_dark() -> bool {
    // Honour the desktop setting where it is discoverable. Absence is not an
    // error: the default is light, matching the design reference.
    std::env::var("KESTREL_THEME").map(|v| v == "dark").unwrap_or_else(|_| {
        std::fs::read_to_string("/etc/gtk-3.0/settings.ini")
            .map(|s| {
                s.lines()
                    .any(|l| l.contains("gtk-theme-name") && l.contains("dark"))
            })
            .unwrap_or(false)
    })
}

/// Push the palette into egui's global style for this frame.
pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut style = (*ctx.style()).clone();

    style.visuals.window_bg = p.background;
    style.visuals.panel_fill = p.background;
    style.visuals.extreme_bg_color = p.background;
    style.visuals.faint_bg_color = p.surface_hover;
    style.visuals.override_text_color = Some(p.text_primary);

    style.visuals.widgets.noninteractive.bg_fill = p.surface;
    style.visuals.widgets.noninteractive.weak_bg_fill = p.surface;
    style.visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, p.border);
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.text_primary);

    style.visuals.widgets.inactive.bg_fill = p.surface;
    style.visuals.widgets.inactive.weak_bg_fill = p.surface_hover;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.border);
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.5, p.text_secondary);

    style.visuals.widgets.hovered.bg_fill = p.surface_hover;
    style.visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.border_strong);
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, p.text_primary);

    style.visuals.widgets.active.bg_fill = p.accent;
    style.visuals.widgets.active.weak_bg_fill = p.accent_hover;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, p.accent);
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);

    style.visuals.widgets.open.bg_fill = p.surface_hover;
    style.visuals.widgets.open.weak_bg_fill = p.surface_hover;
    style.visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, p.border_strong);
    style.visuals.widgets.open.fg_stroke = egui::Stroke::new(1.5, p.text_primary);

    style.visuals.selection.bg_fill = p.accent.gamma_multiply(0.25);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);

    style.visuals.hyperlink_color = p.accent;
    style.visuals.error_fg_color = p.danger;
    style.visuals.warn_fg_color = p.warning;

    style.spacing.item_spacing = egui::vec2(space::MD, space::MD);
    style.spacing.button_padding = egui::vec2(space::LG, space::SM);
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.interact_size.y = metrics::ICON_BUTTON;

    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::proportional(22.0).strong(),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::proportional(14.0),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::proportional(14.0),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::proportional(12.0),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::monospace(13.0),
        ),
        (
            egui::TextStyle::Name,
            egui::FontId::proportional(26.0).strong(),
        ),
    ]
    .into();

    ctx.set_style(style);
}

/// Colour used for the security indicator in the omnibox.
pub fn security_color(p: &Palette, secure: bool, mixed: bool) -> egui::Color32 {
    if mixed {
        p.warning
    } else if secure {
        p.success
    } else {
        p.danger
    }
}

/// Convert HSL to RGB, used to give each site a stable, legible tile colour.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> egui::Color32 {
    let h = h.rem_euclid(1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    egui::Color32::from_rgb(
        (((r + m).clamp(0.0, 1.0) * 255.0) as u8),
        (((g + m).clamp(0.0, 1.0) * 255.0) as u8),
        (((b + m).clamp(0.0, 1.0) * 255.0) as u8),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_palettes_are_fully_populated() {
        // Guards against a colour being left as the transparent default.
        for p in [LIGHT, DARK] {
            assert_eq!(p.background.a(), 255, "background must be opaque");
            assert_ne!(p.background, egui::Color32::TRANSPARENT);
            assert_ne!(p.accent, egui::Color32::TRANSPARENT);
            assert!(p.is_dark == (p.background == DARK.background));
        }
    }

    #[test]
    fn light_palette_has_readable_contrast() {
        // Relative luminance difference between text and background must clear
        // the WCAG AA threshold for body text (4.5:1).
        fn luminance(c: egui::Color32) -> f32 {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        let l1 = luminance(LIGHT.text_primary);
        let l2 = luminance(LIGHT.background);
        let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
        assert!(ratio >= 4.5, "text contrast was {ratio:.2}:1, needs 4.5:1");
    }

    #[test]
    fn dark_palette_has_readable_contrast() {
        fn luminance(c: egui::Color32) -> f32 {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        let l1 = luminance(DARK.text_primary);
        let l2 = luminance(DARK.background);
        let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
        assert!(ratio >= 4.5, "text contrast was {ratio:.2}:1, needs 4.5:1");
    }

    #[test]
    fn metrics_are_positive() {
        assert!(metrics::TAB_STRIP > 0.0);
        assert!(metrics::OMNIBOX_RADIUS * 2.0 <= metrics::OMNIBOX_HEIGHT);
    }
}
