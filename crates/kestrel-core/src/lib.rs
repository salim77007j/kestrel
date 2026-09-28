//! Kestrel browser core.
//!
//! Everything in this crate is engine-independent and UI-independent by design.
//! It holds the logic that defines the product: URL handling, the filter engine,
//! privacy policy and the persistent stores. Keeping it free of Servo and egui
//! means it compiles in seconds and can be tested exhaustively in CI, which is
//! where correctness of the privacy logic actually gets proven.

pub mod adblock;
pub mod omnibox;
pub mod privacy;
pub mod store;

pub use adblock::{Category, Decision, FilterEngine, ResourceType};
pub use omnibox::{
    default_search_engines, looks_like_url, parse_navigable, pretty_url, resolve_input,
    NavigationTarget, SearchEngine, SearchQuery,
};
pub use privacy::{Defense, Level, PrivacyConfig};
pub use store::{Bookmark, Download, HistoryEntry, Store};

/// The application identity. Shown in the UI, the user agent and about pages.
pub const APP_NAME: &str = "Kestrel";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_TAGLINE: &str = "Simple. Fast. Yours.";

/// Major sites offered on the new-tab page. Kept as configuration so the user
/// can remove them; nothing here is injected into the page by default.
pub fn default_shortcuts() -> Vec<(String, String)> {
    vec![
        ("Google".into(), "https://www.google.com/".into()),
        ("YouTube".into(), "https://www.youtube.com/".into()),
        ("Gmail".into(), "https://mail.google.com/".into()),
        ("Drive".into(), "https://drive.google.com/".into()),
    ]
}
