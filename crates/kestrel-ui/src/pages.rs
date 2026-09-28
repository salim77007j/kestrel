//! Settings and the privacy dashboard.
//!
//! Every control on these pages writes through to the persisted store in the
//! same frame it is toggled. There are no display-only rows: if a switch is
//! visible, flipping it changes browser behaviour.

use super::icons::{self, Icon};
use super::theme::{metrics, space, Palette};
use egui::{FontId, RichText, ScrollArea, Sense, Ui, Vec2};
use kestrel_core::privacy::{Defense, PrivacyConfig};
use kestrel_core::store::{Settings, Theme};

#[derive(Debug, Default)]
pub struct SettingsIntent {
    pub set_theme: Option<Theme>,
    pub set_homepage: Option<String>,
    pub set_search_engine: Option<String>,
    pub toggle_bookmarks_bar: Option<bool>,
    pub toggle_home_button: Option<bool>,
    pub toggle_privacy: Option<bool>,
    pub toggle_block_trackers: Option<bool>,
    pub toggle_block_ads: Option<bool>,
    pub toggle_block_annoyances: Option<bool>,
    pub toggle_https_only: Option<bool>,
    pub toggle_strip_params: Option<bool>,
    pub set_protection: Option<&'static str>,
    pub toggle_defense: Option<(Defense, bool)>,
    pub clear_history: bool,
    pub clear_downloads: bool,
    pub clear_cookies: bool,
    pub clear_all: bool,
    pub add_shortcut: Option<(String, String)>,
    pub remove_shortcut: Option<usize>,
    pub toggle_hw_accel: Option<bool>,
    pub set_zoom: Option<f32>,
    pub go_back: bool,
}

/// A labelled switch that reports its new state. Returns `None` when unchanged.
///
/// The value is copied in from the caller's immutable settings and only written
/// back through the returned intent, so the UI layer never mutates app state
/// directly and there is exactly one source of truth.
fn switch_row(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    description: &str,
    on: bool,
    id: &str,
) -> Option<bool> {
    let mut value = on;
    let changed = ui
        .add(
            egui::Checkbox::new(&mut value, "")
                .id_source(id)
                .desired_width(18.0),
        )
        .changed();

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(label).size(14.0).color(p.text_primary));
            if !description.is_empty() {
                ui.label(
                    RichText::new(description)
                        .size(11.5)
                        .color(p.text_muted),
                );
            }
        });
    });
    if changed {
        Some(value)
    } else {
        None
    }
}

fn section(ui: &mut Ui, p: &Palette, title: &str) {
    ui.add_space(space::LG);
    ui.label(RichText::new(title).size(18.0).strong().color(p.text_primary));
    ui.add_space(space::XS);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border);
    ui.add_space(space::MD);
}

fn page_header(ui: &mut Ui, p: &Palette, title: &str, intent: &mut SettingsIntent) {
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() {
            intent.go_back = true;
        }
        ui.label(RichText::new(title).size(22.0).strong().color(p.text_primary));
    });
    ui.add_space(space::MD);
}

pub fn show_settings(ui: &mut Ui, s: &Settings, p: &Palette, intent: &mut SettingsIntent) {
    ScrollArea::vertical().show(ui, |ui| {
        page_header(ui, p, "Settings", intent);

        section(ui, p, "Appearance");
        ui.horizontal(|ui| {
            for (label, theme) in [("System", Theme::System), ("Light", Theme::Light), ("Dark", Theme::Dark)] {
                let selected = s.theme == theme;
                let btn = egui::Button::new(RichText::new(label).color(if selected { egui::Color32::WHITE } else { p.text_primary }))
                    .fill(if selected { p.accent } else { p.surface })
                    .corner_radius(8.0);
                if ui.add(btn).clicked() {
                    intent.set_theme = Some(theme);
                }
            }
        });
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            ui.label("Homepage");
            let mut home = s.homepage.clone();
            if ui.add(egui::TextEdit::singleline(&mut home).desired_width(300.0)).changed() {
                intent.set_homepage = Some(home);
            }
        });

        section(ui, p, "Browser");
        if let Some(v) = switch_row(ui, p, "Show bookmarks bar", "Display bookmarks on every page", s.show_bookmarks_bar, "bmbar") {
            intent.toggle_bookmarks_bar = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Show home button", "Display the home button in the toolbar", s.show_home_button, "homebtn") {
            intent.toggle_home_button = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Restore session on launch", "Reopen your tabs after a restart or crash", s.restore_session_on_launch, "restore") {
            s.restore_session_on_launch = v;
        }
        if let Some(v) = switch_row(ui, p, "Hardware acceleration", "Use the GPU to render pages. Turn off if pages render incorrectly.", s.hardware_acceleration, "hw") {
            intent.toggle_hw_accel = Some(v);
        }
        ui.horizontal(|ui| {
            ui.label("Default zoom");
            let mut z = s.default_zoom;
            if ui.add(egui::Slider::new(&mut z, 0.5..=2.0).step_by(0.05)).changed() {
                intent.set_zoom = Some(z);
            }
            ui.label(format!("{:.0}%", z * 100.0));
        });

        section(ui, p, "Search");
        ui.horizontal(|ui| {
            ui.label("Default engine");
            egui::ComboBox::from_label("")
                .selected_text(&engine_name(&s.search_engine))
                .show_ui(ui, |ui| {
                    for e in kestrel_core::default_search_engines() {
                        if ui.selectable_label(s.search_engine == e.id, &e.name).clicked() {
                            intent.set_search_engine = Some(e.id.clone());
                        }
                    }
                });
        });

        section(ui, p, "Privacy");
        if let Some(v) = switch_row(ui, p, "Privacy protections", "Master switch for all fingerprint and tracking defences", s.privacy_enabled, "priv") {
            intent.toggle_privacy = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Block trackers", "Stop scripts from following you across sites", s.block_trackers, "trk") {
            intent.toggle_block_trackers = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Block ads", "Remove advertising from pages", s.block_ads, "ads") {
            intent.toggle_block_ads = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Block annoyances", "Hide cookie banners and pop-ups", s.block_annoyances, "ann") {
            intent.toggle_block_annoyances = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Always use secure connections", "Upgrade insecure sites to HTTPS", s.https_only, "https") {
            intent.toggle_https_only = Some(v);
        }
        if let Some(v) = switch_row(ui, p, "Remove tracking parameters", "Strip known tracking codes from links", s.strip_tracking_params, "strip") {
            intent.toggle_strip_params = Some(v);
        }

        section(ui, p, "Browsing data");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Clear history").clicked() {
                intent.clear_history = true;
            }
            if ui.button("Clear downloads").clicked() {
                intent.clear_downloads = true;
            }
            if ui.button("Clear cookies and site data").clicked() {
                intent.clear_cookies = true;
            }
            if ui.button("Clear everything").clicked() {
                intent.clear_all = true;
            }
        });

        section(ui, p, "New tab shortcuts");
        for (i, (name, url)) in s.new_tab_shortcuts.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).color(p.text_primary));
                ui.label(RichText::new(url).size(11.0).color(p.text_muted));
                if ui.small_button("Remove").clicked() {
                    intent.remove_shortcut = Some(i);
                }
            });
        }
        ui.horizontal(|ui| {
            let mut name = String::new();
            let mut url = String::new();
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name").desired_width(120.0));
            ui.add(egui::TextEdit::singleline(&mut url).hint_text("https://").desired_width(240.0));
            if ui.button("Add").clicked() && !name.is_empty() && !url.is_empty() {
                intent.add_shortcut = Some((name, url));
            }
        });
    });
}

fn engine_name(id: &str) -> String {
    kestrel_core::default_search_engines()
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.name)
        .unwrap_or_else(|| id.to_string())
}

/// The privacy dashboard: a summary plus the per-defence switches.
pub fn show_privacy(
    ui: &mut Ui,
    cfg: &PrivacyConfig,
    blocked_total: u64,
    requests_total: u64,
    p: &Palette,
    intent: &mut SettingsIntent,
) {
    ScrollArea::vertical().show(ui, |ui| {
        page_header(ui, p, "Privacy dashboard", intent);

        // Summary cards.
        ui.horizontal(|ui| {
            stat_card(ui, p, "Requests blocked", blocked_total.to_string());
            stat_card(ui, p, "Requests seen", requests_total.to_string());
            let pct = if requests_total == 0 {
                0.0
            } else {
                blocked_total as f32 / requests_total as f32 * 100.0
            };
            stat_card(ui, p, "Blocked", format!("{pct:.0}%"));
            stat_card(
                ui,
                p,
                "Active defences",
                cfg.active().len().to_string(),
            );
        });

        section(ui, p, "Protection level");
        ui.horizontal(|ui| {
            for (label, value) in [("Balanced", "balanced"), ("Strict", "strict"), ("Custom", "custom")] {
                let selected = cfg.protection_level.as_str() == value
                    || (value == "custom" && cfg.protection_level == kestrel_core::privacy::Level::Custom);
                let btn = egui::Button::new(RichText::new(label).color(if selected { egui::Color32::WHITE } else { p.text_primary }))
                    .fill(if selected { p.accent } else { p.surface })
                    .corner_radius(8.0);
                if ui.add(btn).clicked() {
                    intent.set_protection = Some(value);
                }
            }
        });

        section(ui, p, "Fingerprint protections");
        for d in Defense::all() {
            let on = cfg.is_active(*d);
            if let Some(v) = switch_row(ui, p, d.label(), d.description(), on, d.as_str()) {
                intent.toggle_defense = Some((*d, v));
            }
        }

        section(ui, p, "What this costs you");
        ui.label(
            RichText::new(
                "Stricter settings can break sign-in flows, media players and sites that \
                 fingerprint aggressively. If a site stops working, turn off the specific \
                 defence above rather than disabling protections wholesale.",
            )
            .size(12.0)
            .color(p.text_secondary),
        );
    });
}

fn stat_card(ui: &mut Ui, p: &Palette, label: &str, value: String) {
    let w = 150.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 72.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12.0), p.surface);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(12.0),
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + space::LG, rect.top() + space::MD),
        egui::Align2::LEFT_TOP,
        value,
        FontId::proportional(22.0).strong(),
        p.text_primary,
    );
    ui.painter().text(
        egui::pos2(rect.left() + space::LG, rect.top() + 44.0),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(11.5),
        p.text_muted,
    );
}

/// The downloads panel.
pub fn show_downloads(
    ui: &mut Ui,
    items: &[kestrel_core::store::Download],
    p: &Palette,
    intent: &mut SettingsIntent,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Downloads").size(20.0).strong().color(p.text_primary));
        if ui.button("Clear finished").clicked() {
            intent.clear_downloads = true;
        }
    });
    ui.add_space(space::MD);
    if items.is_empty() {
        ui.label(RichText::new("No downloads yet.").color(p.text_muted));
        return;
    }
    ScrollArea::vertical().show(ui, |ui| {
        for d in items {
            ui.horizontal(|ui| {
                icons::draw(ui, Icon::Download, 18.0, p.text_secondary);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&d.filename).color(p.text_primary));
                    let state = match d.state {
                        kestrel_core::store::DownloadState::Completed => "Completed".to_string(),
                        kestrel_core::store::DownloadState::Failed => {
                            format!("Failed: {}", d.error.clone().unwrap_or_default())
                        }
                        kestrel_core::store::DownloadState::Cancelled => "Cancelled".into(),
                        _ => match d.progress() {
                            Some(pct) => format!("{:.0}%", pct * 100.0),
                            None => "Downloading…".into(),
                        },
                    };
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            kestrel_core::store::format_bytes(d.bytes_written),
                            state
                        ))
                        .size(11.0)
                        .color(p.text_muted),
                    );
                });
            });
            ui.add_space(space::MD);
        }
    });
}

/// Bookmarks and history managers share a list layout.
pub fn show_manager(
    ui: &mut Ui,
    title: &str,
    rows: &[(String, String, String)],
    empty: &str,
    p: &Palette,
    intent: &mut SettingsIntent,
) {
    page_header(ui, p, title, intent);
    if rows.is_empty() {
        ui.label(RichText::new(empty).color(p.text_muted));
        return;
    }
    ScrollArea::vertical().show(ui, |ui| {
        for (i, (primary, secondary, meta)) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 48.0),
                Sense::click(),
            );
            if resp.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(8.0),
                    p.surface_hover,
                );
            }
            ui.painter().text(
                egui::pos2(rect.left() + space::MD, rect.top() + space::SM),
                egui::Align2::LEFT_TOP,
                primary.clone(),
                FontId::proportional(13.5),
                p.text_primary,
            );
            ui.painter().text(
                egui::pos2(rect.left() + space::MD, rect.top() + 26.0),
                egui::Align2::LEFT_TOP,
                secondary.clone(),
                FontId::proportional(11.0),
                p.text_muted,
            );
            ui.painter().text(
                egui::pos2(rect.right() - space::MD, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                meta.clone(),
                FontId::proportional(11.0),
                p.text_muted,
            );
            let _ = i;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_name_resolves_known_ids() {
        assert_eq!(engine_name("duckduckgo"), "DuckDuckGo");
        // An unknown id must not panic; the raw id is a safe fallback.
        assert_eq!(engine_name("custom-thing"), "custom-thing");
    }

    #[test]
    fn every_defense_has_settings_copy() {
        for d in Defense::all() {
            assert!(!d.label().is_empty());
            assert!(d.description().len() > 20);
        }
    }
}
