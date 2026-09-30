# Kestrel — validation report

Written at the end of the first working session. This report states what has
actually been verified and what has not. Nothing below is aspirational.

## Verified

### Core logic — green in CI

`kestrel-core` builds and tests in well under a minute on every push, and the
CI job is fully green including `clippy -- -D warnings`.

```
39 passed; 0 failed
```

Coverage of the product logic:

| Area | What is proven |
|---|---|
| Omnibox | URL vs. search classification, IDNA/punycode host collapsing, origin keys, tracking-parameter stripping |
| Filter engine | Blocking, type restrictions, exceptions, cosmetic CSS, comment/`##` handling, case-insensitivity, stats |
| Privacy | Protection levels, per-defence overrides, kill switch, pseudonym stability and scoping |
| Storage | Atomic write + reopen round trip, corrupt-file fallback, folder-cascade delete, path-traversal defence, history dedup and ranking |
| Tabs | Session round trip, pin reordering, back/forward semantics, close/reopen, throttling eligibility |

### Bugs the tests caught

These were real defects found and fixed, not test-tuning:

1. **Aho-Corasick exception collision.** A block rule and its `@@` exception
   compile to the same literal. Aho-Corasick reports *non-overlapping* matches,
   so a single automaton surfaced whichever pattern id came first and could
   silently drop the exception — an allowlisted tracker could stay blocked.
   Fixed by holding exception needles in a separate set.
2. **Cosmetic rules discarded as comments.** `##.sticky-footer` starts with `#`,
   and the comment filter removed it before the parser ever saw it. Cosmetic
   filtering was silently dead.
3. **Bang-search parsing.** `!g kittens` did not split into engine and query.
4. **Filename derivation.** A trailing-slash URL produced an empty filename.

### CI

`.github/workflows/build.yml` — `core` (fast feedback, green), `browser`
(release build, packages the binary, uploads the artifact), `validate`
(downloads the artifact, runs it under Xvfb, captures a screenshot, measures
RSS).

Two real CI failures were diagnosed and fixed:

* the `validate` job was nested inside `browser`, which made the whole workflow
  invalid YAML and failed every run with zero jobs;
* `wgpu 25` was incompatible with `egui-wgpu 0.36` (which requires `wgpu ^30`),
  pulling an incompatible `naga`/`termcolor` pair and breaking the build deep in
  the dependency tree rather than in project code.

## Not verified

Being explicit, because these were requested and are **not** delivered:

* **A compiled browser binary.** The `browser` job was still compiling the Servo
  tree at the time of writing. Servo is a very large C++/Rust build.
* **Screenshots of the running browser.** Requires the binary above. No image of
  the running product exists.
* **UI comparison against the design reference.** Requires the screenshot. The
  theme, metrics and layout in `kestrel-ui` are transcribed from the design
  (`metrics::TAB_STRIP 44`, `OMNIBOX_HEIGHT 36`, pill omnibox, 5 shortcut tiles,
  greeting band, footer strip) and unit-tested for contrast, but that is not a
  substitute for looking at it next to the reference.
* **RAM and CPU measurements.** The `validate` job measures RSS and fails if the
  browser does not start, but no numbers have been observed yet.
* **Real page rendering.** The engine binding was written against the Servo
  0.1.3 API read from the published crate source, not against a compiler. It
  has not been compile-checked. Expect API mismatches on the first build
  attempt; each is a small mechanical fix.
* **Browsing behaviour on live sites.** Ad blocking, tracker counting and
  permission denial are wired to real Servo callbacks but have not been observed
  against a real page.

## Honest assessment

What is solid: the product logic. It is tested, CI-enforced, and four real bugs
were found and fixed by that testing.

What is incomplete: everything from the engine binding outward. The UI is
written against a verified `egui` API and the design, and the intent plumbing
that guarantees no inert controls is in place, but none of it has been seen by a
compiler or a screen.

The next session should start by reading the `browser` job log, fixing the
engine API mismatches, and then running the `validate` job to get the first real
screenshot and the first real memory number. Everything downstream depends on
that binary existing.
