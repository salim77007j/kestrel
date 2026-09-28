//! Network-level and cosmetic content filtering.
//!
//! Design goals, in priority order:
//!  1. Never slow a page down. Matching is a single Aho-Corasick pass over the
//!     URL, so cost is O(url length) regardless of how many rules are loaded.
//!  2. Low memory. Rules are compiled once into a compact automaton; there is no
//!     per-rule heap allocation on the hot path.
//!  3. Predictable. A blocked request is reported with the rule that blocked it,
//!     so the user can audit and allowlist it.

use aho_corasick::{AhoCorasick, MatchKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// What kind of resource the engine is being asked about. Lets us apply
/// different rules to images than to scripts, which is the difference between
/// a usable blocker and one that breaks sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    Document,
    Script,
    Image,
    Stylesheet,
    Font,
    Media,
    Xhr,
    Beacon,
    WebSocket,
    Ping,
    Other,
}

impl ResourceType {
    pub fn from_mime(mime: &str) -> Self {
        let m = mime.to_ascii_lowercase();
        if m.contains("html") {
            ResourceType::Document
        } else if m.contains("javascript") || m.contains("ecmascript") {
            ResourceType::Script
        } else if m.contains("css") {
            ResourceType::Stylesheet
        } else if m.contains("font") || m.contains("woff") {
            ResourceType::Font
        } else if m.contains("image") {
            ResourceType::Image
        } else if m.contains("video") || m.contains("audio") {
            ResourceType::Media
        } else {
            ResourceType::Other
        }
    }
}

/// The decision produced for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Blocked, with the rule that matched and the category for the UI.
    Block(BlockReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockReason {
    pub rule: String,
    pub category: Category,
    /// Whether the page can be told *why* it was blocked, or should just see a
    /// silent failure (a silent failure is harder for trackers to fingerprint).
    pub disclosed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Advertising,
    Tracking,
    Analytics,
    Annoyance,
    Social,
    CookieNotice,
    Malicious,
    Custom,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Advertising => "Advertising",
            Category::Tracking => "Tracking",
            Category::Analytics => "Analytics",
            Category::Annoyance => "Annoyance",
            Category::Social => "Social",
            Category::CookieNotice => "Cookie notice",
            Category::Malicious => "Malicious",
            Category::Custom => "Custom",
        }
    }
}

/// One compiled rule.
#[derive(Debug, Clone)]
struct CompiledRule {
    /// Human-readable form, kept for the "what blocked this?" UI.
    source: String,
    category: Category,
    /// `Some(types)` restricts the rule to those resource types.
    types: Option<Vec<ResourceType>>,
    /// For anchored domain rules, the hostname suffix this rule covers.
    domain: Option<String>,
    /// Exceptions (`@@`) override blocking rules.
    exception: bool,
    /// Cosmetic rules carry a CSS snippet instead of matching URLs.
    cosmetic: bool,
    /// The literal substrings this rule matches on, one automaton pattern each.
    /// Kept here so the engine can register every rule's patterns in a single
    /// pass and look the rule up again from the match id.
    needles: Vec<String>,
}

/// The compiled filter set. Immutable once built, shared behind an `Arc`, so the
/// network threads can match against it concurrently with zero locking.
pub struct FilterEngine {
    ac: AhoCorasick,
    rules: Vec<CompiledRule>,
    /// Map from automaton pattern id to rule index.
    id_to_rule: HashMap<u32, usize>,
    /// Needles that have an exception rule. Checked separately from the
    /// automaton: a block rule and its `@@` exception usually compile to the
    /// *same* needle, and Aho-Corasick reports non-overlapping matches, so a
    /// single automaton would report whichever pattern id came first and could
    /// silently drop the exception. An explicit set removes that ambiguity.
    exception_needles: std::collections::HashSet<String>,
    /// Cosmetic CSS keyed by the document hostname.
    cosmetic: HashMap<String, Vec<String>>,
    cosmetic_generic: Vec<String>,
    /// Hosts explicitly allowed, which bypass matching entirely.
    allowlist: Vec<String>,
    stats: parking_lot_lite::Mutex<Stats>,
}

/// Tiny mutex shim so the core crate stays dependency-light; we only ever hold
/// it for a few integer increments on the stats path.
pub mod parking_lot_lite {
    use std::sync::Mutex as StdMutex;
    use std::sync::MutexGuard;

    #[derive(Default, Debug)]
    pub struct Stats {
        pub requests: u64,
        pub blocked: u64,
    }

    /// Contention is negligible: only counters are touched, never the filter
    /// data, and the critical section is a couple of increments.
    pub struct Mutex<T>(StdMutex<T>);

    impl<T> Mutex<T> {
        pub fn new(v: T) -> Self {
            Self(StdMutex::new(v))
        }
        pub fn lock(&self) -> MutexGuard<'_, T> {
            // A poisoned stats lock must never take the browser down.
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }
    }
}

pub use parking_lot_lite::{Mutex, Stats};

impl FilterEngine {
    /// Build an engine from a set of filter lines in AdGuard/ABP syntax.
    ///
    /// Unsupported syntax is skipped rather than fatal: a single malformed line
    /// in a downloaded list must never be able to disable blocking entirely.
    pub fn build(lines: impl IntoIterator<Item = String>) -> Arc<Self> {
        let mut rules = Vec::new();
        let mut patterns: Vec<String> = Vec::new();
        let mut id_to_rule: HashMap<u32, usize> = HashMap::new();
        let mut exception_needles: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut cosmetic: HashMap<String, Vec<String>> = HashMap::new();
        let mut cosmetic_generic: Vec<String> = Vec::new();
        let mut allowlist: Vec<String> = Vec::new();

        let mut next_pattern_id = 0u32;
        for line in lines {
            let line = line.trim();
            // `!` is a comment in AdGuard syntax. `#` is *also* a comment, except
            // for a leading `##`, which introduces a cosmetic rule and must not
            // be discarded here.
            if line.is_empty() || line.starts_with('!') {
                continue;
            }
            if line.starts_with('#') && !line.starts_with("##") {
                continue;
            }
            match self::parse_rule(line) {
                ParsedRule::Cosmetic { selector, domain } => {
                    match domain {
                        Some(d) => cosmetic.entry(d).or_default().push(selector),
                        None => cosmetic_generic.push(selector),
                    }
                }
                ParsedRule::AllowHost(host) => {
                    allowlist.push(host.to_ascii_lowercase());
                }
                ParsedRule::Network(r) => {
                    let idx = rules.len();
                    let needles = r.needles.clone();
                    let mut entry = r;
                    entry.needles = Vec::new();
                    // Register one automaton pattern per needle; the match id
                    // maps back to this rule index. Exception needles are not
                    // registered: they are consulted separately so that a block
                    // and its exception cannot collide in the automaton.
                    let mut registered = false;
                    for needle in needles {
                        let lowered = needle.to_ascii_lowercase();
                        if lowered.is_empty() {
                            continue;
                        }
                        if entry.exception {
                            exception_needles.insert(lowered);
                            continue;
                        }
                        id_to_rule.insert(next_pattern_id, idx);
                        next_pattern_id += 1;
                        patterns.push(lowered);
                        registered = true;
                    }
                    if registered {
                        rules.push(entry);
                    }
                }
                ParsedRule::Unsupported => {}
            }
        }

        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .start_kind(aho_corasick::StartKind::Both)
            .prefilter(true)
            .build(&patterns)
            .expect("static pattern set must compile");

        Arc::new(Self {
            ac,
            rules,
            id_to_rule,
            exception_needles,
            cosmetic,
            cosmetic_generic,
            allowlist,
            stats: parking_lot_lite::Mutex::new(Stats::default()),
        })
    }

    /// An engine that blocks nothing. Used when the user disables the blocker
    /// and in tests, so the rest of the code never has to special-case `None`.
    pub fn passthrough() -> Arc<Self> {
        Self::build(Vec::new())
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn cosmetic_rule_count(&self) -> usize {
        self.cosmetic.values().map(|v| v.len()).sum::<usize>() + self.cosmetic_generic.len()
    }

    /// Decide whether a request should be allowed.
    pub fn check(&self, url: &str, resource: ResourceType) -> Decision {
        self.stats.lock().requests += 1;

        if self.allowlist.iter().any(|h| url.contains(h.as_str())) {
            return Decision::Allow;
        }

        let hay = url.to_ascii_lowercase();
        let mut blocked: Option<BlockReason> = None;

        for m in self.ac.find_iter(&hay) {
            let Some(&rule_idx) = self.id_to_rule.get(&(m.pattern().as_usize() as u32)) else {
                continue;
            };
            let rule = &self.rules[rule_idx];
            if let Some(types) = &rule.types {
                if !types.contains(&resource) {
                    continue;
                }
            }
            // A domain rule only applies when the URL is on that domain.
            if let Some(domain) = &rule.domain {
                if !hay.contains(domain.as_str()) {
                    continue;
                }
            }
            // If an exception rule covers this exact needle, the request is
            // explicitly allowlisted and must not be blocked.
            if self.exception_needles.contains(&hay[m.start()..m.end()].to_string()) {
                return Decision::Allow;
            }
            blocked = Some(BlockReason {
                rule: rule.source.clone(),
                category: rule.category,
                disclosed: false,
            });
        }

        match blocked {
            Some(reason) => {
                self.stats.lock().blocked += 1;
                Decision::Block(reason)
            }
            None => Decision::Allow,
        }
    }

    /// CSS to inject into a document to hide cosmetic-only elements.
    pub fn cosmetic_css_for(&self, host: &str) -> String {
        let host = host.to_ascii_lowercase();
        let mut out = String::new();
        for sel in &self.cosmetic_generic {
            push_rule(&mut out, sel);
        }
        // A rule registered for a parent domain applies to its subdomains.
        for (domain, sels) in &self.cosmetic {
            if host == *domain || host.ends_with(&format!(".{domain}")) {
                for sel in sels {
                    push_rule(&mut out, sel);
                }
            }
        }
        out
    }

    /// Requests seen and requests blocked since the last reset.
    pub fn stats(&self) -> (u64, u64) {
        let s = self.stats.lock();
        (s.requests, s.blocked)
    }

    pub fn reset_stats(&self) {
        let mut s = self.stats.lock();
        s.requests = 0;
        s.blocked = 0;
    }
}

fn push_rule(out: &mut String, selector: &str) {
    // Class-based rules use a leading dot; attribute rules use `[`.
    let sel = selector.trim().trim_start_matches(['#', '.', '[']);
    if sel.is_empty() {
        return;
    }
    out.push_str(&format!("{selector}{{display:none!important}}\n"));
}

enum ParsedRule {
    Cosmetic { selector: String, domain: Option<String> },
    AllowHost(String),
    Network(CompiledRule),
    Unsupported,
}

/// Parse one filter line. Kept separate and pure so it can be tested directly.
fn parse_rule(line: &str) -> ParsedRule {
    let mut rest = line;
    let mut exception = false;
    if let Some(r) = rest.strip_prefix("@@") {
        exception = true;
        rest = r;
    }
    if rest.contains("##") {
        // Cosmetic rule. Two accepted shapes:
        //   `##selector`      — hide this selector on every site
        //   `domain##selector`— hide this selector on that domain (and subdomains)
        //   `selector##domain`— the ABP "extended" order, accepted for
        //                      compatibility with downloaded lists
        let (left, right) = rest.split_once("##").unwrap();
        // A domain never starts with a selector sigil, and a selector never
        // contains a bare domain-looking label, so the sigil tells us which
        // side is which. Anything ambiguous is treated as generic.
        let starts_with_sigil = |s: &str| s.starts_with('.') || s.starts_with('#') || s.starts_with('[');
        return match (left.is_empty(), right.is_empty()) {
            // `##selector`
            (true, false) => ParsedRule::Cosmetic { selector: right.to_string(), domain: None },
            // `domain##`
            (false, true) => ParsedRule::Cosmetic { selector: left.to_string(), domain: None },
            _ => {
                if starts_with_sigil(left) && !starts_with_sigil(right) {
                    ParsedRule::Cosmetic {
                        selector: left.to_string(),
                        domain: Some(right.split(',').next().unwrap_or(right).to_string()),
                    }
                } else if !starts_with_sigil(left) && starts_with_sigil(right) {
                    ParsedRule::Cosmetic {
                        selector: right.to_string(),
                        domain: Some(left.split(',').next().unwrap_or(left).to_string()),
                    }
                } else {
                    // Both or neither look like selectors: apply globally, which
                    // is the conservative reading of an ambiguous rule.
                    ParsedRule::Cosmetic { selector: format!("{left}{right}"), domain: None }
                }
            }
        };
    }
    // `||example.com^` whole-domain rule: the domain itself is the needle, so we
    // do not need the separator tail to match anything.
    if let Some(r) = rest.strip_prefix("||") {
        let (domain, _) = split_domain_rule(r);
        let domain = domain.to_ascii_lowercase();
        if domain.is_empty() {
            return ParsedRule::Unsupported;
        }
        return ParsedRule::Network(CompiledRule {
            source: line.to_string(),
            category: Category::Advertising,
            types: None,
            domain: Some(domain.clone()),
            exception,
            cosmetic: false,
            needles: vec![domain],
        });
    }

    // Option types: `$script,image`
    let (pattern, options) = match rest.split_once('$') {
        Some((p, o)) => (p, Some(o)),
        None => (rest, None),
    };
    if pattern.is_empty() {
        return ParsedRule::Unsupported;
    }
    let types = options.and_then(|o| {
        let t: Vec<ResourceType> = o
            .split(',')
            .filter_map(|opt| match opt.trim() {
                "script" => Some(ResourceType::Script),
                "image" => Some(ResourceType::Image),
                "stylesheet" => Some(ResourceType::Stylesheet),
                "font" => Some(ResourceType::Font),
                "media" => Some(ResourceType::Media),
                "xmlhttprequest" | "xhr" => Some(ResourceType::Xhr),
                "ping" | "beacon" => Some(ResourceType::Ping),
                "websocket" => Some(ResourceType::WebSocket),
                "subdocument" => Some(ResourceType::Document),
                "document" | "doc" => Some(ResourceType::Document),
                _ => None,
            })
            .collect();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });

    // Strip separator and wildcard characters to get a literal needle.
    let needle: String = pattern
        .chars()
        .filter(|c| !matches!(c, '^' | '*' | '/' | '|'))
        .collect();
    if needle.len() < 4 {
        // Very short needles produce false positives on ordinary URLs; a real
        // filter list never needs them, so drop rather than block randomly.
        return ParsedRule::Unsupported;
    }

    ParsedRule::Network(CompiledRule {
        source: line.to_string(),
        category: Category::Advertising,
        types,
        domain: None,
        exception,
        cosmetic: false,
        needles: vec![needle],
    })
}

fn split_domain_rule(rest: &str) -> (&str, &str) {
    match rest.find(['/', '^', '*']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eng(lines: &[&str]) -> Arc<FilterEngine> {
        FilterEngine::build(lines.iter().map(|s| s.to_string()))
    }

    #[test]
    fn blocks_ad_server() {
        let e = eng(&["||ads.example.com^"]);
        let d = e.check("https://ads.example.com/banner.gif", ResourceType::Image);
        assert!(matches!(d, Decision::Block(_)));
    }

    #[test]
    fn allows_normal_traffic() {
        let e = eng(&["||ads.example.com^"]);
        assert_eq!(e.check("https://example.com/a.png", ResourceType::Image), Decision::Allow);
    }

    #[test]
    fn exception_overrides_block() {
        let e = eng(&["||tracker.net^", "@@||tracker.net^$domain=example.com"]);
        // Without the domain restriction the exception still wins for this rule.
        let d = e.check("https://tracker.net/beacon", ResourceType::Xhr);
        assert_eq!(d, Decision::Allow);
    }

    #[test]
    fn type_restriction_respected() {
        let e = eng(&["/pixel.gif$image"]);
        assert!(matches!(
            e.check("https://x.com/pixel.gif", ResourceType::Image),
            Decision::Block(_)
        ));
        assert_eq!(
            e.check("https://x.com/pixel.gif", ResourceType::Script),
            Decision::Allow
        );
    }

    #[test]
    fn cosmetic_rules_produce_css() {
        let e = eng(&[".ad-banner##", "##.sticky-footer"]);
        let css = e.cosmetic_css_for("news.example.com");
        assert!(css.contains("display:none"));
        assert!(css.contains("sticky-footer"));
    }

    #[test]
    fn domain_scoped_cosmetic() {
        let e = eng(&[".promo##example.com"]);
        assert!(e.cosmetic_css_for("example.com").contains("promo"));
        assert!(!e.cosmetic_css_for("other.com").contains("promo"));
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let e = eng(&["! comment", "", "   ", "# another"]);
        assert_eq!(e.rule_count(), 0);
        assert_eq!(e.check("https://ads.example.com", ResourceType::Image), Decision::Allow);
    }

    #[test]
    fn short_patterns_rejected_to_avoid_false_positives() {
        let e = eng(&["/a$"]);
        assert_eq!(e.rule_count(), 0);
    }

    #[test]
    fn stats_track_blocked() {
        let e = eng(&["||ads.example.com^"]);
        let _ = e.check("https://ads.example.com/1", ResourceType::Image);
        let _ = e.check("https://ok.example.com/1", ResourceType::Image);
        let (req, blocked) = e.stats();
        assert_eq!(req, 2);
        assert_eq!(blocked, 1);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let e = eng(&["||ads.example.com^"]);
        assert!(matches!(
            e.check("https://ADS.Example.COM/x", ResourceType::Image),
            Decision::Block(_)
        ));
    }
}
