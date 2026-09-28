//! Tab and window state.
//!
//! This is the state machine the UI renders and the engine drives. It contains
//! no rendering and no engine types, so tab behaviour — pinning, grouping,
//! session restore, background throttling — is unit-testable on its own.

use kestrel_core::omnibox::{looks_like_url, parse_navigable, pretty_url};
use kestrel_core::store::{Bookmark, SessionTab};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where a tab's content should be rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabState {
    /// A page is loading.
    Loading,
    /// Content is loaded and interactive.
    Ready,
    /// The tab crashed; show a recovery page.
    Crashed,
    /// The engine has not yet produced a first frame.
    Blank,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub favicon: Option<Vec<u8>>,
    pub pinned: bool,
    pub muted: bool,
    pub group: Option<String>,
    pub state: TabState,
    /// Session history for back/forward within this tab.
    pub history: Vec<String>,
    pub history_index: usize,
    pub zoom: f32,
    /// Identity of the live engine view, when one is attached.
    pub engine_view: Option<u64>,
    /// Count of trackers blocked on this page, shown in the privacy dashboard.
    pub trackers_blocked: u32,
    pub audible: bool,
    /// Set when the page asked to go fullscreen.
    pub fullscreen: bool,
}

impl Tab {
    pub fn new(id: u64, url: &str) -> Self {
        let mut history = Vec::new();
        history.push(url.to_string());
        Self {
            id,
            url: url.to_string(),
            title: "New Tab".to_string(),
            favicon: None,
            pinned: false,
            muted: false,
            group: None,
            state: TabState::Blank,
            history,
            history_index: 0,
            zoom: 1.0,
            engine_view: None,
            trackers_blocked: 0,
            audible: false,
            fullscreen: false,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    /// Navigate within the tab, truncating any forward history.
    pub fn navigate(&mut self, url: &str) {
        if self.history.get(self.history_index).map(|s| s.as_str()) == Some(url) {
            return;
        }
        self.history.truncate(self.history_index + 1);
        self.history.push(url.to_string());
        self.history_index = self.history.len() - 1;
        self.url = url.to_string();
        self.state = TabState::Loading;
        // A new document resets per-page state.
        self.trackers_blocked = 0;
        self.fullscreen = false;
    }

    /// Rewrite the current entry in place, used while a page redirects or
    /// updates its title without creating a history entry.
    pub fn replace_current(&mut self, url: &str) {
        self.url = url.to_string();
        if let Some(slot) = self.history.get_mut(self.history_index) {
            *slot = url.to_string();
        }
    }

    pub fn go_back(&mut self) -> Option<&str> {
        if !self.can_go_back() {
            return None;
        }
        self.history_index -= 1;
        let url = self.history[self.history_index].clone();
        self.url = url;
        Some(self.history.get(self.history_index).map(|s| s.as_str()).unwrap_or(""))
    }

    pub fn go_forward(&mut self) -> Option<&str> {
        if !self.can_go_forward() {
            return None;
        }
        self.history_index += 1;
        self.url = self.history[self.history_index].clone();
        Some(self.history.get(self.history_index).map(|s| s.as_str()).unwrap_or(""))
    }

    /// The URL shown in the tab strip: the page title when known, otherwise a
    /// readable form of the URL.
    pub fn display_title(&self) -> String {
        if !self.title.is_empty() && self.title != "New Tab" {
            return self.title.clone();
        }
        match parse_navigable(&self.url) {
            Ok(u) => pretty_url(&u),
            Err(_) => self.url.clone(),
        }
    }

    pub fn is_audible_or_muted(&self) -> bool {
        self.audible || self.muted
    }

    /// Secure if the current URL is https, or is an internal page.
    pub fn is_secure(&self) -> bool {
        self.url.starts_with("https://") || self.url.starts_with("about:")
    }

    pub fn to_session(&self) -> SessionTab {
        SessionTab {
            url: self.url.clone(),
            title: self.title.clone(),
            pinned: self.pinned,
            muted: self.muted,
            group: self.group.clone(),
            history: self.history.clone(),
            history_index: self.history_index,
        }
    }

    pub fn from_session(s: &SessionTab, id: u64) -> Self {
        let mut t = Tab::new(id, &s.url);
        t.title = s.title.clone();
        t.pinned = s.pinned;
        t.muted = s.muted;
        t.group = s.group.clone();
        if !s.history.is_empty() {
            t.history = s.history.clone();
            t.history_index = s.history_index.min(s.history.len() - 1);
            t.url = s.history[t.history_index].clone();
        }
        t.state = TabState::Ready;
        t
    }
}

/// Which overlay the main area is showing. Internal pages are real navigations,
/// not modal state, so they participate in tab history.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Web,
    NewTab,
    Settings,
    PrivacyDashboard,
    Bookmarks,
    History,
    Downloads,
    Find,
    DevTools,
    Permissions,
    About,
    Error(String),
}

impl Page {
    pub fn url(&self) -> String {
        match self {
            Page::Web => String::new(),
            Page::NewTab => "about:newtab".into(),
            Page::Settings => "kestrel://settings".into(),
            Page::PrivacyDashboard => "kestrel://privacy".into(),
            Page::Bookmarks => "kestrel://bookmarks".into(),
            Page::History => "kestrel://history".into(),
            Page::Downloads => "kestrel://downloads".into(),
            Page::Find => "kestrel://find".into(),
            Page::DevTools => "kestrel://devtools".into(),
            Page::Permissions => "kestrel://permissions".into(),
            Page::About => "kestrel://about".into(),
            Page::Error(e) => format!("kestrel://error/{e}"),
        }
    }

    pub fn from_url(url: &str) -> Option<Page> {
        match url {
            "about:newtab" => Some(Page::NewTab),
            "kestrel://settings" => Some(Page::Settings),
            "kestrel://privacy" => Some(Page::PrivacyDashboard),
            "kestrel://bookmarks" => Some(Page::Bookmarks),
            "kestrel://history" => Some(Page::History),
            "kestrel://downloads" => Some(Page::Downloads),
            "kestrel://find" => Some(Page::Find),
            "kestrel://devtools" => Some(Page::DevTools),
            "kestrel://permissions" => Some(Page::Permissions),
            "kestrel://about" => Some(Page::About),
            u if u.starts_with("kestrel://error/") => {
                Some(Page::Error(u.trim_start_matches("kestrel://error/").to_string()))
            }
            _ => None,
        }
    }

    pub fn is_internal(&self) -> bool {
        !matches!(self, Page::Web | Page::Error(_))
    }
}

/// The full set of tabs plus selection, pinning order and groups.
#[derive(Debug, Default)]
pub struct TabStrip {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
    /// Tabs closed this session, for reopen-closed-tab.
    pub recently_closed: Vec<(u64, Tab)>,
}

impl TabStrip {
    pub fn new(url: &str) -> Self {
        let mut s = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            recently_closed: Vec::new(),
        };
        s.open(url);
        s
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn open(&mut self, url: &str) -> u64 {
        let id = self.alloc_id();
        self.tabs.push(Tab::new(id, url));
        self.active = self.tabs.len() - 1;
        id
    }

    pub fn open_background(&mut self, url: &str) -> u64 {
        let id = self.alloc_id();
        self.tabs.push(Tab::new(id, url));
        id
    }

    /// Close a tab, keeping the selection sensible and remembering it so the
    /// user can reopen it. The last remaining tab is never closed; it is
    /// replaced with a new tab page instead, matching user expectation.
    pub fn close(&mut self, id: u64) -> Option<Tab> {
        let idx = self.tabs.iter().position(|t| t.id == id)?;
        let removed = self.tabs.remove(idx);
        self.recently_closed.insert(0, (id, removed.clone()));
        self.recently_closed.truncate(25);

        if self.tabs.is_empty() {
            self.open("about:newtab");
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        Some(removed)
    }

    pub fn close_active(&mut self) -> Option<Tab> {
        self.tabs.get(self.active).map(|t| t.id).and_then(|id| self.close(id))
    }

    /// Reopen the most recently closed tab, preserving its history.
    pub fn reopen_closed(&mut self) -> Option<u64> {
        let (_, tab) = self.recently_closed.remove(0)?;
        let id = self.alloc_id();
        let mut restored = tab;
        restored.id = id;
        restored.engine_view = None;
        restored.state = TabState::Loading;
        // Insert at the position it occupied so tab order feels unchanged.
        let insert_at = self.active + 1;
        self.tabs.insert(insert_at.min(self.tabs.len()), restored);
        self.active = insert_at.min(self.tabs.len() - 1);
        Some(id)
    }

    pub fn get(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let a = self.active;
        self.tabs.get_mut(a)
    }

    pub fn select(&mut self, id: u64) {
        if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
            self.active = i;
        }
    }

    pub fn pin(&mut self, id: u64) {
        let Some(tab) = self.get_mut(id) else { return };
        if tab.pinned {
            return;
        }
        tab.pinned = true;
        self.reorder_pinned_first();
    }

    pub fn unpin(&mut self, id: u64) {
        let Some(tab) = self.get_mut(id) else { return };
        tab.pinned = false;
        self.reorder_pinned_first();
    }

    /// Pinned tabs always sort to the front, preserving relative order within
    /// each group so re-pinning does not shuffle the strip unexpectedly.
    fn reorder_pinned_first(&mut self) {
        let active_id = self.tabs.get(self.active).map(|t| t.id);
        let active_pos = self.active;
        let mut pinned: Vec<Tab> = Vec::new();
        let mut normal: Vec<Tab> = Vec::new();
        for t in self.tabs.drain(..) {
            if t.pinned {
                pinned.push(t);
            } else {
                normal.push(t);
            }
        }
        pinned.extend(normal);
        self.tabs = pinned;
        if let Some(id) = active_id {
            if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                self.active = i;
            }
        } else {
            self.active = active_pos.min(self.tabs.len().saturating_sub(1));
        }
    }

    pub fn set_group(&mut self, ids: &[u64], group: Option<String>) {
        for id in ids {
            if let Some(t) = self.get_mut(*id) {
                t.group = group.clone();
            }
        }
    }

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let t = self.tabs.remove(from);
        self.tabs.insert(to, t);
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
    }

    /// Close every tab except the pinned ones and the active tab.
    pub fn close_others(&mut self, id: u64) {
        let keep = self.tabs.iter().find(|t| t.id == id).map(|t| t.pinned).unwrap_or(false);
        let closing: Vec<u64> = self
            .tabs
            .iter()
            .filter(|t| t.id != id && !(keep && t.pinned))
            .map(|t| t.id)
            .collect();
        for c in closing {
            self.close(c);
        }
    }

    pub fn groups(&self) -> HashMap<String, Vec<u64>> {
        let mut out: HashMap<String, Vec<u64>> = HashMap::new();
        for t in &self.tabs {
            if let Some(g) = &t.group {
                out.entry(g.clone()).or_default().push(t.id);
            }
        }
        out
    }

    pub fn to_session(&self) -> Vec<SessionTab> {
        self.tabs.iter().map(|t| t.to_session()).collect()
    }

    pub fn restore(session: &[SessionTab]) -> Self {
        let mut s = Self {
            tabs: session
                .iter()
                .enumerate()
                .map(|(i, t)| Tab::from_session(t, i as u64 + 1))
                .collect(),
            active: 0,
            next_id: session.len() as u64 + 1,
            recently_closed: Vec::new(),
        };
        if s.tabs.is_empty() {
            s.open("about:newtab");
        }
        s
    }

    /// Tabs eligible for background throttling: not active, not pinned (pinned
    /// tabs are explicitly kept hot by the user), not audible.
    pub fn throttlable(&self) -> Vec<u64> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(i, t)| *i != self.active && !t.pinned && !t.audible)
            .map(|(_, t)| t.id)
            .collect()
    }
}

/// A permission decision requested by a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    ClipboardRead,
    ClipboardWrite,
    Midi,
    Bluetooth,
    ScreenShare,
    PersistentStorage,
}

impl PermissionKind {
    pub fn label(&self) -> &'static str {
        match self {
            PermissionKind::Camera => "Camera",
            PermissionKind::Microphone => "Microphone",
            PermissionKind::Geolocation => "Location",
            PermissionKind::Notifications => "Notifications",
            PermissionKind::ClipboardRead => "Read clipboard",
            PermissionKind::ClipboardWrite => "Write clipboard",
            PermissionKind::Midi => "MIDI",
            PermissionKind::Bluetooth => "Bluetooth",
            PermissionKind::ScreenShare => "Screen sharing",
            PermissionKind::PersistentStorage => "Persistent storage",
        }
    }

    /// The risk shown in the permission prompt. A page asking for the camera
    /// deserves more scrutiny than one asking to store data.
    pub fn risk(&self) -> Risk {
        match self {
            PermissionKind::Camera
            | PermissionKind::Microphone
            | PermissionKind::Geolocation
            | PermissionKind::ScreenShare
            | PermissionKind::Bluetooth => Risk::High,
            PermissionKind::Notifications | PermissionKind::Midi => Risk::Medium,
            _ => Risk::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDecision {
    /// Ask the user each time.
    Ask,
    Allow,
    Deny,
}

/// A stored permission for one origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePermission {
    pub origin: String,
    pub kind: String,
    pub decision: PermissionDecision,
    pub updated: u64,
}

/// A bookmark match, used to drive the omnibox star state.
pub fn is_bookmarked(bookmarks: &[Bookmark], url: &str) -> bool {
    bookmarks.iter().any(|b| b.url == url)
}

/// The icon to show in a tab: favicon when the engine produced one, otherwise a
/// globe so the tab never looks empty.
pub fn tab_icon(tab: &Tab) -> crate::icons::Icon {
    if tab.favicon.is_some() {
        crate::icons::Icon::Globe
    } else if tab.audible {
        crate::icons::Icon::Volume
    } else {
        crate::icons::Icon::Globe
    }
}

/// Whether the omnibox should show the "search" affordance rather than a URL.
pub fn omnibox_is_search(text: &str) -> bool {
    !text.is_empty() && !looks_like_url(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip() -> TabStrip {
        TabStrip::new("about:newtab")
    }

    #[test]
    fn opening_tabs_selects_the_new_one() {
        let mut s = strip();
        let a = s.active;
        let b = s.open("https://example.com");
        assert_eq!(s.active_tab().unwrap().id, b);
        assert_ne!(a, b);
    }

    #[test]
    fn closing_last_tab_opens_a_fresh_one() {
        let mut s = strip();
        let id = s.active_tab().unwrap().id;
        s.close(id);
        assert_eq!(s.tabs.len(), 1, "browser must never be left with no tab");
        assert_eq!(s.active_tab().unwrap().url, "about:newtab");
    }

    #[test]
    fn closed_tab_reopens_with_history() {
        let mut s = strip();
        let id = s.open("https://a.com");
        s.get_mut(id).unwrap().navigate("https://b.com");
        s.close(id);
        let restored = s.reopen_closed().unwrap();
        let t = s.get(restored).unwrap();
        assert_eq!(t.url, "https://b.com");
        assert_eq!(t.history.len(), 2, "history must survive a close/reopen");
    }

    #[test]
    fn pinning_sorts_to_the_front() {
        let mut s = strip();
        let a = s.open("https://a.com");
        let _b = s.open("https://b.com");
        s.pin(a);
        assert!(s.tabs[0].pinned, "pinned tab must lead the strip");
        s.pin(a);
        assert_eq!(s.tabs.len(), 3, "pinning twice must not duplicate");
    }

    #[test]
    fn back_and_forward_track_history() {
        let mut t = Tab::new(1, "https://a.com");
        t.navigate("https://b.com");
        t.navigate("https://c.com");
        assert!(!t.can_go_back() == false);
        assert_eq!(t.go_back(), Some("https://b.com"));
        assert_eq!(t.go_back(), Some("https://a.com"));
        assert!(!t.can_go_back());
        assert_eq!(t.go_forward(), Some("https://b.com"));
    }

    #[test]
    fn navigating_truncates_forward_history() {
        let mut t = Tab::new(1, "https://a.com");
        t.navigate("https://b.com");
        t.go_back();
        t.navigate("https://c.com");
        assert!(!t.can_go_forward(), "new navigation must drop forward entries");
        assert_eq!(t.history.len(), 2);
    }

    #[test]
    fn navigating_to_same_url_does_not_duplicate() {
        let mut t = Tab::new(1, "https://a.com");
        t.navigate("https://a.com");
        assert_eq!(t.history.len(), 1);
    }

    #[test]
    fn session_round_trips() {
        let mut s = strip();
        let a = s.open("https://a.com");
        s.get_mut(a).unwrap().navigate("https://b.com");
        s.get_mut(a).unwrap().pinned = true;
        s.get_mut(a).unwrap().group = Some("Work".into());
        let session = s.to_session();
        let restored = TabStrip::restore(&session);
        let t = &restored.tabs[1];
        assert_eq!(t.url, "https://b.com");
        assert!(t.pinned);
        assert_eq!(t.group.as_deref(), Some("Work"));
    }

    #[test]
    fn moving_tabs_keeps_selection_on_the_same_tab() {
        let mut s = strip();
        let a = s.open("https://a.com");
        s.open("https://b.com");
        s.open("https://c.com");
        s.select(a);
        let from = s.tabs.iter().position(|t| t.id == a).unwrap();
        s.move_tab(from, 2);
        assert_eq!(s.tabs[s.active].id, a, "selection must follow the tab");
    }

    #[test]
    fn close_others_spares_pinned_tabs() {
        let mut s = strip();
        let a = s.open("https://a.com");
        let b = s.open("https://b.com");
        s.open("https://c.com");
        s.pin(a);
        s.close_others(b);
        assert!(s.tabs.iter().any(|t| t.id == a), "pinned tab must survive");
        assert!(!s.tabs.iter().any(|t| t.id != a && t.id != b));
    }

    #[test]
    fn throttling_spares_active_pinned_and_audible() {
        let mut s = strip();
        let a = s.open("https://a.com");
        let b = s.open("https://b.com");
        let c = s.open("https://c.com");
        s.pin(b);
        s.get_mut(c).unwrap().audible = true;
        s.select(a);
        let t = s.throttlable();
        assert!(!t.contains(&a), "active tab must not throttle");
        assert!(!t.contains(&b), "pinned tab must not throttle");
        assert!(!t.contains(&c), "audible tab must not throttle");
    }

    #[test]
    fn internal_pages_round_trip() {
        for p in [Page::Settings, Page::History, Page::NewTab] {
            assert_eq!(Page::from_url(&p.url()), Some(p));
        }
        assert_eq!(Page::from_url("https://example.com"), None);
    }

    #[test]
    fn secure_detection() {
        let mut t = Tab::new(1, "https://example.com");
        assert!(t.is_secure());
        t.navigate("http://example.com");
        assert!(!t.is_secure(), "plain http must not report as secure");
        t.navigate("about:newtab");
        assert!(t.is_secure());
    }

    #[test]
    fn high_risk_permissions_are_flagged() {
        assert_eq!(PermissionKind::Camera.risk(), Risk::High);
        assert_eq!(PermissionKind::ClipboardWrite.risk(), Risk::Low);
    }
}
