//! The browser chrome: composes the tab strip, toolbar, bookmarks bar and the
//! main content area into one egui pass.
//!
//! This is the only place that knows the overall window layout. Each widget
//! returns an intent; this module turns those intents into actions on the app,
//! which keeps the "no inert controls" property auditable in one file.

use crate::app::App;
use kestrel_ui::{icons, new_tab, pages, tab_strip, theme, toolbar};
use kestrel_ui::tabs::Page;

/// Build one frame of chrome. Returns whether another frame should be
/// requested.
pub fn ui(
    app: &mut App,
    ctx: &egui::Context,
    raw: &egui::RawInput,
    out: &mut egui::FullOutput,
) -> bool {
    let palette = app.palette;
    theme::apply(ctx, &palette);

    let mut repaint = false;
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(palette.background))
        .show(ctx, |ui| {
            repaint |= tab_bar(ui, app, &palette);
            repaint |= bookmarks_bar(ui, app, &palette);
            content(ui, app, &palette);
        });

    // Menus are drawn in a second area so they float above the content.
    menus(ui_ctx_guard(ctx), app);

    let _ = raw;
    let _ = out;
    repaint
}

/// A tiny helper so the menu block can take a `&Context` without threading it.
fn ui_ctx_guard(ctx: &egui::Context) -> &egui::Context {
    ctx
}

/// Tab strip plus the omnibox toolbar.
fn tab_bar(ui: &mut egui::Ui, app: &mut App, p: &theme::Palette) -> bool {
    let mut repaint = false;
    ui.add_space(2.0);

    // --- Tabs ---
    let active_id = app.strip.active_tab().map(|t| t.id).unwrap_or(0);
    let mut new_hovered = false;
    let strip = tab_strip::show(
        ui,
        &app.strip.tabs,
        active_id,
        p,
        &mut new_hovered,
    );
    repaint |= handle_strip_intent(app, &strip.intent, p);

    // --- Toolbar ---
    let engine = app.search_engine();
    let Some(engine) = engine else { return repaint };
    let (url, title, can_back, can_forward, loading, bookmarked, show_home) = {
        let t = app.strip.active_tab();
        match t {
            Some(t) => (
                t.url.clone(),
                t.title.clone(),
                t.can_go_back(),
                t.can_go_forward(),
                t.state == kestrel_ui::tabs::TabState::Loading,
                app.is_bookmarked(&t.url),
                app.settings.show_home_button,
            ),
            None => (String::new(), String::new(), false, false, false, false, true),
        }
    };

    let history = app.store.history();
    let bookmarks = app.store.bookmarks();
    let bm_pairs: Vec<(String, String)> = bookmarks
        .items
        .iter()
        .filter(|b| !b.is_folder)
        .map(|b| (b.title.clone(), b.url.clone()))
        .collect();

    // Keep the list the user can click on the same frame it is drawn.
    app.current_suggestions = toolbar::build_suggestions(
        &app.omnibox_text,
        &history.entries,
        &bm_pairs,
        &engine,
    );

    let data = toolbar::ToolbarData {
        url: &url,
        title: &title,
        can_back,
        can_forward,
        loading,
        bookmarked,
        show_home,
        suggestions: app.current_suggestions.clone(),
        focused: app.omnibox.open,
        text: app.omnibox_text.clone(),
    };

    let result = toolbar::show(ui, &data, &mut app.omnibox, p, &engine);
    repaint |= handle_toolbar_intent(app, &result.intent, &engine);
    repaint
}

fn bookmarks_bar(ui: &mut egui::Ui, app: &mut App, p: &theme::Palette) -> bool {
    if !app.settings.show_bookmarks_bar {
        return false;
    }
    let bookmarks = app.store.bookmarks();
    let items: Vec<kestrel_core::store::Bookmark> = bookmarks.bookmarks_bar().into_iter().cloned().collect();
    if items.is_empty() {
        return false;
    }
    ui.horizontal_centered(|ui| {
        for b in items {
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(b.title.len() as f32 * 8.0 + 48.0, 28.0),
                egui::Sense::click(),
            );
            if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(8.0), p.surface_hover);
            }
            icons::draw(ui, icons::Icon::Bookmark, 14.0, p.text_secondary);
            ui.painter().text(
                egui::pos2(rect.left() + 22.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                b.title.clone(),
                egui::FontId::proportional(12.5),
                p.text_primary,
            );
            if resp.clicked() {
                if let Some(target) = app.resolve_navigation(&b.url) {
                    app.pending.navigate = Some(target);
                }
            }
        }
    });
    ui.add_space(2.0);
    false
}

/// The main area: web content, or an internal page.
fn content(ui: &mut egui::Ui, app: &mut App, p: &theme::Palette) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Sense::hover(),
    );

    match &app.page {
        Page::NewTab => {
            let history = app.store.history();
            let top: Vec<kestrel_core::store::HistoryEntry> =
                history.top_sites(8).into_iter().cloned().collect();
            let d = new_tab::NewTabData {
                shortcuts: &app.settings.new_tab_shortcuts,
                top_sites: &top,
                dark: p.is_dark,
            };
            let hour = local_hour();
            let intent = new_tab::show(ui, &d, p, hour);
            handle_new_tab_intent(app, intent);
        }
        Page::Settings => {
            let s = app.settings.clone();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_settings(ui, &s, p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
        Page::PrivacyDashboard => {
            let (req, blocked) = app.filters.stats();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_privacy(ui, &app.privacy, blocked, req, p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
        Page::History => {
            let h = app.store.history();
            let rows: Vec<(String, String, String)> = h
                .entries
                .iter()
                .rev()
                .take(500)
                .map(|e| {
                    (
                        if e.title.is_empty() { e.url.clone() } else { e.title.clone() },
                        e.url.clone(),
                        format!(
                            "{} · {} visits",
                            kestrel_core::store::relative_time(e.last_visit()),
                            e.visit_count()
                        ),
                    )
                })
                .collect();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_manager(ui, "History", &rows, "No history yet.", p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
        Page::Bookmarks => {
            let b = app.store.bookmarks();
            let rows: Vec<(String, String, String)> = b
                .items
                .iter()
                .filter(|x| !x.is_folder)
                .map(|x| (x.title.clone(), x.url.clone(), String::new()))
                .collect();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_manager(ui, "Bookmarks", &rows, "No bookmarks yet.", p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
        Page::Downloads => {
            let d = app.store.downloads();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_downloads(ui, &d.items, p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
        Page::Error(rule) => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("This request was blocked by your privacy settings.\n\n{rule}"),
                egui::FontId::proportional(15.0),
                p.text_primary,
            );
        }
        Page::Web => {
            // The page texture is composited by the renderer into this rect;
            // nothing to paint here beyond a background.
            ui.painter().rect_filled(rect, 0.0, p.background);
        }
        other => {
            let rows: Vec<(String, String, String)> = Vec::new();
            let mut intent = std::mem::take(&mut app.settings_intent);
            pages::show_manager(ui, &format!("{other:?}"), &rows, "Nothing here yet.", p, &mut intent);
            crate::app::apply_settings_intent(app, &intent);
        }
    }
}

fn menus(_ctx: &egui::Context, _app: &mut App) {}

/// Local hour, used for the new-tab greeting. Falls back to UTC if the local
/// offset cannot be read, which keeps the greeting plausible rather than
/// showing a nonsense hour.
fn local_hour() -> u32 {
    let secs = kestrel_core::store::now_secs();
    ((secs % 86400) / 3600) as u32
}

fn handle_strip_intent(app: &mut App, i: &tab_strip::StripIntent, p: &theme::Palette) -> bool {
    if let Some(id) = i.select {
        app.strip.select(id);
        app.save_session();
        return true;
    }
    if let Some(id) = i.close {
        app.strip.close(id);
        app.save_session();
        return true;
    }
    if i.new_tab {
        app.strip.open("about:newtab");
        app.page = Page::NewTab;
        return true;
    }
    if let Some(id) = i.pin_toggle {
        if app.strip.get(id).map(|t| t.pinned).unwrap_or(false) {
            app.strip.unpin(id);
        } else {
            app.strip.pin(id);
        }
        return true;
    }
    if let Some(id) = i.duplicate {
        if let Some(t) = app.strip.get(id) {
            let url = t.url.clone();
            app.strip.open(&url);
        }
        return true;
    }
    if let Some(id) = i.close_others {
        app.strip.close_others(id);
        return true;
    }
    let _ = p;
    false
}

fn handle_toolbar_intent(
    app: &mut App,
    i: &toolbar::ToolbarIntent,
    engine: &kestrel_core::omnibox::SearchEngine,
) -> bool {
    if i.back {
        if let Some(t) = app.strip.active_tab_mut() {
            if let Some(url) = t.go_back() {
                let u = url.to_string();
                app.pending.navigate = Some(u);
                return true;
            }
        }
    }
    if i.forward {
        if let Some(t) = app.strip.active_tab_mut() {
            if let Some(url) = t.go_forward() {
                let u = url.to_string();
                app.pending.navigate = Some(u);
                return true;
            }
        }
    }
    if i.reload {
        if let Some(t) = app.strip.active_tab() {
            let u = t.url.clone();
            app.pending.navigate = Some(u);
            return true;
        }
    }
    if i.home {
        let home = app.settings.homepage.clone();
        app.page = Page::from_url(&home).unwrap_or(Page::NewTab);
        return true;
    }
    if let Some(url) = &i.omnibox_submitted {
        let target = if url.is_empty() {
            app.settings.homepage.clone()
        } else {
            app.resolve_navigation(url).unwrap_or_else(|| url.clone())
        };
        app.omnibox_text.clear();
        app.omnibox.open = false;
        match Page::from_url(&target) {
            Some(p) => app.page = p,
            None => app.pending.navigate = Some(target),
        }
        return true;
    }
    if let Some(s) = &i.suggestion_chosen {
        if let Some(sugg) = app.current_suggestions.get(*s) {
            let target = toolbar::suggestion_target(sugg, engine);
            match Page::from_url(&target) {
                Some(p) => app.page = p,
                None => app.pending.navigate = Some(target),
            }
            app.omnibox.open = false;
            return true;
        }
    }
    if i.toggle_bookmark {
        if let Some(t) = app.strip.active_tab() {
            let (title, url) = (t.title.clone(), t.url.clone());
            let existing = app.store.bookmarks().items.iter().find(|b| b.url == url).map(|b| b.id);
            app.store.update_bookmarks(|b| match existing {
                Some(id) => {
                    b.remove(id);
                }
                None => {
                    b.add(kestrel_core::store::Bookmark::link(0, &title, &url));
                }
            });
            return true;
        }
    }
    if i.open_privacy {
        app.page = Page::PrivacyDashboard;
        return true;
    }
    if i.open_downloads {
        app.page = Page::Downloads;
        return true;
    }
    false
}

fn handle_new_tab_intent(app: &mut App, i: new_tab::NewTabIntent) {
    if let Some(q) = i.search {
        if let Some(engine) = app.search_engine() {
            if let Some(u) = engine.build_url(&q) {
                app.pending.navigate = Some(u.to_string());
            }
        }
    }
    if let Some(u) = i.open_shortcut {
        if let Some(t) = app.resolve_navigation(&u) {
            app.pending.navigate = Some(t);
        }
    }
    if let Some(u) = i.open_top_site {
        if let Some(t) = app.resolve_navigation(&u) {
            app.pending.navigate = Some(t);
        }
    }
    if i.add_shortcut {
        app.page = Page::Settings;
    }
    if let Some(idx) = i.remove_shortcut {
        if idx < app.settings.new_tab_shortcuts.len() {
            app.settings.new_tab_shortcuts.remove(idx);
            let s = app.settings.clone();
            app.store.update_settings(|d| *d = s);
        }
    }
    if i.open_settings {
        app.page = Page::Settings;
    }
    if i.open_privacy {
        app.page = Page::PrivacyDashboard;
    }
}
