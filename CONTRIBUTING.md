# Contributing to ORBISTRA

Thanks for your interest! ORBISTRA is in active early development.

## Getting started

```bash
cargo build --workspace
cargo test --workspace
cargo run --release -p orbistra-server -- --tle data/sample_tles.txt
```

## Before opening a PR

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Add tests for new behavior. Screening changes should include an engineered
  TLE fixture demonstrating the case (see `orbistra-sentry/src/lib.rs` tests).

## Where help is wanted

See [docs/ROADMAP.md](docs/ROADMAP.md) — maneuver detection, CDM ingestion,
Foster's-method Pc, and PyO3 bindings are great first projects.
