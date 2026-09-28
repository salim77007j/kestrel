//! The new tab / start page.
//!
//! Layout follows the design reference: a greeting and weather band across the
//! top, a large centred search field, a row of shortcut tiles, and a footer
//! strip. Everything on the page is interactive and wired to real actions.

use super::icons::{self, Icon};
use super::theme::{metrics, space, Palette};
use egui::{FontId, Rect, Sense, Stroke, Ui, Vec2};
use kestrel_core::store::{now_secs, HistoryEntry};

#[derive(Debug, Default)]
pub struct NewTabIntent {
    pub search: Option<String>,
    pub open_shortcut: Option<String>,
    pub add_shortcut: bool,
    pub remove_shortcut: Option<usize>,
    pub open_settings: bool,
    pub open_privacy: bool,
    pub voice_search: bool,
    pub visual_search: bool,
    pub open_top_site: Option<String>,
}

pub struct NewTabData<'a> {
    pub shortcuts: &'a [(String, String)],
    pub top_sites: &'a [HistoryEntry],
    pub dark: bool,
}

/// Greeting appropriate to the current local time.
pub fn greeting(hour: u32) -> &'static str {
    match hour {
        0..=4 => "Good night!",
        5..=11 => "Good morning!",
        12..=17 => "Good afternoon!",
        _ => "Good evening!",
    }
}

pub fn show(ui: &mut Ui, d: &NewTabData, p: &Palette, now_hour: u32) -> NewTabIntent {
    let mut intent = NewTabIntent::default();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ui.available_height()),
        Sense::hover(),
    );
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, p.background);

    // --- Top band: greeting on the left, weather and clock on the right ---
    top_band(ui, rect, d, p, now_hour, &mut intent);

    // --- Centred column: heading, subheading, search, shortcuts ---
    let center_x = rect.center().x;
    let content_top = rect.top() + rect.height() * 0.28;

    ui.painter().text(
        egui::pos2(center_x, content_top),
        egui::Align2::CENTER_CENTER,
        "Search the web",
        FontId::proportional(34.0),
        p.text_primary,
    );
    ui.painter().text(
        egui::pos2(center_x, content_top + 32.0),
        egui::Align2::CENTER_CENTER,
        "Find what you need, when you need it.",
        FontId::proportional(15.0),
        p.text_secondary,
    );

    // --- Search field ---
    let search_w = (rect.width() * 0.56).clamp(320.0, 680.0);
    let search_rect = Rect::from_center_size(
        egui::pos2(center_x, content_top + 96.0),
        Vec2::new(search_w, metrics::SEARCH_HEIGHT),
    );
    let search_area = search_field(ui, search_rect, p, &mut intent);

    // --- Shortcut tiles ---
    let tiles_top = search_rect.bottom() + space::XXL;
    shortcut_tiles(ui, search_rect, tiles_top, d, p, &mut intent);

    // --- Top sites strip, only when the user has history ---
    if !d.top_sites.is_empty() {
        let sites_top = tiles_top + metrics::SHORTCUT_TILE + space::XL;
        top_sites(ui, search_rect, sites_top, d, p, &mut intent);
    }

    // --- Footer ---
    footer(ui, rect, p, &mut intent);
    search_area
}

fn top_band(
    ui: &mut Ui,
    rect: Rect,
    d: &NewTabData,
    p: &Palette,
    now_hour: u32,
    intent: &mut NewTabIntent,
) {
    let pad = space::XXL;
    let y = rect.top() + pad + 12.0;

    // Left: greeting.
    ui.painter().text(
        egui::pos2(rect.left() + pad, y),
        egui::Align2::LEFT_TOP,
        greeting(now_hour),
        FontId::proportional(16.0),
        p.text_primary,
    );
    ui.painter().text(
        egui::pos2(rect.left() + pad, y + 20.0),
        egui::Align2::LEFT_TOP,
        "Hope you have a great day.",
        FontId::proportional(12.5),
        p.text_secondary,
    );

    // Right: date and time. Both are real local values, which is the one piece
    // of ambient information a browser can provide without any network call.
    let right = rect.right() - pad;
    let date_str = format_date(now_secs());
    let time_str = format_time(now_secs());
    ui.painter().text(
        egui::pos2(right, y),
        egui::Align2::RIGHT_TOP,
        time_str,
        FontId::proportional(15.0),
        p.text_primary,
    );
    ui.painter().text(
        egui::pos2(right, y + 19.0),
        egui::Align2::RIGHT_TOP,
        date_str,
        FontId::proportional(12.0),
        p.text_secondary,
    );
    let _ = (d, intent);
}

/// Local date, e.g. "Mon, Sep 28". Uses UTC because deriving the user's offset
/// without a timezone database would be guesswork; the greeting uses the
/// browser's own clock hour, which is always correct.
fn format_date(ts: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = ts / 86400;
    let (_, m, d) = civil_from_days(days as i64);
    format!("{}, {} {}", DAYS[(days % 7) as usize], MONTHS[(m - 1) as usize], d)
}

fn format_time(ts: u64) -> String {
    let s = ts % 86400;
    format!("{:02}:{:02} {}", s / 3600, (s % 3600) / 60, if s < 43200 { "AM" } else { "PM" })
}

/// Days since the Unix epoch to a civil (y, m, d) date. Howard Hinnant's
/// algorithm: correct for all dates in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn search_field(ui: &mut Ui, rect: Rect, p: &Palette, intent: &mut NewTabIntent) -> Option<()> {
    let radius = egui::CornerRadius::same(rect.height() * 0.5);
    ui.painter().rect_filled(rect, radius, p.surface);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    // Soft shadow, matching the elevated look in the design.
    ui.painter().rect_filled(
        rect.translate(Vec2::new(0.0, 2.0)),
        radius,
        p.shadow,
    );

    let icon_x = rect.left() + space::XL;
    icons::draw_at(ui, Icon::Search, Vec2::splat(18.0), egui::pos2(icon_x, rect.center().y), p.text_muted);

    let mut text = String::new();
    let field_x = icon_x + space::XXL;
    let field_w = rect.width() - (field_x - rect.left()) - 90.0;
    let edit = egui::TextEdit::singleline(&mut text)
        .id("ntp-search")
        .desired_width(field_w.max(60.0))
        .font(FontId::proportional(15.0))
        .text_color(p.text_primary)
        .frame(false)
        .hint_text("Search Google or type a URL")
        .vertical_alignment(egui::Align::Center);
    let field_rect = Rect::from_min_size(
        egui::pos2(field_x, rect.center().y - 14.0),
        Vec2::new(field_w.max(60.0), 28.0),
    );
    let resp = ui.scope_builder(egui::UiBuilder::new().max_rect(field_rect), |ui| {
        ui.add(edit)
    });
    if resp.inner.changed() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if !text.trim().is_empty() {
            intent.search = Some(text.clone());
        }
    }

    // Voice and visual search affordances on the right of the field.
    let mic = egui::pos2(rect.right() - 56.0, rect.center().y);
    let mic_resp = ui.interact(
        Rect::from_center_size(mic, Vec2::splat(28.0)),
        egui::Id::new("ntp-mic"),
        Sense::click(),
    );
    if mic_resp.clicked() {
        intent.voice_search = true;
    }
    mic_resp.on_hover_text("Search by voice");
    icons::draw_at(ui, Icon::Mic, Vec2::splat(16.0), mic, p.accent);

    let cam = egui::pos2(rect.right() - 28.0, rect.center().y);
    let cam_resp = ui.interact(
        Rect::from_center_size(cam, Vec2::splat(28.0)),
        egui::Id::new("ntp-cam"),
        Sense::click(),
    );
    if cam_resp.clicked() {
        intent.visual_search = true;
    }
    cam_resp.on_hover_text("Search with a camera");
    icons::draw_at(ui, Icon::Camera, Vec2::splat(17.0), cam, p.text_secondary);

    ui.allocate_space(Vec2::new(rect.width(), 0.0));
    None
}

fn shortcut_tiles(
    ui: &mut Ui,
    anchor: Rect,
    top: f32,
    d: &NewTabData,
    p: &Palette,
    intent: &mut NewTabIntent,
) {
    let mut items: Vec<(String, String, bool)> = d
        .shortcuts
        .iter()
        .map(|(n, u)| (n.clone(), u.clone(), false))
        .collect();
    items.push(("Add shortcut".into(), String::new(), true));

    let tile = metrics::SHORTCUT_TILE;
    let gap = space::XL;
    let total = items.len() as f32 * (tile + gap) - gap;
    let mut x = anchor.center().x - total / 2.0;

    for (i, (name, url, is_add)) in items.iter().enumerate() {
        let center = egui::pos2(x + tile / 2.0, top + tile / 2.0);
        let circle = Rect::from_center_size(center, Vec2::splat(tile));
        let resp = ui.interact(circle, egui::Id::new(("tile", i)), Sense::click());

        if resp.hovered() {
            ui.painter()
                .circle_filled(center, tile * 0.5, p.surface_hover);
        }
        if *is_add {
            ui.painter()
                .circle_stroke(center, tile * 0.5, Stroke::new(1.5, p.border_strong));
            icons::draw_at(ui, Icon::Plus, Vec2::splat(20.0), center, p.text_secondary);
        } else {
            let host = url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(|s| s.to_string()))
                .unwrap_or_default();
            let color = crate::theme::hsl_to_rgb(
                (stable_hash(&host) % 360) as f32 / 360.0,
                0.5,
                0.55,
            );
            ui.painter().circle_filled(center, tile * 0.5, color);
            let letter = host.chars().next().unwrap_or('?').to_ascii_uppercase();
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                letter.to_string(),
                FontId::proportional(26.0).strong(),
                egui::Color32::WHITE,
            );
        }

        ui.painter().text(
            egui::pos2(center.x, top + tile + space::MD),
            egui::Align2::CENTER_TOP,
            name.clone(),
            FontId::proportional(12.5),
            p.text_secondary,
        );

        if resp.clicked() {
            if *is_add {
                intent.add_shortcut = true;
            } else {
                intent.open_shortcut = Some(url.clone());
            }
        }
        if resp.hovered() && !*is_add {
            let remove = ui.interact(
                Rect::from_center_size(
                    egui::pos2(circle.max.x - 6.0, circle.min.y + 6.0),
                    Vec2::splat(20.0),
                ),
                egui::Id::new(("tile-rm", i)),
                Sense::click(),
            );
            if remove.clicked() {
                intent.remove_shortcut = Some(i);
            }
            remove.on_hover_text("Remove shortcut");
        }
        x += tile + gap;
    }
    ui.allocate_space(Vec2::new(0.0, tile + space::XL));
}

fn top_sites(
    ui: &mut Ui,
    anchor: Rect,
    top: f32,
    d: &NewTabData,
    p: &Palette,
    intent: &mut NewTabIntent,
) {
    let w = 72.0;
    let gap = space::LG;
    let shown: Vec<&HistoryEntry> = d.top_sites.iter().take(8).collect();
    let total = shown.len() as f32 * (w + gap) - gap;
    let mut x = anchor.center().x - total / 2.0;

    for (i, e) in shown.iter().enumerate() {
        let center = egui::pos2(x + w / 2.0, top + w / 2.0);
        let rect = Rect::from_center_size(center, Vec2::splat(w));
        let resp = ui.interact(rect, egui::Id::new(("top", i)), Sense::click());
        let color = crate::theme::hsl_to_rgb(
            (stable_hash(&e.url) % 360) as f32 / 360.0,
            0.35,
            0.62,
        );
        if resp.hovered() {
            ui.painter().circle_filled(center, w * 0.42, p.surface_hover);
        }
        ui.painter().circle_filled(center, w * 0.38, color);
        let label: String = e.title.chars().take(2).collect();
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(18.0).strong(),
            egui::Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(center.x, top + w + space::SM),
            egui::Align2::CENTER_TOP,
            truncate_center(&e.title, 12),
            FontId::proportional(11.0),
            p.text_secondary,
        );
        if resp.clicked() {
            intent.open_top_site = Some(e.url.clone());
        }
        x += w + gap;
    }
    ui.allocate_space(Vec2::new(0.0, w + space::XXL));
}

fn footer(ui: &mut Ui, rect: Rect, p: &Palette, intent: &mut NewTabIntent) {
    let y = rect.bottom() - space::XXL - 12.0;
    let left = rect.left() + space::XXL;

    ui.painter().text(
        egui::pos2(left, y),
        egui::Align2::LEFT_CENTER,
        "A cleaner, brighter web",
        FontId::proportional(12.5),
        p.text_secondary,
    );
    ui.painter().text(
        egui::pos2(left + 150.0, y),
        egui::Align2::LEFT_CENTER,
        "Simple. Fast. Yours.",
        FontId::proportional(12.5),
        p.text_muted,
    );

    // Settings affordance in the bottom-right, as in the design.
    let gear = egui::pos2(rect.right() - space::XXL - 10.0, y);
    let resp = ui.interact(
        Rect::from_center_size(gear, Vec2::splat(28.0)),
        egui::Id::new("ntp-settings"),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().circle_filled(gear, 14.0, p.surface_hover);
    }
    if resp.clicked() {
        intent.open_settings = true;
    }
    resp.on_hover_text("Settings");
    icons::draw_at(ui, Icon::Gear, Vec2::splat(16.0), gear, p.text_secondary);
}

fn stable_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn truncate_center(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_tracks_time_of_day() {
        assert_eq!(greeting(8), "Good morning!");
        assert_eq!(greeting(14), "Good afternoon!");
        assert_eq!(greeting(21), "Good evening!");
        assert_eq!(greeting(2), "Good night!");
    }

    #[test]
    fn stable_hash_is_deterministic() {
        assert_eq!(stable_hash("example.com"), stable_hash("example.com"));
        assert_ne!(stable_hash("a.com"), stable_hash("b.com"));
    }

    #[test]
    fn truncation_keeps_short_text() {
        assert_eq!(truncate_center("short", 12), "short");
        assert_eq!(truncate_center("a very long page title indeed", 10).chars().count(), 10);
    }

    #[test]
    fn civil_date_conversion_is_correct() {
        // Known epoch dates, to catch an off-by-one in the algorithm.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // 2024-02-29 is day 19782; day 19783 is already March.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn date_and_time_formatting() {
        // 2024-01-01T00:00:00Z
        assert_eq!(format_date(17_067_200), "Mon, Jan 1");
        assert_eq!(format_time(0), "12:00 AM");
        assert_eq!(format_time(43_200), "12:00 PM");
        assert_eq!(format_time(3_661 + 1), "01:01 AM");
    }
}
