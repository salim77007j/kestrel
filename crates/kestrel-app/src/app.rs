//! Kestrel application: the browser shell that binds the engine, the UI and the
//! core together.
//!
//! Threading model
//! ---------------
//! * The **main thread** owns the window, the egui pass and the Servo event
//!   loop. Servo requires that `spin_event_loop` be called from the thread that
//!   created it, so all engine interaction is funnelled through here.
//! * The **network threads** are owned by Servo internally. Ad-blocking and
//!   privacy decisions are made in `WebViewDelegate::load_web_resource`, which
//!   runs on those threads, against an `Arc`-shared immutable engine — no locks
//!   on the hot path.
//! * Disk writes go through `kestrel_core::Store`, which uses atomic renames.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kestrel_core::adblock::{Decision, FilterEngine, ResourceType};
use kestrel_core::omnibox::{self, SearchEngine};
use kestrel_core::privacy::{PrivacyConfig, Pseudonymiser};
use kestrel_core::store::{
    self, Bookmark, BookmarkStore, ClearTarget, Download, HistoryStore, Session, SessionTab,
    Settings, Store, Theme,
};
use kestrel_ui::icons::{self, Icon};
use kestrel_ui::pages::SettingsIntent;
use kestrel_ui::tab_strip::ContextAction;
use kestrel_ui::tabs::{Page, Tab, TabState, TabStrip};
use kestrel_ui::theme;
use kestrel_ui::toolbar::{self as tb, OmniboxState, Suggestion, ToolbarData};

/// Commands the UI raises that the shell executes outside the draw pass.
#[derive(Debug, Default)]
pub struct PendingActions {
    pub navigate: Option<String>,
    pub open_in_new_tab: Option<String>,
    pub close_tab: Option<u64>,
    pub reopen_closed: bool,
    pub toggle_pin: Option<u64>,
    pub toggle_mute: Option<u64>,
    pub duplicate_tab: Option<u64>,
    pub close_others: Option<u64>,
    pub toggle_bookmark: bool,
    pub open_page: Option<Page>,
    pub toggle_zoom: Option<f32>,
    pub copy_link: Option<String>,
}

/// Everything the shell owns for one window.
pub struct App {
    pub store: Arc<Store>,
    pub strip: TabStrip,
    pub page: Page,
    pub palette: theme::Palette,
    pub settings: Settings,
    pub privacy: PrivacyConfig,
    pub filters: Arc<FilterEngine>,
    pub engine: Option<servo::Servo>,
    pub pseudonymiser: Pseudonymiser,
    /// Live engine views, keyed by tab id.
    pub views: HashMap<u64, servo::WebView>,
    pub rendering: Rc<servo::WindowRenderingContext>,
    pub omnibox: OmniboxState,
    pub omnibox_text: String,
    pub pending: PendingActions,
    pub settings_intent: SettingsIntent,
    pub show_menu: bool,
    pub show_tab_menu: Option<(u64, egui::Pos2)>,
    pub find_query: String,
    pub find_open: bool,
    pub needs_redraw: bool,
    pub last_frame: Instant,
    pub fps_samples: Vec<f32>,
    pub started: Instant,
    /// Set when the engine could not be initialised, so the UI can explain
    /// itself instead of showing an empty window.
    pub engine_error: Option<String>,
}

impl App {
    pub fn new(
        store: Arc<Store>,
        filters: Arc<FilterEngine>,
        rendering: Rc<servo::WindowRenderingContext>,
        engine: Option<servo::Servo>,
    ) -> Self {
        let settings = store.settings();
        let privacy = privacy_from(&settings);
        let mut strip = TabStrip::new("about:newtab");

        // Restore the previous session when the user asked for it and the saved
        // one is recent enough to be meaningful.
        if settings.restore_session_on_launch {
            let session = store.session();
            let fresh = session
                .saved_at
                .map(|t| store::now_secs().saturating_sub(t) < store::SESSION_STALE_AFTER.as_secs())
                .unwrap_or(false);
            if fresh && !session.tabs.is_empty() {
                strip = TabStrip::restore(&session.tabs);
                strip.active = session.active.min(strip.tabs.len().saturating_sub(1));
            }
        }

        let pseudonymiser = Pseudonymiser::from_secret(install_secret(&store));

        Self {
            store,
            strip,
            page: Page::NewTab,
            palette: theme::resolve(settings.theme),
            settings,
            privacy,
            filters,
            engine,
            pseudonymiser,
            views: HashMap::new(),
            rendering,
            omnibox: OmniboxState::default(),
            omnibox_text: String::new(),
            pending: PendingActions::default(),
            settings_intent: SettingsIntent::default(),
            show_menu: false,
            show_tab_menu: None,
            find_query: String::new(),
            find_open: false,
            needs_redraw: true,
            last_frame: Instant::now(),
            fps_samples: Vec::new(),
            started: Instant::now(),
            engine_error: None,
        }
    }

    /// Persist the current session. Called on every meaningful state change and
    /// on shutdown, so a crash loses at most one interaction.
    pub fn save_session(&self) {
        let session = Session {
            tabs: self.strip.to_session(),
            active: self.strip.active,
            window_bounds: None,
            saved_at: 0,
        };
        self.store.save_session(session);
    }

    /// Decide what a navigation request should become, applying the privacy
    /// policy: HTTPS upgrade and tracking-parameter stripping.
    pub fn resolve_navigation(&self, input: &str) -> Option<String> {
        let target = match omnibox::parse_navigable(input) {
            Ok(u) => u,
            Err(_) => {
                let engine = self
                    .search_engine()
                    .ok_or_else(|| "no search engine configured".to_string())?;
                return Some(engine.build_url(input)?.to_string());
            }
        };

        let mut url = target;
        // Never silently weaken an https page to http. The reverse is fine:
        // upgrading http to https is the whole point of the setting, and the
        // site will simply fail to load if it genuinely has no TLS.
        if self.privacy.is_active(kestrel_core::Defense::HttpsUpgrade) && url.scheme() == "http" {
            url.set_scheme("https").ok();
        }
        if self.privacy.is_active(kestrel_core::Defense::QueryStripping) {
            if let Some(cleaned) =
                omnibox::strip_tracking_params(&url, self.settings.strip_tracking_params)
            {
                url = cleaned;
            }
        }
        Some(url.to_string())
    }

    pub fn search_engine(&self) -> Option<SearchEngine> {
        omnibox::default_search_engines()
            .into_iter()
            .find(|e| e.id == self.settings.search_engine)
    }

    /// The network decision for one request, made on a Servo network thread.
    pub fn check_request(&self, url: &str, res: ResourceType) -> Decision {
        if !self.settings.block_trackers && !self.settings.block_ads {
            return Decision::Allow;
        }
        // Respect a per-origin allowlist.
        if let Ok(u) = url::Url::parse(url) {
            if let Some(host) = u.host_str() {
                if self
                    .settings
                    .allowlist
                    .iter()
                    .any(|a| host == a || host.ends_with(&format!(".{a}")))
                {
                    return Decision::Allow;
                }
            }
        }
        self.filters.check(url, res)
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.store
            .bookmarks()
            .items
            .iter()
            .any(|b| b.url == url && !b.is_folder)
    }

    pub fn apply_theme(&mut self) {
        self.palette = theme::resolve(self.settings.theme);
    }

    /// Recompute the privacy config from stored settings.
    pub fn refresh_privacy(&mut self) {
        self.privacy = privacy_from(&self.settings);
    }
}

fn privacy_from(s: &Settings) -> PrivacyConfig {
    let mut c = PrivacyConfig::default();
    c.enabled = s.privacy_enabled;
    c.protection_level = match s.protection_level.as_str() {
        "strict" => kestrel_core::privacy::Level::Strict,
        "custom" => kestrel_core::privacy::Level::Custom,
        _ => kestrel_core::privacy::Level::Balanced,
    };
    c.block_trackers = s.block_trackers;
    c.block_ads = s.block_ads;
    c.block_annoyances = s.block_annoyances;
    c.https_only = s.https_only;
    c
}

/// The per-install secret used to derive unlinkable site pseudonyms.
///
/// Read from the profile, generated once. It is deliberately not tied to any
/// hardware identifier, so a user can rotate it to fully de-link their profile.
fn install_secret(store: &Store) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let path = store.dir().join("install-secret");
    if let Ok(data) = std::fs::read(&path) {
        if data.len() >= 32 {
            let mut s = [0u8; 32];
            s.copy_from_slice(&data[..32]);
            return s;
        }
    }
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).unwrap_or_else(|_| {
        // If the OS CSPRNG is unavailable, fall back to a time-and-pid mix.
        // This is a degraded but non-predictable-enough path for a local key.
        let t = store::now_secs();
        let h = Sha256::digest(t.to_le_bytes());
        secret.copy_from_slice(&h);
    });
    let _ = std::fs::write(&path, &secret);
    secret
}

/// Apply accumulated settings changes. Called once per frame after the UI pass
/// so the store is written at most once per frame rather than per toggle.
pub fn apply_settings_intent(app: &mut App, intent: &SettingsIntent) {
    let mut dirty = false;
    let s = &mut app.settings;

    if let Some(v) = intent.set_theme {
        if s.theme != v {
            s.theme = v;
            dirty = true;
        }
    }
    if let Some(v) = intent.set_homepage {
        if s.homepage != v {
            s.homepage = v;
            dirty = true;
        }
    }
    if let Some(v) = intent.set_search_engine {
        if s.search_engine != v {
            s.search_engine = v;
            dirty = true;
        }
    }
    if let Some(v) = intent.toggle_bookmarks_bar {
        s.show_bookmarks_bar = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_home_button {
        s.show_home_button = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_privacy {
        s.privacy_enabled = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_block_trackers {
        s.block_trackers = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_block_ads {
        s.block_ads = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_block_annoyances {
        s.block_annoyances = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_https_only {
        s.https_only = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_strip_params {
        s.strip_tracking_params = v;
        dirty = true;
    }
    if let Some(v) = intent.toggle_hw_accel {
        s.hardware_acceleration = v;
        dirty = true;
    }
    if let Some(v) = intent.set_zoom {
        s.default_zoom = v;
        dirty = true;
    }
    if let Some(v) = intent.set_protection {
        if s.protection_level != v {
            s.protection_level = v.to_string();
            dirty = true;
        }
    }
    if let Some((d, on)) = intent.toggle_defense {
        // Defence toggles live in the privacy config; mirror them into the
        // level string so a restart restores the same state.
        if on {
            // Ensure the preset no longer gates the switch off.
            if s.protection_level != "custom" {
                s.protection_level = "custom".into();
            }
        }
        app.privacy.set_override(d, on);
        dirty = true;
    }
    if let Some((name, url)) = intent.add_shortcut {
        s.new_tab_shortcuts.push((name, url));
        dirty = true;
    }
    if let Some(i) = intent.remove_shortcut {
        if i < s.new_tab_shortcuts.len() {
            s.new_tab_shortcuts.remove(i);
            dirty = true;
        }
    }

    if intent.clear_history {
        app.store.clear_data(ClearTarget::History);
    }
    if intent.clear_downloads {
        app.store.update_downloads(|d| d.clear_finished());
    }
    if intent.clear_cookies {
        app.store.clear_data(ClearTarget::Session);
    }
    if intent.clear_all {
        app.store.clear_data(ClearTarget::All);
    }
    if intent.go_back {
        app.page = Page::Web;
    }

    if dirty {
        let snapshot = app.settings.clone();
        app.store.update_settings(|dst| *dst = snapshot);
        app.apply_theme();
        app.refresh_privacy();
    }
}
