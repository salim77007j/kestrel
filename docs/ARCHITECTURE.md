# Kestrel — architecture

Kestrel is an independent web browser whose engine, network stack, privacy layer
and user interface are all written in Rust. There is no Chromium, no WebKit and
no C++ toolkit in the process.

## Stack decision

The brief suggested Rust core + Qt 6. I dropped that and went fully Rust. The
reasoning:

| Option | Verdict |
|---|---|
| Rust core + Qt 6 | A C++ toolkit contradicts the "memory-safe, ultra-light" goal, adds ~80 MB of runtime before any browser code, and makes every UI change a two-language round trip. |
| Rust core + egui/wgpu | Pure Rust, small, one GPU surface, immediate redraw only when something changed. **Chosen.** |
| Chromium/EmbeddedWebView | Defeats the entire point — it is the thing being replaced. |

**Engine: Servo.** The `servo` crate was published to crates.io in April 2026
(v0.1 LTS, monthly releases through v0.6). It is a real, independent Rust engine
— SpiderMonkey for JavaScript, Stylo for CSS, WebRender for rasterisation,
Servo-net for HTTP/TLS — and it is embeddable. That is what makes an independent
browser practical at all: writing a compliant engine is a decade of work, and
forging one badly would be worse than building on a well-maintained one.

## Crate layout

```
kestrel/
├── crates/
│   ├── kestrel-core/   product logic, engine-free
│   ├── kestrel-ui/     egui widgets
│   └── kestrel-app/    the binary: engine binding, event loop, compositing
```

`kestrel-core` depends on neither Servo nor egui. That is a deliberate
architectural constraint, not an accident:

* it compiles in ~30 seconds, so CI gives feedback in under a minute;
* the filter engine, privacy policy and storage are testable exhaustively
  without a 40-minute engine build;
* the product logic is auditable in one place, which matters more than usual
  for a privacy browser.

## The no-fake-UI guarantee

UI widgets never mutate browser state. Each returns an **intent** struct
describing what the user asked for:

```rust
pub struct ToolbarIntent {
    pub back: bool,
    pub omnibox_submitted: Option<String>,
    pub toggle_bookmark: bool,
    // ...
}
```

`chrome.rs` is the single place that converts intents into actions on `App`.
This makes the "no inert controls" property auditable in one file rather than
scattered across widget code: a visible control that produces an intent nobody
handles is a dead branch you can find by inspection, not a button that silently
does nothing.

## Privacy design

Blocking happens at the network boundary, in `TabDelegate::load_web_resource`,
which Servo calls for every request:

* **Filtering** — a single Aho-Corasick pass over the lowercased URL. Cost is
  O(url length) regardless of how many rules are loaded, which is what keeps
  blocking off the critical path.
* **Silent failure** — a blocked subresource returns an empty *successful*
  response, not a network error. An error is observable and tells a tracker
  something happened; silence does not.
* **Exceptions** — a block rule and its `@@` exception usually compile to the same
  literal. Aho-Corasick reports non-overlapping matches, so one automaton would
  surface whichever pattern id came first and could silently drop the
  exception. Exception needles are therefore held in a separate set and checked
  explicitly. This was a real bug caught by a test.
* **Permissions** — deny by default. A capability is granted only if the user
  explicitly approved that origin; nothing is granted implicitly.
* **Fingerprinting** — defences are per-site toggleable policy in
  `privacy.rs`, and the injected script uses a *stable per-session* noise value.
  Random-per-call noise is more identifying than no noise, because it makes the
  value unique on every read; stability within a page and variation between
  sites is the actual goal.

## Compositing

Servo renders page content into its own offscreen surface. The active tab is read
back into a CPU image, uploaded as a texture, and composited by the same egui
pass that draws the chrome.

This is a deliberate trade. It costs a CPU readback per new frame, which is
slower than handing the window's GL context to WebRender directly. It was chosen
because:

1. it avoids two graphics APIs contending for one window surface;
2. it makes the browser runnable on a software adapter, so the automated
   validation can run headless on a machine with no GPU;
3. chrome and page are composited in one pass, so z-order is exact.

The readback only happens when Servo reports `notify_new_frame_ready`, so an
idle page costs nothing.

## Storage

No database. A browser that cannot start because a database library failed to
initialise is not a fast browser.

* Settings, bookmarks, downloads, session — JSON, written to a temp file,
  `fsync`ed, then `rename`d. The rename is atomic on POSIX, so a reader sees
  either the old file or the new one, never a half-written mix. This is what
  makes session restore survive a crash.
* History — append-only JSON Lines, because it is the only unbounded data set
  and a page visit must never rewrite the whole file. Compacted on shutdown.

## Performance posture

* egui repaints only on interaction or animation, so an idle browser uses no CPU.
* Only the active tab is painted; background tabs keep state but consume no
  compositor time, which is the cheapest form of throttling. `TabStrip::
  throttlable` identifies which tabs are eligible.
* The filter engine is an immutable `Arc`; the network path takes no locks.
* Cold start is dominated by Servo's own initialisation, not by the browser shell.
