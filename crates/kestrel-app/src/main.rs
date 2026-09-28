//! Kestrel browser entry point.
//!
//! Owns the winit window and drives three things per frame, in order:
//!   1. Servo's event loop and painting, so web content advances.
//!   2. The egui pass, which draws the browser chrome.
//!   3. Presentation.
//!
//! Compositing model: Servo renders page content into its own offscreen
//! surface; the active tab is read back into a CPU image and uploaded as a
//! texture that egui draws inside the content rectangle. Chrome and page are
//! therefore composited by a single GPU pass, which keeps the z-order exact,
//! removes any dependency on a shared GL context, and lets the whole browser
//! run on a software adapter when no GPU is present.

mod app;
mod engine;
mod render;

use anyhow::Result;
use app::App;
use engine::EngineHost;
use kestrel_core::adblock::FilterEngine;
use kestrel_core::store::Store;
use std::sync::Arc;
use std::time::Instant;

/// Set by `--screenshot`, used by the automated UI validation. When present the
/// browser writes a PNG of the first stable frame and exits, so a headless CI
/// job can check the interface without a display server.
static SCREENSHOT_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Parse the small set of flags we support. Unknown flags are ignored rather
    // than fatal so the browser still starts if a launcher passes extras.
    let mut screenshot = None;
    let mut profile_dir = None;
    let mut headless_ui = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next(),
            "--profile" => profile_dir = args.next(),
            "--ui-only" => headless_ui = true,
            "--version" | "-V" => {
                println!(
                    "{} {}",
                    kestrel_core::APP_NAME,
                    kestrel_core::APP_VERSION
                );
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = SCREENSHOT_PATH.set(screenshot.clone());

    // Servo requires a process-wide crypto provider before any TLS use.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let dir = match profile_dir {
        Some(p) => std::path::PathBuf::from(p),
        None => Store::default_dir(),
    };
    let store = Arc::new(Store::open(&dir)?);
    let filters = Arc::new(load_filters(&store));
    log::info!(
        "kestrel {} starting: {} network rules, {} cosmetic rules",
        kestrel_core::APP_VERSION,
        filters.rule_count(),
        filters.cosmetic_rule_count()
    );

    let host = EngineHost::new(headless_ui)?;
    let app = App::new(store, filters, &host);
    host.run(app, screenshot)
}

fn print_help() {
    println!(
        "{name} {version} — {tag}\n\n\
USAGE:\n    kestrel [OPTIONS]\n\n\
OPTIONS:\n\
    --profile <DIR>   Use a specific profile directory\n\
    --screenshot <P>  Render one frame to a PNG and exit\n\
    --ui-only         Start without the rendering engine (diagnostics)\n\
    -V, --version     Print the version\n\
    -h, --help        Print this help\n",
        name = kestrel_core::APP_NAME,
        version = kestrel_core::APP_VERSION,
        tag = kestrel_core::APP_TAGLINE
    );
}

/// Build the filter engine from the built-in list plus the user's additions.
fn load_filters(store: &Store) -> FilterEngine {
    let settings = store.settings();
    let mut lines: Vec<String> = builtin_filters();
    lines.extend(settings.custom_filters.iter().cloned());
    // Any allowlist entry becomes an exception rule so the user can unblock a
    // whole site without hunting for individual rules.
    for host in &settings.allowlist {
        lines.push(format!("@@||{host}^"));
    }
    FilterEngine::build(lines)
}

/// The filter list shipped with the browser.
///
/// A production build fetches a maintained list over HTTPS; this embedded set
/// is the offline floor that guarantees blocking works with no network at all.
/// The format is identical, so a downloaded list simply extends it.
fn builtin_filters() -> Vec<String> {
    const RULES: &[&str] = &[
        // --- Common advertising and tracking domains ---
        "||doubleclick.net^",
        "||googlesyndication.com^",
        "||googleadservices.com^",
        "||google-analytics.com^$script",
        "||analytics.google.com^",
        "||scorecardresearch.com^",
        "||quantserve.com^",
        "||outbrain.com^",
        "||taboola.com^",
        "||criteo.com^",
        "||criteo.net^",
        "||adnxs.com^",
        "||rubiconproject.com^",
        "||pubmatic.com^",
        "||openx.net^",
        "||casalemedia.com^",
        "||33across.com^",
        "||adroll.com^",
        "||branch.io^",
        "||amplitude.com^",
        "||segment.io^$script",
        "||segment.com^$script",
        "||mixpanel.com^",
        "||hotjar.com^",
        "||fullstory.com^",
        "||mouseflow.com^",
        "||inspectlet.com^",
        "||optimizely.com^",
        "||vwo.com^",
        "||kissmetrics.com^",
        "||chartbeat.com^",
        "||newrelic.com^",
        "||sentry.io^",
        // --- Social embeds ---
        "||connect.facebook.net^",
        "||platform.twitter.com^",
        "||platform.linkedin.com^",
        "||apis.google.com/js/plusone^",
        "||assets.pinterest.com^$script",
        "||instagram.com/embed^",
        // --- Fingerprinting / device ID ---
        "||fingerprintjs.com^",
        "||fpnpmcdn.net^",
        "||deviceandbrowserinfo.com^",
        "||maxmind.com^$script",
        "||iovation.com^",
        "||threatmetrix.com^",
        "||bluekai.com^",
        "||krxd.net^",
        "||demdex.net^",
        "||everesttech.net^",
        "||agkn.com^",
        "||rlcdn.com^",
        // --- Cosmetic ---
        "##.ad-banner",
        "##.ad-container",
        "##.advertisement",
        "##[id^=\"google_ads_\"]",
        "##[id^=\"div-gpt-ad\"]",
        "##.taboola-container",
        "##.outbrain-widget",
        "##.sticky-footer",
        "##.newsletter-signup-overlay",
    ];
    RULES.iter().map(|s| s.to_string()).collect()
}

/// Kept for symmetry with the shutdown path; `Instant` is used by the FPS meter.
pub type FrameClock = Instant;
