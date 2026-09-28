//! Kestrel browser chrome.
//!
//! The UI is split into widgets (`tabs`, `toolbar`, `tab_strip`, `new_tab`,
//! `pages`) that each return an *intent* struct describing what the user asked
//! for. Nothing here mutates browser state directly. That inversion is what
//! keeps the widgets pure enough to reason about, and it makes it structurally
//! impossible to ship a control that is not connected to a real action: an
//! intent that nobody handles is a compile-visible dead branch, not a silent
//! no-op button.

pub mod icons;
pub mod new_tab;
pub mod pages;
pub mod tab_strip;
pub mod tabs;
pub mod theme;
pub mod toolbar;

pub use theme::{metrics, space, Palette};
