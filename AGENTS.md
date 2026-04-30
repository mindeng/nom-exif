# AGENTS.md

## Project

`nom-exif` — Rust Exif/metadata parsing library for images, video, and audio files.

## Commands

```bash
cargo build                     # build
cargo test -- --nocapture       # run tests
cargo test --all-features -- --nocapture   # test all features
cargo fmt --check               # format check
cargo clippy -- -D warnings     # lint (warnings are errors)
cargo doc --all-features        # build docs
```

## Features

- `async` — async API with tokio
- `json_dump` — JSON serialization for rexiftool example

## Workspace

- `.` — main library
- `afl-fuzz/` — fuzz testing (requires `cargo install cargo-afl`)

## Test Data

`testdata/` contains sample files for testing and examples.

## Example

```bash
cargo run --example rexiftool testdata/meta.mov
cargo run --features json_dump --example rexiftool testdata/meta.mov -j
```

## Notes

- Minimum Rust version: 1.80
- CI tests 32-bit Android target (`armv7-linux-androideabi`) in addition to native
- `-- --nocapture` shows test output (required for some tests)