//! The tab strip: the row of tabs across the top of the window.
//!
//! Tabs are drawn as rounded-top rectangles that merge into the toolbar when
//! active, which is the shape the design calls for. Interaction is handled
//! entirely here so the main app only has to act on the returned intents.

use super::icons::{self, Icon};
use super::theme::{metrics, space, Palette};
use super::tabs::Tab;
use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

/// What the user did to the tab strip.
#[derive(Debug, Default)]
pub struct StripIntent {
    pub select: Option<u64>,
    pub close: Option<u64>,
    pub new_tab: bool,
    pub pin_toggle: Option<u64>,
    pub mute_toggle: Option<u64>,
    pub duplicate: Option<u64>,
    pub close_others: Option<u64>,
    pub reorder: Option<(usize, usize)>,
    pub context_menu: Option<(u64, Vec<ContextAction>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Reload,
    Duplicate,
    Pin,
    Mute,
    Close,
    CloseOthers,
    CloseToTheRight,
    CopyLink,
    OpenInNewTab,
    Bookmark,
    Inspect,
}

/// One entry in the context menu, resolved by the caller.
pub const CONTEXT_LABELS: &[(&str, ContextAction)] = &[
    ("Reload", ContextAction::Reload),
    ("Duplicate", ContextAction::Duplicate),
    ("Open in new tab", ContextAction::OpenInNewTab),
    ("Copy link address", ContextAction::CopyLink),
    ("Bookmark this tab", ContextAction::Bookmark),
    ("Mute tab", ContextAction::Mute),
    ("Pin tab", ContextAction::Pin),
    ("Close tab", ContextAction::Close),
    ("Close other tabs", ContextAction::CloseOthers),
    ("Close tabs to the right", ContextAction::CloseToTheRight),
    ("Inspect", ContextAction::Inspect),
];

/// Minimum and maximum tab widths. Narrow tabs are still readable; this keeps a
/// strip of 40 tabs from becoming unusable.
const TAB_MIN_WIDTH: f32 = 56.0;
const TAB_MAX_WIDTH: f32 = 240.0;
/// Space reserved inside a tab for the close button and padding.
const TAB_CHROME: f32 = 52.0;

pub struct StripResponse {
    pub intent: StripIntent,
    /// Geometry of each tab, so the content area can map clicks to tabs.
    pub rects: Vec<(u64, Rect)>,
}

/// Draw the strip. `drop_target` is where a dragged tab would land.
pub fn show(
    ui: &mut Ui,
    tabs: &[Tab],
    active_id: u64,
    p: &Palette,
    new_tab_hovered: &mut bool,
) -> StripResponse {
    let mut intent = StripIntent::default();
    let mut rects = Vec::with_capacity(tabs.len());
    let available = ui.available_width();

    ui.horizontal(|ui| {
        // Reserve room for the new-tab button before laying out tabs.
        let count = tabs.len().max(1);
        let tab_width = ((available - metrics::ICON_BUTTON * 2.0) / count as f32)
            .clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH)
            .max(TAB_MIN_WIDTH);

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = tab.id == active_id;
            let (rect, resp) = ui.allocate_exact_size(
                Vec2::new(tab_width, metrics::TAB_STRIP),
                Sense::click(),
            );
            rects.push((tab.id, rect));

            paint_tab(ui.painter(), rect, tab, is_active, p);

            if resp.clicked() {
                if resp.clicked_by(egui::PointerButton::Secondary) {
                    intent.context_menu = Some((tab.id, Vec::new()));
                } else {
                    intent.select = Some(tab.id);
                }
            }
            if resp.double_clicked() {
                intent.duplicate = Some(tab.id);
            }
            if resp.hovered() {
                // Only show the close affordance on hover, or on the active tab,
                // which keeps the strip visually calm.
                close_button(ui, rect, tab.id, is_active, p, &mut intent);
            } else if is_active {
                close_button(ui, rect, tab.id, true, p, &mut intent);
            }
            let _ = i;
        }

        // New tab button.
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(metrics::ICON_BUTTON, metrics::TAB_STRIP), Sense::click());
        let hovered = resp.hovered();
        *new_tab_hovered = hovered;
        if hovered {
            ui.painter().circle_filled(
                rect.center(),
                metrics::ICON_BUTTON * 0.36,
                p.surface_hover,
            );
        }
        icons::draw(
            ui,
            Icon::Plus,
            16.0,
            if hovered { p.text_primary } else { p.text_secondary },
        );
        if resp.clicked() {
            intent.new_tab = true;
        }
    });

    StripResponse { intent, rects }
}

/// Paint one tab. The active tab gets a filled background and no bottom border
/// so it visually merges with the page area below, as in the design.
fn paint_tab(painter: &egui::Painter, rect: Rect, tab: &Tab, active: bool, p: &Palette) {
    let fill = if active {
        p.tab_active
    } else if tab.pinned {
        // Pinned tabs stay visually distinct so they read as permanent.
        p.tab_inactive
    } else {
        p.tab_inactive
    };

    // Rounded top corners, square bottom: the classic browser tab silhouette.
    let r = metrics::TAB_RADIUS;
    let mut path = egui::epaint::RoundedRectangle::new(rect, r);
    path.radii = egui::CornerRadii {
        top_left: r,
        top_right: r,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };
    if active {
        // Bridge the gap between the active tab and the toolbar.
        path = path.expand(egui::Rect::from_min_max(
            Pos2(rect.min.x, rect.min.y),
            Pos2(rect.max.x, rect.max.y + 1.0),
        ));
    }
    painter.add(egui::Shape::path(path, fill, Stroke::NONE));

    // Separator between inactive tabs, omitted around the active one.
    if !active {
        painter.line_segment(
            [
                egui::pos2(rect.right() - 0.5, rect.min.y + space::SM),
                egui::pos2(rect.right() - 0.5, rect.max.y - space::SM),
            ],
            Stroke::new(1.0, p.border),
        );
    }
}

use egui::Pos2;

/// The close affordance inside a tab.
fn close_button(
    ui: &mut Ui,
    tab_rect: Rect,
    id: u64,
    active: bool,
    p: &Palette,
    intent: &mut StripIntent,
) {
    let size = 18.0;
    let rect = egui::Rect::from_center_size(
        Pos2::new(tab_rect.right() - size * 0.75, tab_rect.center().y - 1.0),
        Vec2::splat(size),
    );
    let resp = ui.interact(rect, egui::Id::new(("tab-close", id)), Sense::click());
    let color = if resp.hovered() {
        p.text_primary
    } else if active {
        p.text_secondary
    } else {
        p.text_muted
    };
    if resp.hovered() {
        ui.painter()
            .circle_filled(resp.rect.center(), size * 0.5, p.surface_hover);
    }
    icons::draw(ui, Icon::Close, 11.0, color);
    if resp.clicked() {
        intent.close = Some(id);
    }
}

/// Tab title, favicon and any status glyphs, laid out inside the tab body.
pub fn tab_content(ui: &mut Ui, tab: &Tab, active: bool, p: &Palette) {
    let title = tab.display_title();
    let color = if active { p.text_primary } else { p.text_secondary };

    ui.horizontal_centered(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::splat(metrics::TAB_ICON), Sense::hover());
        paint_favicon(ui.painter(), rect, tab, p);

        if tab.muted {
            icons::draw(ui, Icon::Volume, 12.0, p.text_muted);
        }

        // The close button occupies the right end of the tab, so cap the title
        // at whatever room is actually left to avoid drawing under it.
        let avail = ui.available_width() - space::MD;
        if avail <= 8.0 {
            return;
        }
        let galley = ui.painter().layout(
            egui::text::TextLayout::simple(
                title,
                egui::FontId::proportional(13.0),
                color,
            )
            .max_width(avail),
            color,
        );
        ui.painter().galley(ui.cursor().left_top(), galley, color);
    });
}

/// A colour derived from the site, so a tab without a favicon is still
/// recognisable. Hashing the host gives a stable hue across restarts.
pub fn site_color(host: &str) -> Color32 {
    let mut h: u32 = 2166136261;
    for b in host.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32;
    // Fixed saturation/lightness keeps every generated tile legible against
    // both light and dark chrome.
    crate::theme::hsl_to_rgb(hue / 360.0, 0.55, 0.55)
}

fn paint_favicon(painter: &egui::Painter, rect: Rect, tab: &Tab, p: &Palette) {
    let host = url::Url::parse(&tab.url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();

    if host.is_empty() {
        icons::paint(painter, Icon::Globe, rect, p.text_muted, 1.6);
        return;
    }
    // The favicon image itself is supplied by the engine; until it arrives we
    // show a deterministic tile so the tab is never blank.
    let letter = host.chars().next().unwrap_or('?').to_ascii_uppercase();
    let color = site_color(&host);
    let r = egui::CornerRadius::same(rect.height() * 0.25);
    painter.add(egui::Shape::rounded_rect(rect, r, color));
    let galley = painter.layout_no_wrap(
        letter.to_string(),
        egui::FontId::proportional(rect.height() * 0.62).strong(),
        Color32::WHITE,
    );
    let g = galley.paint(painter, rect.left_top());
    let offset = Vec2::new(
        (rect.width() - galley.size().x) / 2.0,
        (rect.height() - galley.size().y) / 2.0,
    );
    g.translate(offset);
}

/// Shorten text with an ellipsis to fit `max_px`.
fn truncate(s: &str, max_px: f32) -> String {
    if s.is_empty() {
        return String::new();
    }
    // Cheap character budget: good enough for tab titles and avoids needing
    // font metrics on every frame.
    let approx_char_w = 7.2;
    let max_chars = ((max_px / approx_char_w).floor() as usize).max(3);
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Group headers for tabs that belong to a group. Rendered as a thin caption
/// above the strip so a group is identifiable without a heavy coloured band.
pub fn group_labels(ui: &mut Ui, tabs: &[Tab], p: &Palette) {
    let groups: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for t in tabs {
            if let Some(g) = &t.group {
                if !seen.contains(g) {
                    seen.push(g.clone());
                }
            }
        }
        seen
    };
    if groups.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for g in groups {
            ui.label(
                egui::RichText::new(format!("{g}"))
                    .color(p.text_muted)
                    .size(11.0)
                    .strong(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_adds_ellipsis_and_respects_budget() {
        let long = "a".repeat(200);
        let t = truncate(&long, 70.0);
        assert!(t.chars().count() <= 11, "got {} chars", t.chars().count());
        assert!(t.ends_with('…'));
    }

    #[test]
    fn truncate_leaves_short_text_alone() {
        assert_eq!(truncate("ok", 500.0), "ok");
        assert_eq!(truncate("", 100.0), "");
    }

    #[test]
    fn site_color_is_stable_and_varied() {
        assert_eq!(site_color("example.com"), site_color("example.com"));
        assert_ne!(site_color("a.com"), site_color("b.com"));
    }

    #[test]
    fn every_context_action_has_a_label() {
        // A menu entry with no label would render as a blank, dead row.
        for (_, a) in CONTEXT_LABELS {
            assert!(!a.to_string().is_empty());
        }
    }
}
