//! The navigation toolbar: back/forward/reload, the omnibox, and the
//! bookmark/profile/menu controls on the right.
//!
//! The omnibox is the highest-traffic control in the browser, so it is built
//! around a single `TextEdit` with its own focus handling. Suggestions are
//! computed per keystroke from history, bookmarks and the default engine, and
//! keyboard navigation is handled here so the main loop never has to.

use super::icons::{self, Icon};
use super::theme::{metrics, security_color, space, Palette};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, TextEdit, Ui, Vec2};
use kestrel_core::omnibox::{self, SearchEngine};
use kestrel_core::store::HistoryEntry;

/// One row in the suggestion dropdown.
#[derive(Debug, Clone, PartialEq)]
pub enum Suggestion {
    /// A URL the user has visited.
    History { title: String, url: String },
    /// A saved bookmark.
    Bookmark { title: String, url: String },
    /// Turn the typed text into a search on the default engine.
    Search { text: String, engine: String },
}

#[derive(Debug, Default)]
pub struct ToolbarIntent {
    pub back: bool,
    pub forward: bool,
    pub reload: bool,
    pub stop: bool,
    pub home: bool,
    pub omnibox_submitted: Option<String>,
    pub omnibox_focused: bool,
    pub toggle_bookmark: bool,
    pub open_menu: bool,
    pub open_profile: bool,
    pub open_privacy: bool,
    pub open_downloads: bool,
    pub suggestion_chosen: Option<Suggestion>,
    /// Index of the highlighted suggestion, if the dropdown is open.
    pub selected_suggestion: Option<usize>,
}

/// Keyboard state for the dropdown, kept across frames by the caller.
#[derive(Debug, Default)]
pub struct OmniboxState {
    pub open: bool,
    pub selected: usize,
    /// Set when the user edits the field, so we can decide whether to reopen.
    pub editing: bool,
}

/// How the omnibox should present the current page.
#[derive(Debug, Clone, PartialEq)]
pub enum OmniboxMode {
    /// Showing a URL; show the security indicator.
    Url { secure: bool, mixed: bool },
    /// Showing typed text; show a search affordance.
    Editing,
}

pub struct ToolbarResult {
    pub intent: ToolbarIntent,
}

/// Everything the toolbar needs to render. Passing a snapshot rather than
/// borrowing the whole app keeps this module independent of app state.
pub struct ToolbarData<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    pub bookmarked: bool,
    pub show_home: bool,
    pub suggestions: Vec<Suggestion>,
    pub focused: bool,
    pub text: String,
}

pub fn show(
    ui: &mut Ui,
    d: &ToolbarData,
    state: &mut OmniboxState,
    p: &Palette,
    engine: &SearchEngine,
) -> ToolbarResult {
    let mut intent = ToolbarIntent::default();
    ui.add_space(space::SM);
    ui.horizontal_centered(|ui| {
        // --- Navigation cluster ---
        nav_button(ui, Icon::Back, d.can_back, p, "Back (Alt+Left)", |r| {
            if r.clicked() {
                intent.back = true;
            }
        });
        nav_button(ui, Icon::Forward, d.can_forward, p, "Forward (Alt+Right)", |r| {
            if r.clicked() {
                intent.forward = true;
            }
        });
        nav_button(
            ui,
            if d.loading { Icon::Stop } else { Icon::Reload },
            true,
            p,
            if d.loading { "Stop (Esc)" } else { "Reload (Ctrl+R)" },
            |r| {
                if r.clicked() {
                    if d.loading {
                        intent.stop = true;
                    } else {
                        intent.reload = true;
                    }
                }
            },
        );
        if d.show_home {
            nav_button(ui, Icon::Home, true, p, "Home", |r| {
                if r.clicked() {
                    intent.home = true;
                }
            });
        }

        ui.add_space(space::SM);

        // --- Omnibox ---
        omnibox_area(ui, d, state, p, engine, &mut intent);

        ui.add_space(space::SM);

        // --- Right cluster ---
        nav_button(
            ui,
            if d.bookmarked { Icon::StarFilled } else { Icon::Star },
            true,
            p,
            if d.bookmarked { "Remove bookmark" } else { "Bookmark this tab" },
            |r| {
                if r.clicked() {
                    intent.toggle_bookmark = true;
                }
            },
        );
        nav_button(ui, Icon::Privacy, true, p, "Privacy dashboard", |r| {
            if r.clicked() {
                intent.open_privacy = true;
            }
        });
        nav_button(ui, Icon::Download, true, p, "Downloads", |r| {
            if r.clicked() {
                intent.open_downloads = true;
            }
        });
        nav_button(ui, Icon::Globe, true, p, "Profile", |r| {
            if r.clicked() {
                intent.open_profile = true;
            }
        });
        nav_button(ui, Icon::Menu, true, p, "Menu", |r| {
            if r.clicked() {
                intent.open_menu = true;
            }
        });
    });
    ToolbarResult { intent }
}

fn nav_button(
    ui: &mut Ui,
    icon: Icon,
    enabled: bool,
    p: &Palette,
    tooltip: &str,
    on_click: impl FnOnce(egui::Response),
) {
    // A plain `allocate_exact_size` with hover painting keeps the icon vector
    // and avoids the layout cost of a real Button widget on a hot path.
    let (rect, response) = ui.allocate_exact_size(
        Vec2::splat(metrics::ICON_BUTTON),
        Sense::click().enabled(enabled),
    );
    if response.hovered() && enabled {
        ui.painter()
            .circle_filled(rect.center(), metrics::ICON_BUTTON * 0.42, p.surface_hover);
    }
    let color = if enabled {
        if response.hovered() { p.text_primary } else { p.text_secondary }
    } else {
        p.text_muted
    };
    icons::draw(ui, icon, 18.0, color);
    response.on_hover_text(tooltip);
    on_click(response);
}

/// Draw the omnibox and, when focused with text, the suggestion list.
fn omnibox_area(
    ui: &mut Ui,
    d: &ToolbarData,
    state: &mut OmniboxState,
    p: &Palette,
    engine: &SearchEngine,
    intent: &mut ToolbarIntent,
) {
    let full = ui.available_width();
    let rect = Rect::from_min_size(
        ui.cursor().left_top(),
        Vec2::new(full, metrics::OMNIBOX_HEIGHT),
    );

    let resp = ui.interact(
        rect,
        egui::Id::new("omnibox"),
        Sense::click(),
    );
    if resp.clicked() {
        ui.memory_mut(|m| m.request_focus("omnibox-edit"));
        state.open = true;
    }
    let focused = ui.memory(|m| m.has_focus("omnibox-edit"));
    state.open = focused;

    let bg = if focused { p.omnibox_focus } else { p.omnibox_bg };
    let radius = egui::CornerRadius::same(metrics::OMNIBOX_RADIUS);
    ui.painter()
        .rect_filled(rect, radius, bg);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );

    // Security indicator on the left, search icon when editing.
    let icon_size = 16.0;
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + space::XL, rect.center().y),
        Vec2::splat(icon_size),
    );
    if focused {
        icons::draw(ui, Icon::Search, icon_size, p.text_muted);
    } else {
        let secure = d.url.starts_with("https://") || d.url.starts_with("about:");
        let mixed = d.url.starts_with("https://") && d.url.contains("ads");
        let c = security_color(p, secure, mixed);
        if secure {
            icons::draw(ui, Icon::Shield, icon_size, c);
        } else {
            icons::draw(ui, Icon::Lock, icon_size, c);
        }
    }

    // The text field, inset past the icon.
    let field_x = icon_rect.right() + space::MD;
    let field_w = rect.width() - (field_x - rect.left()) - space::XXL;
    let field_rect = Rect::from_min_size(
        egui::pos2(field_x, rect.center().y - metrics::OMNIBOX_HEIGHT * 0.32),
        Vec2::new(field_w.max(40.0), metrics::OMNIBOX_HEIGHT * 0.64),
    );

    let mut text = d.text.clone();
    let edit = TextEdit::singleline(&mut text)
        .id("omnibox-edit")
        .desired_width(field_w)
        .font(FontId::proportional(14.0))
        .text_color(p.text_primary)
        .frame(false)
        .vertical_alignment(egui::Align::Center);
    let er = ui.scope_builder(egui::UiBuilder::new().max_rect(field_rect), |ui| {
        ui.add(edit).on_hover_text("Search or enter address")
    });
    if er.changed() {
        state.editing = true;
        state.open = true;
        state.selected = 0;
    }
    // Keep the caller's text in sync through the intent on the next frame.
    if er.changed() {
        intent.omnibox_focused = true;
    }
    if ui.memory(|m| m.has_focus("omnibox-edit")) {
        intent.omnibox_focused = true;
        // Enter commits, Escape reverts to the page URL.
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let target = if text.trim().is_empty() {
                d.url.to_string()
            } else {
                match omnibox::resolve_input(&text, engine) {
                    omnibox::NavigationTarget::Url(u) | omnibox::NavigationTarget::Inferred(u) => {
                        u.to_string()
                    }
                    omnibox::NavigationTarget::Search(q) => engine
                        .build_url(&q.text)
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| text.clone()),
                }
            };
            intent.omnibox_submitted = Some(target);
            ui.memory_mut(|m| m.surrender_focus("omnibox-edit"));
            state.open = false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.memory_mut(|m| m.surrender_focus("omnibox-edit"));
            state.open = false;
        }
    }

    // --- Suggestions ---
    if state.open && !d.suggestions.is_empty() {
        intent.selected_suggestion = dropdown(ui, rect, &d.suggestions, p, state.selected, engine);
    }
    ui.allocate_space(Vec2::new(rect.width(), 0.0));
}

/// The suggestion popup. Returns the chosen index, if any.
fn dropdown(
    ui: &mut Ui,
    anchor: Rect,
    items: &[Suggestion],
    p: &Palette,
    selected: usize,
    engine: &SearchEngine,
) -> Option<usize> {
    let row_h = 40.0;
    let h = row_h * items.len() as f32 + space::SM;
    let rect = Rect::from_min_size(
        egui::pos2(anchor.left(), anchor.bottom() + space::XS),
        Vec2::new(anchor.width(), h),
    );
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(12.0),
        p.surface_raised,
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(12.0),
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    // Soft drop shadow so the popup reads as floating above the page.
    painter.rect_filled(
        rect.translate(Vec2::new(0.0, 2.0)),
        egui::CornerRadius::same(12.0),
        p.shadow,
    );

    let mut chosen = None;
    for (i, s) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + space::SM + row_h * i as f32),
            Vec2::new(rect.width(), row_h),
        );
        let response = ui.interact(row, egui::Id::new(("sugg", i)), Sense::click());
        let is_sel = i == selected;
        if response.hovered() || is_sel {
            ui.painter().rect_filled(
                Rect::from_min_size(row.min, Vec2::new(row.width(), row.height())),
                egui::CornerRadius::same(8.0),
                if is_sel { p.surface_hover } else { p.omnibox_bg },
            );
        }
        let icon = match s {
            Suggestion::Search { .. } => Icon::Search,
            Suggestion::Bookmark { .. } => Icon::StarFilled,
            Suggestion::History { .. } => Icon::Clock,
        };
        icons::draw(ui, icon, 16.0, p.text_muted);
        let (title, sub) = match s {
            Suggestion::History { title, url } | Suggestion::Bookmark { title, url } => {
                // Fall back to the raw string if it will not parse, rather than
                // substituting a placeholder that would mislead the user.
                let display = url::Url::parse(url)
                    .map(|u| omnibox::pretty_url(&u))
                    .unwrap_or_else(|_| url.clone());
                (title.clone(), display)
            }
            Suggestion::Search { text, .. } => {
                (format!("Search for \"{text}\""), engine.name.clone())
            }
        };
        ui.painter().text(
            egui::pos2(row.left() + 44.0, row.center().y - 8.0),
            egui::Align2::LEFT_TOP,
            title,
            FontId::proportional(13.5),
            p.text_primary,
        );
        ui.painter().text(
            egui::pos2(row.left() + 44.0, row.center().y + 4.0),
            egui::Align2::LEFT_TOP,
            sub,
            FontId::proportional(11.5),
            p.text_muted,
        );
        if response.clicked() {
            chosen = Some(i);
        }
    }
    // Reserve the popup's height so following panels are not overlapped.
    ui.allocate_space(Vec2::new(rect.width(), h));
    chosen
}

/// Build the suggestion list for the current text.
pub fn build_suggestions(
    text: &str,
    history: &[HistoryEntry],
    bookmarks: &[(String, String)],
    engine: &SearchEngine,
) -> Vec<Suggestion> {
    let q = text.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let lq = q.to_lowercase();
    let mut out: Vec<Suggestion> = Vec::new();

    // Bookmarks first: an explicit save is a stronger signal of intent than a
    // past visit.
    for (title, url) in bookmarks {
        if title.to_lowercase().contains(&lq) || url.to_lowercase().contains(&lq) {
            out.push(Suggestion::Bookmark {
                title: title.clone(),
                url: url.clone(),
            });
        }
    }
    for h in history {
        if h.title.to_lowercase().contains(&lq) || h.url.to_lowercase().contains(&lq) {
            out.push(Suggestion::History {
                title: if h.title.is_empty() { h.url.clone() } else { h.title.clone() },
                url: h.url.clone(),
            });
        }
    }
    out.sort_by_key(|s| match s {
        Suggestion::Bookmark { .. } => 0,
        Suggestion::History { .. } => 1,
        Suggestion::Search { .. } => 2,
    });
    out.truncate(6);
    // Always offer the search fallback last.
    out.push(Suggestion::Search {
        text: q.to_string(),
        engine: engine.name.clone(),
    });
    out
}

/// The URL a chosen suggestion navigates to.
pub fn suggestion_target(s: &Suggestion, engine: &SearchEngine) -> String {
    match s {
        Suggestion::History { url, .. } | Suggestion::Bookmark { url, .. } => url.clone(),
        Suggestion::Search { text, .. } => engine
            .build_url(text)
            .map(|u| u.to_string())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> SearchEngine {
        omnibox::default_search_engines().into_iter().next().unwrap()
    }

    fn hist(url: &str, title: &str) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            url: url.into(),
            title: title.into(),
            visits: vec![0],
        }
    }

    #[test]
    fn empty_input_gives_no_suggestions() {
        let s = build_suggestions("  ", &[], &[], &engine());
        assert!(s.is_empty());
    }

    #[test]
    fn history_matches_title_and_url() {
        let h = vec![hist("https://example.com/docs", "Docs")];
        let s = build_suggestions("docs", &h, &[], &engine());
        assert!(matches!(s[0], Suggestion::History { .. }));
    }

    #[test]
    fn bookmarks_outrank_history() {
        let h = vec![hist("https://a.com/x", "Shared")];
        let b = vec![("Shared doc".to_string(), "https://a.com/x".to_string())];
        let s = build_suggestions("shared", &h, &b, &engine());
        assert!(matches!(s[0], Suggestion::Bookmark { .. }));
    }

    #[test]
    fn search_fallback_is_always_offered() {
        let s = build_suggestions("some query", &[], &[], &engine());
        assert!(matches!(s.last().unwrap(), Suggestion::Search { .. }));
    }

    #[test]
    fn suggestion_resolves_to_a_url() {
        let s = Suggestion::Search { text: "cats".into(), engine: "DuckDuckGo".into() };
        let t = suggestion_target(&s, &engine());
        assert!(t.starts_with("https://duckduckgo.com/"));
        assert!(t.contains("cats"));
    }

    #[test]
    fn history_suggestion_returns_its_own_url() {
        let s = Suggestion::History { title: "t".into(), url: "https://x.com/".into() };
        assert_eq!(suggestion_target(&s, &engine()), "https://x.com/");
    }

    #[test]
    fn suggestions_are_capped() {
        let h: Vec<HistoryEntry> = (0..50)
            .map(|i| hist(&format!("https://site{i}.com/q"), "query"))
            .collect();
        let s = build_suggestions("query", &h, &[], &engine());
        assert!(s.len() <= 7, "got {}", s.len());
    }
}
