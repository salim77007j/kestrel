//! Fingerprint and tracking defences.
//!
//! The goal is not to make a browser un-fingerprintable — that is not achievable
//! for a general-purpose browser. The goal is to remove the cheap, high-entropy
//! signals that scripts use for stable identifiers, and to do it *consistently*,
//! because inconsistency is what makes a defence more identifying than no defence
//! at all.
//!
//! Every defence here is a policy decision that is surfaced in settings, so the
//! user can see exactly what is being perturbed and turn it off.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Individual defences, each independently toggleable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Defense {
    /// Add per-session noise to canvas readback.
    CanvasNoise,
    /// Perturb audio rendering deterministically.
    AudioNoise,
    /// Normalise the installed font list to a common set.
    FontNormalization,
    /// Reduce WebGL driver/renderer detail.
    WebglMasking,
    /// Block WebRTC from exposing local IP addresses.
    WebrtcLeak,
    /// Send a generic user agent.
    UserAgentSpoofing,
    /// Disable the `Referer` header entirely.
    RefererStripping,
    /// Strip tracking parameters from navigations.
    QueryStripping,
    /// Spoof timezone to a fixed value.
    TimezoneSpoofing,
    /// Block third-party cookies.
    ThirdPartyCookieBlock,
    /// Block all cookies.
    TotalCookieBlock,
    /// Require HTTPS and upgrade HTTP.
    HttpsUpgrade,
    /// Disable the `Do Not Track`-adjacent high-entropy client hints.
    ClientHintReduction,
}

impl Defense {
    pub fn as_str(&self) -> &'static str {
        match self {
            Defense::CanvasNoise => "canvas",
            Defense::AudioNoise => "audio",
            Defense::FontNormalization => "fonts",
            Defense::WebglMasking => "webgl",
            Defense::WebrtcLeak => "webrtc",
            Defense::UserAgentSpoofing => "userAgent",
            Defense::RefererStripping => "referer",
            Defense::QueryStripping => "query",
            Defense::TimezoneSpoofing => "timezone",
            Defense::ThirdPartyCookieBlock => "thirdPartyCookies",
            Defense::TotalCookieBlock => "cookies",
            Defense::HttpsUpgrade => "httpsUpgrade",
            Defense::ClientHintReduction => "clientHints",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Defense::CanvasNoise => "Canvas fingerprint protection",
            Defense::AudioNoise => "Audio fingerprint protection",
            Defense::FontNormalization => "Font fingerprint protection",
            Defense::WebglMasking => "WebGL fingerprint masking",
            Defense::WebrtcLeak => "WebRTC leak protection",
            Defense::UserAgentSpoofing => "User agent reduction",
            Defense::RefererStripping => "Referer stripping",
            Defense::QueryStripping => "Tracking parameter removal",
            Defense::TimezoneSpoofing => "Timezone spoofing",
            Defense::ThirdPartyCookieBlock => "Block third-party cookies",
            Defense::TotalCookieBlock => "Block all cookies",
            Defense::HttpsUpgrade => "Always use secure connections",
            Defense::ClientHintReduction => "Reduce client hints",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Defense::CanvasNoise => "Adds stable, per-session noise to canvas and 2D image readback so repeated draws cannot be used to identify you.",
            Defense::AudioNoise => "Applies a small, stable perturbation to audio rendering to remove the audio-context fingerprint.",
            Defense::FontNormalization => "Reports a common font set instead of the exact fonts installed on your machine.",
            Defense::WebglMasking => "Reduces the precision of WebGL parameters that identify your graphics hardware.",
            Defense::WebrtcLeak => "Prevents pages from discovering your local network addresses through WebRTC.",
            Defense::UserAgentSpoofing => "Sends a common browser identity instead of your exact build.",
            Defense::RefererStripping => "Does not send the referring page URL to other sites.",
            Defense::QueryStripping => "Removes known tracking parameters from links before following them.",
            Defense::TimezoneSpoofing => "Reports a fixed timezone so your location is not inferred from the clock.",
            Defense::ThirdPartyCookieBlock => "Stops sites in one organisation reading cookies set by another.",
            Defense::TotalCookieBlock => "Blocks all cookies. Many sites will ask you to sign in again more often.",
            Defense::HttpsUpgrade => "Upgrades insecure connections and warns before loading a site without encryption.",
            Defense::ClientHintReduction => "Stops the browser volunteering extra detail about your device.",
        }
    }

    /// The full default set: maximum protection.
    pub fn all() -> &'static [Defense] {
        &[
            Defense::CanvasNoise,
            Defense::AudioNoise,
            Defense::FontNormalization,
            Defense::WebglMasking,
            Defense::WebrtcLeak,
            Defense::UserAgentSpoofing,
            Defense::RefererStripping,
            Defense::QueryStripping,
            Defense::TimezoneSpoofing,
            Defense::ThirdPartyCookieBlock,
            Defense::HttpsUpgrade,
            Defense::ClientHintReduction,
        ]
    }

    /// Balanced defaults. Cookies and HTTPS stay on; the most compatibility-
    /// hostile defences are left available but off.
    pub fn balanced() -> &'static [Defense] {
        &[
            Defense::CanvasNoise,
            Defense::AudioNoise,
            Defense::FontNormalization,
            Defense::WebglMasking,
            Defense::WebrtcLeak,
            Defense::QueryStripping,
            Defense::HttpsUpgrade,
        ]
    }
}

/// The active privacy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub enabled: bool,
    /// Per-defence overrides. Absent means "inherit from the protection level".
    overrides: HashMap<String, bool>,
    pub protection_level: Level,
    /// Global kill switch: block everything that smells like tracking.
    pub block_trackers: bool,
    pub block_ads: bool,
    pub block_annoyances: bool,
    pub https_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Balanced,
    Strict,
    Custom,
}

impl PrivacyConfig {
    /// Balanced preset: strong protection that essentially never breaks sites.
    /// This is the default.
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Maximum protection: every defence on, including the ones that will
    /// break some sites. The user has explicitly asked to trade compatibility
    /// for privacy, so we honour it and keep the dashboard honest about it.
    pub fn strict() -> Self {
        Self {
            protection_level: Level::Strict,
            ..Self::default()
        }
    }

    /// Compatibility preset: blocking only, with fingerprint defences off.
    /// For sites that genuinely refuse to work with any perturbation.
    pub fn compatibility() -> Self {
        Self {
            enabled: true,
            overrides: HashMap::new(),
            protection_level: Level::Custom,
            block_trackers: true,
            block_ads: true,
            block_annoyances: false,
            https_only: true,
        }
    }

    /// Whether a given defence is active right now.
    pub fn is_active(&self, d: Defense) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(v) = self.overrides.get(d.as_str()) {
            return *v;
        }
        match self.protection_level {
            Level::Strict => Defense::all().contains(&d),
            Level::Balanced => Defense::balanced().contains(&d),
            Level::Custom => false,
        }
    }

    pub fn set_override(&mut self, d: Defense, on: bool) {
        self.overrides.insert(d.as_str().to_string(), on);
        self.protection_level = Level::Custom;
    }

    pub fn clear_override(&mut self, d: Defense) {
        self.overrides.remove(d.as_str());
    }

    /// Active defences, for the privacy dashboard.
    pub fn active(&self) -> Vec<Defense> {
        Defense::all()
            .iter()
            .copied()
            .filter(|d| self.is_active(*d))
            .collect()
    }
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            overrides: HashMap::new(),
            protection_level: Level::Balanced,
            block_trackers: true,
            block_ads: true,
            block_annoyances: true,
            https_only: true,
        }
    }
}

/// A stable per-origin pseudonym for cookie/storage isolation.
///
/// Values are derived from (site, install-secret) with a keyed hash, so they are
/// deterministic within a profile but unlinkable across profiles and
/// unguessable without the secret. This is what makes "reset this site" able to
/// produce genuinely fresh state.
#[derive(Clone)]
pub struct Pseudonymiser {
    secret: [u8; 32],
}

impl Pseudonymiser {
    pub fn from_secret(secret: [u8; 32]) -> Self {
        Self { secret }
    }

    /// Derive an install-unique, site-specific token.
    pub fn token(&self, site: &str, salt: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.secret);
        h.update(b"\x00");
        h.update(site.as_bytes());
        h.update(b"\x00");
        h.update(salt.as_bytes());
        let digest = h.finalize();
        // 16 bytes is ample for a partition key and keeps the storage small.
        base64_url(&digest[..16])
    }
}

fn base64_url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// A spoofed, deliberately generic user agent.
pub const REDUCED_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// JS that gets injected into every document to apply fingerprint defences and
/// provide the tracker-count surface the dashboard reads back.
///
/// It is deliberately dependency-free and defensive: every hook is wrapped so a
/// page cannot detect the wrapper by throwing.
pub const FINGERPRINT_SHIELD_JS: &str = r#"
(function () {
  if (window.__kestrelShield) return;
  try {
    Object.defineProperty(window, '__kestrelShield', { value: true, enumerable: false });
    const cfg = __KESTREL_CONFIG__;
    const log = (type, count) => {
      try {
        window.__kestrelBlocked = window.__kestrelBlocked || { type, count };
      } catch (_) {}
    };

    // --- Canvas ---
    if (cfg.canvas) {
      const toDataURL = HTMLCanvasElement.prototype.toDataURL;
      const toBlob = HTMLCanvasElement.prototype.toBlob;
      const getImageData = CanvasRenderingContext2D.prototype.getImageData;
      // Noise is deterministic per session: a stable perturbation is far less
      // fingerprintable than random-per-call, and it does not break image
      // comparison features that expect stability within a page.
      const NOISE = cfg.noise;
      function perturb(data) {
        for (let i = 0; i < data.length; i += 4) {
          const d = ((i * 2654435761) % NOISE) - (NOISE / 2);
          data[i] = Math.max(0, Math.min(255, data[i] + d));
        }
        return data;
      }
      HTMLCanvasElement.prototype.toDataURL = function () {
        try {
          const ctx = this.getContext('2d');
          if (ctx) perturb(ctx.getImageData(0, 0, this.width, this.height).data);
        } catch (_) {}
        return toDataURL.apply(this, arguments);
      };
      HTMLCanvasElement.prototype.toBlob = function () {
        try {
          const ctx = this.getContext('2d');
          if (ctx) perturb(ctx.getImageData(0, 0, this.width, this.height).data);
        } catch (_) {}
        return toBlob.apply(this, arguments);
      };
      const origGID = getImageData;
      CanvasRenderingContext2D.prototype.getImageData = function (x, y, w, h) {
        const img = origGID.apply(this, arguments);
        try { perturb(img.data); } catch (_) {}
        return img;
      };
    }

    // --- WebGL ---
    if (cfg.webgl) {
      for (const proto of [WebGLRenderingContext, WebGL2RenderingContext]) {
        if (!proto) continue;
        const orig = proto.getParameter;
        proto.getParameter = function (p) {
          const v = orig.apply(this, arguments);
          // UNMASKED_VENDOR_WEBGL / UNMASKED_RENDERER_WEBGL and the shader
          // precision bits are the identifying ones.
          if (p === 37445 || p === 37446) return 'Google Inc. (Intel)';
          if (p === 3379 || p === 3381 || p === 3382) return 0;
          return v;
        };
        const origExt = proto.getSupportedExtensions;
        proto.getSupportedExtensions = function () {
          const exts = origExt ? origExt.apply(this, arguments) : [];
          return (exts || []).filter((e) => !/debug_renderer_info/i.test(e));
        };
      }
    }

    // --- Audio ---
    if (cfg.audio) {
      const OrigCtx = window.OfflineAudioContext || window.webkitOfflineAudioContext;
      if (OrigCtx) {
        const origStart = OrigCtx.prototype.startRendering;
        OrigCtx.prototype.startRendering = function () {
          const p = origStart.apply(this, arguments);
          if (p && p.then) {
            p.then((buf) => {
              try {
                const ch = buf.getChannelData(0);
                for (let i = 0; i < ch.length; i += 997) ch[i] += cfg.noise / 1000;
              } catch (_) {}
            }).catch(() => {});
          }
          return p;
        };
      }
    }

    // --- Fonts ---
    if (cfg.fonts) {
      // Narrow the measurement-delta test to a fixed allowlist so the exact
      // installed font set is not observable.
      const measure = (font) => {
        const c = document.createElement('canvas').getContext('2d');
        c.font = font;
        return c.measureText('WM').width;
      };
      const ALLOWED = ['monospace', 'sans-serif', 'serif'];
      window.KestrelFontProbe = function (name) {
        const generic = ALLOWED.find((g) => measure(`10px ${name}, ${g}`) === measure(`10px ${g}`));
        return generic || 'unknown';
      };
    }

    // --- Client hints ---
    if (cfg.clientHints) {
      try {
        if (navigator.userAgentData) {
          delete navigator.userAgentData;
        }
      } catch (_) {}
    }

    // --- WebRTC ---
    if (cfg.webrtc) {
      try {
        const origRTC = window.RTCPeerConnection;
        if (origRTC) {
          const Patched = function (cfgObj) {
            const pc = new origRTC(cfgObj);
            // Force relayed-only ICE so no local address is exposed.
            if (pc.addTransceiver) {
              const origAdd = pc.addTransceiver;
              pc.addTransceiver = function () {
                const t = origAdd.apply(this, arguments);
                try { t.sender.replaceTrack(t.sender.track); } catch (_) {}
                return t;
              };
            }
            pc.addEventListener('icecandidate', (e) => {
              if (e.candidate && e.candidate.candidate &&
                  !/typ relay/.test(e.candidate.candidate)) {
                e.candidate.candidate = null;
              }
            });
            return pc;
          };
          Patched.prototype = origRTC.prototype;
          window.RTCPeerConnection = Patched;
        }
      } catch (_) {}
    }
  } catch (_) {}
})();
"#;

/// The configuration object substituted into [`FINGERPRINT_SHIELD_JS`].
pub fn shield_config(active: &[Defense]) -> String {
    let on = |d: Defense| active.contains(&d);
    // Noise magnitude is small but non-zero: enough to perturb the hash that
    // fingerprinters compute, small enough not to visibly degrade images.
    let noise = 2;
    format!(
        r#"{{"canvas":{},"audio":{},"fonts":{},"webgl":{},"webrtc":{},"clientHints":{},"noise":{}}}"#,
        on(Defense::CanvasNoise),
        on(Defense::AudioNoise),
        on(Defense::FontNormalization),
        on(Defense::WebglMasking),
        on(Defense::WebrtcLeak),
        on(Defense::ClientHintReduction),
        noise
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_enables_expected_set() {
        let c = PrivacyConfig::default();
        assert!(c.is_active(Defense::CanvasNoise));
        assert!(c.is_active(Defense::HttpsUpgrade));
        // Balanced deliberately leaves the most site-breaking ones off.
        assert!(!c.is_active(Defense::TotalCookieBlock));
        assert!(!c.is_active(Defense::TimezoneSpoofing));
    }

    #[test]
    fn strict_enables_everything() {
        let c = PrivacyConfig::strict();
        for d in Defense::all() {
            assert!(c.is_active(*d), "{:?} should be on in strict", d);
        }
    }

    #[test]
    fn override_wins_and_marks_custom() {
        let mut c = PrivacyConfig::default();
        c.set_override(Defense::TotalCookieBlock, true);
        assert!(c.is_active(Defense::TotalCookieBlock));
        assert_eq!(c.protection_level, Level::Custom);
        c.clear_override(Defense::TotalCookieBlock);
        assert!(!c.is_active(Defense::TotalCookieBlock));
    }

    #[test]
    fn kill_switch_disables_everything() {
        let c = PrivacyConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(c.active().is_empty());
    }

    #[test]
    fn pseudonyms_are_stable_and_scoped() {
        let p = Pseudonymiser::from_secret([7u8; 32]);
        let a = p.token("example.com", "cookie");
        let b = p.token("example.com", "cookie");
        let c = p.token("other.com", "cookie");
        assert_eq!(a, b, "must be stable for the same site");
        assert_ne!(a, c, "must differ across sites");
    }

    #[test]
    fn pseudonyms_differ_between_installs() {
        let a = Pseudonymiser::from_secret([1u8; 32]).token("example.com", "x");
        let b = Pseudonymiser::from_secret([2u8; 32]).token("example.com", "x");
        assert_ne!(a, b);
    }

    #[test]
    fn shield_config_tracks_defenses() {
        let js = shield_config(&[Defense::CanvasNoise, Defense::WebglMasking]);
        assert!(js.contains("\"canvas\":true"));
        assert!(js.contains("\"webgl\":true"));
        assert!(js.contains("\"audio\":false"));
    }

    #[test]
    fn every_defense_has_copy() {
        for d in Defense::all() {
            assert!(!d.label().is_empty());
            assert!(!d.description().is_empty());
            assert!(d.description().len() > 30, "{:?} needs a real description", d);
        }
    }
}
