# ◉ ORBISTRA

**Open-source space operations platform** — track everything in orbit, predict collisions, and (roadmap) talk to spacecraft, monitor their health, and run AI onboard.

ORBISTRA ingests the live public space catalog (~16,000 tracked objects), propagates every orbit in parallel with SGP4, screens the full catalog for upcoming conjunctions, estimates collision probability, and renders it all in a real-time 3-D console.

```
TLEs (CelesTrak) ──► orbistra-core ──► orbistra-sentry ──► orbistra-server ──► console (3-D globe)
                     SGP4 engine       conjunction         REST API            live catalog +
                     (Rust, rayon)     screening + Pc      (axum)              conjunction watch
```

## Quickstart

```bash
# Build (Rust 1.75+)
cargo build --release

# Screen the bundled catalog for conjunctions in the next 24 h
./target/release/orbistra-sentry --tle data/sample_tles.txt --hours 24 --threshold-km 5

# Or run the full console with live data from CelesTrak
./target/release/orbistra-server --group active --port 8080
# open http://localhost:8080
```

## What it does today (Phase 1)

- **`orbistra-core`** — TLE catalog parsing, parallel SGP4 batch propagation (full catalog in milliseconds via rayon), TEME→ECEF→geodetic transforms (GMST IAU-1982, Bowring), validated against Vallado reference values.
- **`orbistra-sentry`** — full-catalog conjunction screening using a three-stage sieve: altitude-band prefilter → spatial-hash coarse sweep with a relative-velocity "smart sieve" → fine TCA refinement with quadratic interpolation. Simplified 2-D encounter-plane collision probability. Screens 16k objects × 6 h in about a minute on a laptop.
- **`orbistra-server`** — axum REST API: live CelesTrak ingestion with offline fallback, propagated state endpoint, cached conjunction feed; hosts the console.
- **`console`** — zero-build Three.js UI: 3-D Earth, full catalog point cloud colored by orbit regime, clickable conjunction watch list.

## API

| Endpoint | Description |
|---|---|
| `GET /api/summary` | catalog size, data source, screening status |
| `GET /api/states?limit=N` | current propagated states (ECEF + geodetic) |
| `GET /api/conjunctions` | upcoming close approaches, sorted by miss distance |
| `GET /api/health` | liveness |

## Roadmap (the full ORBISTRA vision)

| Module | Status | Description |
|---|---|---|
| `orbistra-core` | ✅ Phase 1 | Orbit propagation engine |
| `orbistra-sentry` | ✅ Phase 1 | Conjunction screening & collision risk |
| `orbistra-console` | ✅ Phase 1 | 3-D operations console |
| `orbistra-pulse` | Phase 2 | Telemetry anomaly detection + LLM ops agent (NASA SMAP/MSL) |
| `orbistra-link` | Phase 3 | CCSDS TM/TC protocol stack |
| `orbistra-vision` | Phase 3 | Quantized onboard vision models + C++ edge inference runtime |
| `orbistra-earth` | Phase 4 | Satellite imagery change-detection service |

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for design details and honest accuracy caveats, and [`docs/ROADMAP.md`](docs/ROADMAP.md) for the phase plan.

## Accuracy caveats (read this)

TLE + SGP4 is a *screening-grade* tool: position errors are km-scale and grow with propagation time, and TLEs carry no covariance. Collision probabilities here use a configurable assumed uncertainty and are **not** operational-grade risk numbers. Real conjunction assessment uses owner/operator ephemerides and CDMs — supporting those is on the roadmap.

## Contributing

Contributions are welcome — the [roadmap](docs/ROADMAP.md) has plenty of meaty problems (maneuver detection, CDM ingestion, Foster's-method Pc, PyO3 bindings).

1. **⭐ Star this repo** to follow progress, and **fork it** to your account.
2. Clone your fork and create a feature branch:
   ```bash
   git clone https://github.com/<your-username>/orbistra.git
   cd orbistra
   git checkout -b feat/my-change
   ```
3. Build and make your changes:
   ```bash
   cargo build --workspace && cargo test --workspace
   ```
4. Before committing, make sure the CI gates pass locally:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
5. Add tests for new behavior — screening changes should include an engineered TLE fixture demonstrating the case (see the tests in `orbistra-sentry/src/lib.rs`).
6. Push your branch and open a pull request with a clear description of the problem and approach.

For larger changes, open an issue first so we can discuss the design. See [CONTRIBUTING.md](CONTRIBUTING.md) for more.

## License

MIT OR Apache-2.0, at your option. TLE data courtesy of [CelesTrak](https://celestrak.org).
