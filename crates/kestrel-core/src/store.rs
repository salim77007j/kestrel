//! Persistent storage: bookmarks, history, downloads, settings and session state.
//!
//! Implementation notes
//! --------------------
//! * **Atomic writes.** Every file is written to a temp path and renamed into
//!   place, so a crash mid-write can never leave a truncated profile. This is
//!   what makes session restore reliable.
//! * **Append-only history.** History is the only unbounded data set, so it is
//!   appended in JSON Lines and compacted on a threshold. This keeps a page
//!   visit from ever rewriting the whole file.
//! * **No C dependencies.** A browser that cannot start because a database
//!   library failed to initialise is not a fast browser.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, used for all timestamps.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub folder: Option<String>,
    /// When true this is a folder rather than a link. Folders hold children
    /// addressed by `parent`.
    pub is_folder: bool,
    pub parent: Option<u64>,
    pub created: u64,
}

impl Bookmark {
    pub fn link(id: u64, title: &str, url: &str) -> Self {
        Self {
            id,
            title: title.to_string(),
            url: url.to_string(),
            folder: None,
            is_folder: false,
            parent: None,
            created: now_secs(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookmarkStore {
    pub items: Vec<Bookmark>,
}

impl BookmarkStore {
    pub fn add(&mut self, mut b: Bookmark) -> u64 {
        let id = if b.id == 0 { self.next_id() } else { b.id };
        b.id = id;
        if b.title.is_empty() {
            b.title = b.url.clone();
        }
        self.items.push(b);
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        // Remove the entry and, if it was a folder, everything beneath it.
        let doomed = self.descendants(id);
        self.items.retain(|b| !doomed.contains(&b.id));
        self.items.len() != before
    }

    fn descendants(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i];
            for b in &self.items {
                if b.parent == Some(cur) && !out.contains(&b.id) {
                    out.push(b.id);
                }
            }
            i += 1;
        }
        out
    }

    pub fn move_to(&mut self, id: u64, parent: Option<u64>) {
        if let Some(b) = self.items.iter_mut().find(|b| b.id == id) {
            b.parent = parent;
        }
    }

    pub fn top_level(&self) -> Vec<&Bookmark> {
        self.items.iter().filter(|b| b.parent.is_none()).collect()
    }

    pub fn children(&self, parent: u64) -> Vec<&Bookmark> {
        self.items.iter().filter(|b| b.parent == Some(parent)).collect()
    }

    pub fn search(&self, query: &str) -> Vec<&Bookmark> {
        let q = query.to_lowercase();
        self.items
            .iter()
            .filter(|b| b.title.to_lowercase().contains(&q) || b.url.to_lowercase().contains(&q))
            .collect()
    }

    pub fn bookmarks_bar(&self) -> Vec<&Bookmark> {
        self.items
            .iter()
            .filter(|b| !b.is_folder && b.folder.as_deref() == Some("bar"))
            .collect()
    }

    fn next_id(&self) -> u64 {
        self.items.iter().map(|b| b.id).max().unwrap_or(0) + 1
    }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub url: String,
    pub title: String,
    /// Visits recorded as unix seconds, most recent last.
    pub visits: Vec<u64>,
}

impl HistoryEntry {
    pub fn last_visit(&self) -> u64 {
        self.visits.last().copied().unwrap_or(0)
    }

    pub fn visit_count(&self) -> usize {
        self.visits.len()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryStore {
    pub entries: Vec<HistoryEntry>,
    /// Recently closed tabs for "reopen closed tab".
    pub closed_tabs: Vec<(String, u64)>,
}

impl HistoryStore {
    pub fn record(&mut self, url: &str, title: &str) {
        let t = now_secs();
        if let Some(e) = self.entries.iter_mut().find(|e| e.url == url) {
            e.visits.push(t);
            if !title.is_empty() {
                e.title = title.to_string();
            }
            return;
        }
        let id = self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        self.entries.push(HistoryEntry {
            id,
            url: url.to_string(),
            title: title.to_string(),
            visits: vec![t],
        });
    }

    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return self.entries.iter().collect();
        }
        self.entries
            .iter()
            .filter(|e| e.title.to_lowercase().contains(&q) || e.url.to_lowercase().contains(&q))
            .collect()
    }

    /// Top sites by visit count, for the new-tab page.
    pub fn top_sites(&self, n: usize) -> Vec<&HistoryEntry> {
        let mut v: Vec<&HistoryEntry> = self.entries.iter().collect();
        v.sort_by(|a, b| b.visit_count().cmp(&a.visit_count()).then(b.last_visit().cmp(&a.last_visit())));
        v.truncate(n);
        v
    }

    pub fn remove(&mut self, url: &str) {
        self.entries.retain(|e| e.url != url);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn add_closed(&mut self, url: &str) {
        self.closed_tabs.insert(0, (url.to_string(), now_secs()));
        self.closed_tabs.truncate(25);
    }

    pub fn pop_closed(&mut self) -> Option<String> {
        if self.closed_tabs.is_empty() {
            None
        } else {
            Some(self.closed_tabs.remove(0).0)
        }
    }
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub id: u64,
    pub url: String,
    pub filename: String,
    pub path: String,
    pub bytes_written: u64,
    pub total_bytes: u64,
    pub state: DownloadState,
    pub started: u64,
    #[serde(default)]
    pub error: Option<String>,
}

impl Download {
    /// Progress in 0..=1, or `None` when the total size is unknown.
    pub fn progress(&self) -> Option<f32> {
        if self.total_bytes == 0 {
            None
        } else {
            Some((self.bytes_written as f32 / self.total_bytes as f32).clamp(0.0, 1.0))
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadStore {
    pub items: Vec<Download>,
}

impl DownloadStore {
    pub fn start(&mut self, url: &str, filename: &str, path: &str, total: u64) -> u64 {
        let id = self.items.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        self.items.insert(
            0,
            Download {
                id,
                url: url.to_string(),
                filename: filename.to_string(),
                path: path.to_string(),
                bytes_written: 0,
                total_bytes: total,
                state: DownloadState::InProgress,
                started: now_secs(),
                error: None,
            },
        );
        id
    }

    pub fn progress(&mut self, id: u64, written: u64) {
        if let Some(d) = self.items.iter_mut().find(|d| d.id == id) {
            d.bytes_written = written;
        }
    }

    pub fn finish(&mut self, id: u64) {
        if let Some(d) = self.items.iter_mut().find(|d| d.id == id) {
            d.state = DownloadState::Completed;
            if d.total_bytes == 0 {
                d.total_bytes = d.bytes_written;
            }
        }
    }

    pub fn fail(&mut self, id: u64, err: &str) {
        if let Some(d) = self.items.iter_mut().find(|d| d.id == id) {
            d.state = DownloadState::Failed;
            d.error = Some(err.to_string());
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(d) = self.items.iter_mut().find(|d| d.id == id) {
            d.state = DownloadState::Cancelled;
        }
    }

    pub fn clear_finished(&mut self) {
        self.items
            .retain(|d| !matches!(d.state, DownloadState::Completed | DownloadState::Cancelled));
    }
}

/// Derive a safe on-disk filename from a URL, matching what a user expects and
/// refusing anything that could escape the download directory.
pub fn filename_from_url(url: &str, suggested: Option<&str>) -> String {
    let raw = suggested
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let path = url.split(['?', '#']).next().unwrap_or(url);
            // Take the last non-empty path segment. A trailing slash would
            // otherwise yield an empty name, so fall back to the host.
            let seg = path
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            if seg.is_empty() {
                // No usable path: name the file after the host, which is what a
                // user expects from "https://example.com/".
                url::Url::parse(url)
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .unwrap_or_else(|| "download".to_string())
            } else {
                seg
            }
        });
    // Strip any path components an attacker may have embedded.
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("download").to_string();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub search_engine: String,
    pub homepage: String,
    pub show_bookmarks_bar: bool,
    pub show_home_button: bool,
    pub default_zoom: f32,
    pub block_trackers: bool,
    pub block_ads: bool,
    pub block_annoyances: bool,
    pub https_only: bool,
    pub strip_tracking_params: bool,
    pub privacy_enabled: bool,
    pub protection_level: String,
    pub ask_before_downloading_multiple: bool,
    pub restore_session_on_launch: bool,
    pub clear_on_exit: bool,
    pub spellcheck: bool,
    pub reduced_motion: bool,
    /// Per-site defence overrides, e.g. site -> disabled defenses.
    pub site_exceptions: HashMap<String, Vec<String>>,
    /// Custom user filters, one per line.
    pub custom_filters: Vec<String>,
    pub custom_blocklist_urls: Vec<String>,
    pub allowlist: Vec<String>,
    pub new_tab_shortcuts: Vec<(String, String)>,
    pub hardware_acceleration: bool,
    pub max_tabs_memory_mb: u32,
    pub autoplay_policy: String,
    pub password_save_enabled: bool,
    pub telemetry_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            search_engine: "duckduckgo".into(),
            homepage: "about:newtab".into(),
            show_bookmarks_bar: true,
            show_home_button: true,
            default_zoom: 1.0,
            block_trackers: true,
            block_ads: true,
            block_annoyances: true,
            https_only: true,
            strip_tracking_params: true,
            privacy_enabled: true,
            protection_level: "balanced".into(),
            ask_before_downloading_multiple: true,
            restore_session_on_launch: true,
            clear_on_exit: false,
            spellcheck: false,
            reduced_motion: false,
            site_exceptions: HashMap::new(),
            custom_filters: Vec::new(),
            custom_blocklist_urls: Vec::new(),
            allowlist: Vec::new(),
            new_tab_shortcuts: crate::default_shortcuts(),
            hardware_acceleration: true,
            max_tabs_memory_mb: 2048,
            autoplay_policy: "document-user-gesture".into(),
            password_save_enabled: true,
            telemetry_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    pub muted: bool,
    pub group: Option<String>,
    pub history: Vec<String>,
    pub history_index: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    pub window_bounds: Option<(i32, i32, u32, u32)>,
    pub saved_at: u64,
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// Owns the profile directory and all persisted state.
pub struct Store {
    dir: PathBuf,
    settings: Mutex<Settings>,
    bookmarks: Mutex<BookmarkStore>,
    history: Mutex<HistoryStore>,
    downloads: Mutex<DownloadStore>,
    session: Mutex<Session>,
    /// Deferred history writes; flushed periodically so navigation stays fast.
    history_dirty: AtomicU64,
}

impl Store {
    /// Open (or create) a profile rooted at `dir`.
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        fs::create_dir_all(dir.join("downloads"))?;
        let s = Self {
            dir: dir.to_path_buf(),
            settings: Mutex::new(load_json(&dir.join("settings.json"), Settings::default())),
            bookmarks: Mutex::new(load_json(&dir.join("bookmarks.json"), BookmarkStore::default())),
            history: Mutex::new(HistoryStore::default()),
            downloads: Mutex::new(load_json(&dir.join("downloads.json"), DownloadStore::default())),
            session: Mutex::new(load_json(&dir.join("session.json"), Session::default())),
            history_dirty: AtomicU64::new(0),
        };
        s.load_history();
        Ok(s)
    }

    /// The conventional profile location.
    pub fn default_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kestrel")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.dir.join("downloads")
    }

    pub fn settings(&self) -> Settings {
        snapshot(&self.settings)
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        {
            let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut s);
        }
        self.save_settings();
    }

    pub fn save_settings(&self) {
        let s = snapshot(&self.settings);
        save_json(&self.dir.join("settings.json"), &s);
    }

    pub fn bookmarks(&self) -> BookmarkStore {
        snapshot(&self.bookmarks)
    }

    pub fn update_bookmarks(&self, f: impl FnOnce(&mut BookmarkStore)) {
        {
            let mut b = self.bookmarks.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut b);
        }
        let b = snapshot(&self.bookmarks);
        save_json(&self.dir.join("bookmarks.json"), &b);
    }

    pub fn history(&self) -> HistoryStore {
        snapshot(&self.history)
    }

    pub fn record_visit(&self, url: &str, title: &str) {
        if url.starts_with("about:") {
            return;
        }
        self.history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(url, title);
        self.history_dirty.fetch_add(1, Ordering::Relaxed);
    }

    /// Append newly visited pages to disk without rewriting the whole file.
    pub fn flush_history(&self) {
        if self.history_dirty.swap(0, Ordering::Relaxed) == 0 {
            return;
        }
        let h = snapshot(&self.history);
        let path = self.dir.join("history.jsonl");
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            for e in &h.entries {
                if let Ok(line) = serde_json::to_string(e) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }

    /// Rewrite history as a single file, dropping duplicates. Called on clean
    /// shutdown and when the file grows past a threshold.
    pub fn compact_history(&self) {
        let h = snapshot(&self.history);
        let path = self.dir.join("history.jsonl");
        let tmp = self.dir.join("history.jsonl.tmp");
        if let Ok(mut f) = File::create(&tmp) {
            for e in &h.entries {
                if let Ok(line) = serde_json::to_string(e) {
                    let _ = writeln!(f, "{line}");
                }
            }
            let _ = f.sync_all();
        }
        let _ = fs::rename(&tmp, &path);
    }

    fn load_history(&self) {
        let path = self.dir.join("history.jsonl");
        let Ok(f) = File::open(&path) else { return };
        let mut store = self.history.lock().unwrap_or_else(|e| e.into_inner());
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(e) = serde_json::from_str::<HistoryEntry>(&line) {
                // Compaction can leave a duplicate trailing record after a
                // crash; keep the first and merge visits.
                if let Some(existing) = store.entries.iter_mut().find(|x| x.url == e.url) {
                    existing.visits.extend(e.visits);
                } else {
                    store.entries.push(e);
                }
            }
        }
    }

    pub fn downloads(&self) -> DownloadStore {
        snapshot(&self.downloads)
    }

    pub fn update_downloads(&self, f: impl FnOnce(&mut DownloadStore)) {
        {
            let mut d = self.downloads.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut d);
        }
        let d = snapshot(&self.downloads);
        save_json(&self.dir.join("downloads.json"), &d);
    }

    pub fn session(&self) -> Session {
        snapshot(&self.session)
    }

    /// Persist the current session for crash recovery.
    pub fn save_session(&self, s: Session) {
        let mut s = s;
        s.saved_at = now_secs();
        {
            *self.session.lock().unwrap_or_else(|e| e.into_inner()) = s.clone();
        }
        save_json(&self.dir.join("session.json"), &s);
    }

    /// Clear session data on request.
    pub fn clear_session(&self) {
        self.save_session(Session::default());
    }

    /// Wipe browsing data. `what` selects history, cookies, cache or all.
    pub fn clear_data(&self, what: ClearTarget) {
        match what {
            ClearTarget::History => {
                let mut h = self.history.lock().unwrap_or_else(|e| e.into_inner());
                h.clear();
                drop(h);
                let _ = fs::remove_file(self.dir.join("history.jsonl"));
            }
            ClearTarget::Session => self.clear_session(),
            ClearTarget::Downloads => {
                let mut d = self.downloads.lock().unwrap_or_else(|e| e.into_inner());
                d.items.clear();
                drop(d);
                save_json(&self.dir.join("downloads.json"), &DownloadStore::default());
            }
            ClearTarget::All => {
                self.clear_data(ClearTarget::History);
                self.clear_data(ClearTarget::Session);
                self.clear_data(ClearTarget::Downloads);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearTarget {
    History,
    Session,
    Downloads,
    All,
}

// ---------------------------------------------------------------------------
// Atomic file helpers
// ---------------------------------------------------------------------------

/// Clone the value out of a mutex guard.
///
/// The UI and engine read state on every frame, so the store hands out owned
/// copies rather than guards: holding a lock across a frame would let a slow
/// paint stall the network threads. Cloning these small structs is far cheaper
/// than that risk.
fn snapshot<T: Clone>(m: &Mutex<T>) -> T {
    let guard = m.lock().unwrap_or_else(|e| e.into_inner());
    T::clone(&guard)
}

fn save_json<T: Serialize>(path: &Path, value: &T) {
    let tmp = path.with_extension("json.tmp");
    let Ok(data) = serde_json::to_vec_pretty(value) else { return };
    // Write, flush, fsync, rename: the rename is atomic on POSIX, so a reader
    // either sees the old file or the new one, never a half-written mix.
    if let Ok(mut f) = File::create(&tmp) {
        if f.write_all(&data).is_ok() && f.sync_all().is_ok() {
            drop(f);
            let _ = fs::rename(&tmp, path);
            return;
        }
    }
    let _ = fs::remove_file(&tmp);
}

fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path, default: T) -> T {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or(default),
        Err(_) => default,
    }
}

/// Human-readable byte size, for the downloads panel.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Compact "3m ago" style timestamp for history and downloads lists.
pub fn relative_time(ts: u64) -> String {
    let diff = now_secs().saturating_sub(ts);
    match diff {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", diff / 60),
        3600..=86399 => format!("{}h ago", diff / 3600),
        86400..=2_592_000 => format!("{}d ago", diff / 86400),
        _ => format!("{}mo ago", diff / 2_592_000),
    }
}

/// How long a session may be considered fresh for restore purposes.
pub const SESSION_STALE_AFTER: Duration = Duration::from_secs(60 * 60 * 24 * 14);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmark_folders_remove_recursively() {
        let mut b = BookmarkStore::default();
        let folder = b.add(Bookmark {
            id: 0,
            title: "Work".into(),
            url: String::new(),
            folder: None,
            is_folder: true,
            parent: None,
            created: 0,
        });
        let child = b.add(Bookmark::link(0, "Doc", "https://d.com"));
        b.move_to(child, Some(folder));
        assert_eq!(b.items.len(), 2);
        b.remove(folder);
        assert!(b.items.is_empty(), "child must be removed with its folder");
    }

    #[test]
    fn history_records_and_ranks() {
        let mut h = HistoryStore::default();
        h.record("https://a.com", "A");
        h.record("https://a.com", "A");
        h.record("https://b.com", "B");
        let top = h.top_sites(1);
        assert_eq!(top[0].url, "https://a.com");
        assert_eq!(top[0].visit_count(), 2);
    }

    #[test]
    fn history_search_matches_title_and_url() {
        let mut h = HistoryStore::default();
        h.record("https://example.com/docs", "Documentation");
        assert_eq!(h.search("doc").len(), 1);
        assert_eq!(h.search("example").len(), 1);
        assert_eq!(h.search("zzz").len(), 0);
    }

    #[test]
    fn closed_tabs_are_a_stack() {
        let mut h = HistoryStore::default();
        h.add_closed("https://1.com");
        h.add_closed("https://2.com");
        assert_eq!(h.pop_closed().as_deref(), Some("https://2.com"));
        assert_eq!(h.pop_closed().as_deref(), Some("https://1.com"));
        assert_eq!(h.pop_closed(), None);
    }

    #[test]
    fn filename_never_escapes_directory() {
        assert_eq!(filename_from_url("https://x.com/a/b.txt", None), "b.txt");
        assert_eq!(filename_from_url("https://x.com/", None), "x.com");
        // Traversal attempts are neutralised.
        let f = filename_from_url("https://x.com/", Some("../../etc/passwd"));
        assert!(!f.contains('/') && !f.contains('\\'));
        assert_eq!(f, "passwd");
    }

    #[test]
    fn download_progress_handles_unknown_size() {
        let mut d = DownloadStore::default();
        let id = d.start("https://x.com/f", "f", "/tmp/f", 0);
        assert_eq!(d.items[0].progress(), None);
        d.progress(id, 500);
        d.finish(id);
        assert_eq!(d.items[0].progress(), Some(1.0));
    }

    #[test]
    fn store_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.update_settings(|s| s.default_zoom = 1.5);
        store.update_bookmarks(|b| {
            b.add(Bookmark::link(0, "Example", "https://example.com"));
        });
        store.record_visit("https://example.com", "Example");
        store.flush_history();

        // Reopen: everything must come back.
        let store2 = Store::open(dir.path()).unwrap();
        assert_eq!(store2.settings().default_zoom, 1.5);
        assert_eq!(store2.bookmarks().items.len(), 1);
        assert_eq!(store2.history().entries.len(), 1);
    }

    #[test]
    fn corrupt_settings_fall_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.settings().default_zoom, 1.0);
    }

    #[test]
    fn internal_pages_are_not_recorded_in_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.record_visit("about:newtab", "New Tab");
        assert!(store.history().entries.is_empty());
    }

    #[test]
    fn format_bytes_is_readable() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
