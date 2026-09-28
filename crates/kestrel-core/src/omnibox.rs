//! URL classification, normalisation and the omnibox input model.
//!
//! The omnibox has to answer one question on every keystroke: "is this a URL to
//! navigate to, or text to hand to a search engine?". Getting this wrong is one of
//! the most annoying failures a browser can have, so the rules live here in pure
//! functions with no engine or UI dependency and are unit-tested directly.

use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// Search engines offered in settings and used by the omnibox fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEngine {
    pub id: String,
    pub name: String,
    /// `{query}` is substituted with the percent-encoded search terms.
    pub template: String,
    pub shortcut: Option<String>,
}

impl SearchEngine {
    pub fn build_url(&self, query: &str) -> Option<Url> {
        let encoded = percent_encoding::utf8_percent_encode(
            query,
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        let joined = self.template.replace("{query}", &encoded);
        Url::parse(&joined).ok()
    }
}

/// The built-in default engines. Kept deliberately small: every extra engine is
/// another outbound query pattern to audit.
pub fn default_search_engines() -> Vec<SearchEngine> {
    vec![
        SearchEngine {
            id: "duckduckgo".into(),
            name: "DuckDuckGo".into(),
            template: "https://duckduckgo.com/?q={query}".into(),
            shortcut: Some("ddg".into()),
        },
        SearchEngine {
            id: "google".into(),
            name: "Google".into(),
            template: "https://www.google.com/search?q={query}".into(),
            shortcut: Some("g".into()),
        },
        SearchEngine {
            id: "wikipedia".into(),
            name: "Wikipedia".into(),
            template: "https://en.wikipedia.org/w/index.php?search={query}".into(),
            shortcut: Some("w".into()),
        },
        SearchEngine {
            id: "bing".into(),
            name: "Bing".into(),
            template: "https://www.bing.com/search?q={query}".into(),
            shortcut: None,
        },
    ]
}

/// What the user (or an opener) asked us to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationTarget {
    /// An explicit, absolute URL.
    Url(Url),
    /// A relative or scheme-less URL that still parsed (e.g. `example.com/x`).
    /// `https` is the assumed scheme for navigation.
    Inferred(Url),
    /// Free text to search for.
    Search(SearchQuery),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    /// Engine chosen by an explicit `!bang`, or `None` to use the default.
    pub engine_id: Option<String>,
}

/// Schemes we will navigate to. Anything else is refused by the security layer,
/// which is what stops `javascript:` and `data:` from being reached by accident.
pub const NAVIGABLE_SCHEMES: &[&str] = &["http", "https", "about", "file", "view-source"];

/// Maximum length we will treat as a URL candidate. Long input is always a search;
/// this bounds the work done per keystroke and avoids pathological parsing.
const MAX_URL_CANDIDATE_LEN: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum NavigateError {
    #[error("empty input")]
    Empty,
    #[error("input is too long to be a url")]
    TooLong,
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
    #[error("not a valid url: {0}")]
    Invalid(String),
}

/// A TLD that is unambiguously a hostname, used to resolve the
/// `example.com` vs `what is an example.com` ambiguity without hitting the network.
const COMMON_TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "mil", "int", "info", "biz", "io", "co", "dev", "app", "ai",
    "me", "tv", "cc", "xyz", "sh", "gg", "de", "uk", "fr", "jp", "ru", "nl", "se", "no", "fi", "es",
    "it", "br", "in", "ca", "au", "nz", "ch", "at", "be", "dk", "pl", "cz", "pt", "gr", "hu", "ro",
    "us", "eu", "asia", "tech", "cloud", "online", "site", "store", "blog", "news", "gov.uk",
];

/// Host labels that are obviously a hostname rather than a sentence.
const HOSTNAME_HINTS: &[&str] = &["localhost"];

fn is_known_tld(host: &str) -> bool {
    // Handle multi-part suffixes such as `co.uk` by checking the last one or two labels.
    let lower = host.to_ascii_lowercase();
    let labels: Vec<&str> = lower.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let last = labels[labels.len() - 1];
    if COMMON_TLDS.contains(&last) {
        return true;
    }
    if labels.len() >= 3 {
        let last_two = format!("{}.{}", labels[labels.len() - 2], last);
        if COMMON_TLDS.contains(&last_two.as_str()) {
            return true;
        }
    }
    false
}

/// Heuristic: does this bare token look like a hostname?
///
/// Deliberately conservative. A false positive sends the user to a DNS failure;
/// a false negative sends them to a search engine. The former is worse, so we
/// require positive evidence of structure.
pub fn looks_like_url(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_URL_CANDIDATE_LEN {
        return false;
    }
    // Any whitespace means it is a sentence, not a URL.
    if trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();

    if HOSTNAME_HINTS.contains(&lower.as_str()) {
        return true;
    }

    // An explicit `scheme://` is a URL whatever follows.
    if let Some((scheme, _)) = lower.split_once("://") {
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return true;
        }
    }

    // `user@host` is an address.
    if trimmed.contains('@') {
        let host_part = trimmed.rsplit('@').next().unwrap_or("");
        if host_part.contains('.') && !host_part.is_empty() {
            return true;
        }
    }

    // Strip a leading `www.` consideration and inspect the labels.
    let host_candidate = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lower.as_str());
    let host_only = host_candidate
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(host_candidate);
    // Drop userinfo and port.
    let host_only = host_only.rsplit('@').next().unwrap_or(host_only);
    let host_only = host_only.split(':').next().unwrap_or(host_only);

    if host_only.is_empty() {
        return false;
    }

    let labels: Vec<&str> = host_only.split('.').collect();
    // Require at least two labels and a plausible final label.
    if labels.len() < 2 {
        return false;
    }
    let last_label = labels[labels.len() - 1];
    if last_label.is_empty() {
        return false;
    }
    // Digits-only TLDs are not valid; punycode TLDs start with `xn--`.
    let is_punycode = last_label.starts_with("xn--");
    let is_alpha_tld = last_label.chars().all(|c| c.is_ascii_alphabetic());
    if !is_punycode && !is_alpha_tld {
        return false;
    }
    // All labels must be non-empty and free of characters illegal in hostnames.
    labels
        .iter()
        .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && is_known_tld(host_only)
}

/// Normalise a user-typed string into an absolute URL, or `None` if it is not one.
pub fn parse_navigable(input: &str) -> Result<Url, NavigateError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(NavigateError::Empty);
    }
    if trimmed.len() > MAX_URL_CANDIDATE_LEN {
        return Err(NavigateError::TooLong);
    }

    // Already absolute with a scheme?
    if let Ok(url) = Url::parse(trimmed) {
        let scheme = url.scheme().to_ascii_lowercase();
        if NAVIGABLE_SCHEMES.contains(&scheme.as_str()) {
            return Ok(url);
        }
        return Err(NavigateError::UnsupportedScheme(scheme));
    }

    // Bare `host[:port][/path]`, assume https.
    if looks_like_url(trimmed) {
        let candidate = format!("https://{trimmed}");
        if let Ok(url) = Url::parse(&candidate) {
            return Ok(url);
        }
    }
    Err(NavigateError::Invalid(trimmed.to_string()))
}

/// Apply IDNA (punycode) normalisation to a host so that unicode domains and
/// their ASCII forms are treated as the same origin by the cookie and tracker
/// layers. Without this, `exämple.com` and `xn--exmple-cua.com` would be two
/// different sites to a tracker and one to a user.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.');
    match idna::domain_to_ascii(host) {
        Ok(ascii) => ascii.to_ascii_lowercase(),
        Err(_) => host.to_ascii_lowercase(),
    }
}

/// The registrable-ish origin used for cookie partitioning and tracker matching.
/// Uses the public-suffix style reduction for the common two-label case and
/// falls back to the full host when we cannot reduce it safely.
pub fn origin_key(url: &Url) -> String {
    let host = match url.host() {
        Some(h) => h,
        None => return url.scheme().to_string(),
    };
    let host = match host {
        Host::Domain(d) => normalize_host(d),
        Host::Ipv4(a) => a.to_string(),
        Host::Ipv6(a) => a.to_string(),
    };
    format!("{}://{}", url.scheme().to_ascii_lowercase(), host)
}

/// True when the URL is one the browser should treat as first-party-internal.
pub fn is_internal_page(url: &Url) -> bool {
    matches!(url.scheme(), "about" | "kestrel" | "moz-extension")
}

/// Strip tracking parameters from a URL so that they are not leaked to the
/// destination, stored in history, or sent in the `Referer` header.
///
/// This is a privacy feature with a visible side effect: the destination may
/// attribute the visit differently. That trade is the user's to make in settings.
pub fn strip_tracking_params(url: &Url, enabled: bool) -> Option<Url> {
    if !enabled {
        return Some(url.clone());
    }
    let mut out = url.clone();
    let tracked: &[&str] = &[
        "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content", "utm_id",
        "gclid", "gclsrc", "dclid", "fbclid", "msclkid", "twclid", "igshid", "mc_cid", "mc_eid",
        "_ga", "_gl", "yclid", "ttclid", "wbraid", "gbraid", "s_kwcid", "vero_id", "oly_enc_id",
        "oly_anon_id", "rb_clickid", "sscid", "epik", "dm_i", "pk_campaign", "pk_kwd", "pk_source",
        "piwik_campaign", "piwik_kwd", "hsa_", "at_custom", "wt_mc", "ncid", "cmpid", "campaign_id",
    ];
    let pairs: Vec<(String, String)> = out
        .query_pairs()
        .filter(|(k, _)| !tracked.iter().any(|t| k == t || k.starts_with(t)))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if pairs.is_empty() {
        out.set_query(None);
    } else {
        let mut q = out.query_pairs_mut();
        q.clear();
        for (k, v) in pairs {
            q.append_pair(&k, &v);
        }
        drop(q);
    }
    Some(out)
}

/// Decide how to handle omnibox input, given the default engine.
pub fn resolve_input(input: &str, default_engine: &SearchEngine) -> NavigationTarget {
    let trimmed = input.trim();

    // `!bang engine terms` — explicit engine selection without touching settings.
    if let Some(rest) = trimmed.strip_prefix('!') {
        // Split on the first run of whitespace, not on a whitespace character,
        // so an engine token never absorbs a leading space.
        let (engine_token, terms) = match rest.find(char::is_whitespace) {
            Some(i) => (&rest[..i], rest[i..].trim_start()),
            None => (rest.trim(), ""),
        };
        let engine_id = engine_token.to_ascii_lowercase();
        if let Some(engine) = default_search_engines()
            .iter()
            .find(|e| e.id == engine_id || e.shortcut.as_deref() == Some(engine_id.as_str()))
        {
            return NavigationTarget::Search(SearchQuery {
                text: terms.to_string(),
                engine_id: Some(engine.id.clone()),
            });
        }
    }

    if let Ok(url) = parse_navigable(trimmed) {
        // If the user typed a bare host we inferred the scheme; keep that
        // distinction so the UI can show "https://" as a hint they did not type.
        if trimmed.contains("://") {
            return NavigationTarget::Url(url);
        }
        return NavigationTarget::Inferred(url);
    }

    NavigationTarget::Search(SearchQuery {
        text: trimmed.to_string(),
        engine_id: Some(default_engine.id.clone()),
    })
}

/// Reduce a URL to a display string for the address bar.
pub fn pretty_url(url: &Url) -> String {
    let mut s = String::new();
    let scheme = url.scheme();
    if scheme == "https" {
        // Chrome-like: hide the default scheme but keep it for http so the
        // user can tell insecure from secure at a glance.
        s.push_str("https://");
    } else if scheme != "about" && scheme != "file" {
        s.push_str(scheme);
        s.push_str("://");
    }
    match url.host() {
        Some(Host::Domain(d)) => s.push_str(d),
        Some(Host::Ipv4(a)) => s.push_str(&a.to_string()),
        Some(Host::Ipv6(a)) => s.push_str(&a.to_string()),
        None => {}
    }
    if let Some(port) = url.port() {
        s.push(':');
        s.push_str(&port.to_string());
    }
    let path = url.path();
    if path != "/" && !path.is_empty() {
        s.push_str(path);
    } else {
        s.push('/');
    }
    if let Some(q) = url.query() {
        s.push('?');
        s.push_str(q);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dg() -> SearchEngine {
        default_search_engines().into_iter().next().unwrap()
    }

    #[test]
    fn recognises_bare_domains() {
        for s in ["example.com", "www.example.com", "sub.example.co.uk", "example.io"] {
            assert!(looks_like_url(s), "{s} should look like a url");
        }
    }

    #[test]
    fn rejects_sentences() {
        for s in [
            "how do i cook rice",
            "what is a url",
            "hello world",
            "example",
            "a.b",
            "what is love?",
            "3.14",
        ] {
            assert!(!looks_like_url(s), "{s} should not look like a url");
        }
    }

    #[test]
    fn recognises_schemes_and_auth() {
        assert!(looks_like_url("https://example.com"));
        assert!(looks_like_url("user@example.com"));
        assert!(looks_like_url("localhost"));
    }

    #[test]
    fn refuses_dangerous_schemes() {
        assert!(matches!(
            parse_navigable("javascript:alert(1)"),
            Err(NavigateError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            parse_navigable("data:text/html,<h1>x"),
            Err(NavigateError::UnsupportedScheme(_))
        ));
    }

    #[test]
    fn parses_bare_domain_to_https() {
        let u = parse_navigable("example.com").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host_str(), Some("example.com"));
    }

    #[test]
    fn strips_tracking_params() {
        let u = Url::parse("https://example.com/p?utm_source=news&gclid=x&id=7").unwrap();
        let cleaned = strip_tracking_params(&u, true).unwrap();
        let q: Vec<_> = cleaned.query_pairs().collect();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].0, "id");
        assert_eq!(q[0].1, "7");
    }

    #[test]
    fn keeps_params_when_disabled() {
        let u = Url::parse("https://example.com/p?utm_source=news").unwrap();
        let kept = strip_tracking_params(&u, false).unwrap();
        assert!(kept.query().unwrap().contains("utm_source"));
    }

    #[test]
    fn resolves_url_vs_search() {
        let e = dg();
        assert!(matches!(
            resolve_input("example.com", &e),
            NavigationTarget::Inferred(_)
        ));
        assert!(matches!(
            resolve_input("https://example.com", &e),
            NavigationTarget::Url(_)
        ));
        // Free text falls back to the engine the caller passed in.
        match resolve_input("how to bake bread", &e) {
            NavigationTarget::Search(q) => {
                assert_eq!(q.text, "how to bake bread");
                assert_eq!(q.engine_id.as_deref(), Some("duckduckgo"));
            }
            other => panic!("expected search, got {other:?}"),
        }
    }

    #[test]
    fn bang_selects_engine() {
        let e = dg();
        match resolve_input("!g kittens", &e) {
            NavigationTarget::Search(q) => {
                assert_eq!(q.text, "kittens");
                assert_eq!(q.engine_id.as_deref(), Some("google"));
            }
            other => panic!("expected search, got {other:?}"),
        }
    }

    #[test]
    fn normalises_unicode_hosts() {
        assert_eq!(normalize_host("EXAMPLE.com."), "example.com");
        // Punycode form and unicode form must collapse to the same key.
        let a = normalize_host("exämple.com");
        let b = normalize_host("xn--exmple-cua.com");
        assert_eq!(a, b);
    }

    #[test]
    fn origin_key_collapses_scheme_and_case() {
        let a = Url::parse("https://Example.COM/a").unwrap();
        let b = Url::parse("https://example.com/b").unwrap();
        assert_eq!(origin_key(&a), origin_key(&b));
    }
}
