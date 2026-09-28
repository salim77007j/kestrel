# Kestrel

An independent, privacy-first web browser built on the [Servo](https://servo.org)
engine. Kestrel is written in Rust end to end — engine, networking, privacy and
interface — with no Chromium, WebKit or C++ toolkit in the process.

## Status

Under active development. See `docs/ARCHITECTURE.md` for the design and
`docs/VALIDATION.md` for the current test and performance report.

## Build

```sh
cargo build --release -p kestrel-app
./target/release/kestrel
```

## Test

```sh
cargo test -p kestrel-core     # product logic, no engine needed
cargo test --workspace         # everything
```

## Licence

MPL-2.0
