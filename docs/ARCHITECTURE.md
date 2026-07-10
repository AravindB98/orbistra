# ORBISTRA Architecture

## Overview

```
                    ┌─────────────────────────────────────────────┐
                    │                orbistra-server              │
   CelesTrak ──────►│  TLE ingestion · state API · conjunction    │◄──── console/
   (live TLEs)      │  feed · static hosting        (axum/tokio)  │      (Three.js)
                    └───────┬─────────────────────────┬───────────┘
                            │                         │
                    ┌───────▼────────┐       ┌────────▼────────┐
                    │ orbistra-core  │       │ orbistra-sentry │
                    │ SGP4 engine    │◄──────│ conjunction     │
                    │ frames, catalog│       │ screening, Pc   │
                    └────────────────┘       └─────────────────┘
```

## orbistra-core

- **Catalog** (`catalog.rs`): parses 2-line and 3-line TLE sets (malformed sets skipped, checksums validated by the `sgp4` crate). Derives perigee/apogee from mean motion for downstream filtering.
- **Propagator** (`propagator.rs`): precomputes SGP4 constants once per object; batch propagation is embarrassingly parallel via rayon. Two paths: full `State` (with identity, for APIs) and a string-free `snapshot()` fast path for screening loops.
- **Frames** (`frames.rs`): TEME→ECEF via GMST (IAU 1982; validated against Vallado example 3-5), ECEF→geodetic via Bowring. Polar motion and UT1−UTC are neglected — meters-level error, appropriate for TLE-class data.

## orbistra-sentry: the screening sieve

Screening N ≈ 16,000 objects pairwise over a time window is O(N²·steps) if done naively (~46 billion distance checks for 6 h at 60 s). Three stages cut this down:

1. **Altitude-band prefilter** — pairs whose [perigee, apogee] bands (± margin) don't overlap can never approach. Cheap, static, removes most LEO×GEO-type pairs.
2. **Coarse sweep + spatial hash** — at each coarse step the snapshot is binned into a uniform grid (cell = capture radius). Only same-cell and forward-neighbor-cell pairs are examined: expected cost per step drops to roughly O(N · density).
3. **Smart sieve (relative velocity)** — a pair at distance d is a candidate only if it can close below the threshold within one coarse step at its *actual* relative speed: `d < threshold + |v_rel|·Δt`. This is the critical memory/CPU filter: dense co-orbital shells (Starlink planes) have thousands of neighbors within a purely geometric capture radius, but their relative velocities are tiny, so they are correctly rejected. Without this stage the candidate set explodes to hundreds of millions of events (§ we learned this the hard way — the first implementation was OOM-killed).
4. **Refinement** — candidate times are merged into windows; relative distance is sampled at the fine step (default 1 s) and the minimum is refined by quadratic interpolation through the three bracketing samples, giving sub-second TCA precision.

### Collision probability

`probability_2d()` implements the standard Rician / 2-D encounter-plane approximation with an isotropic combined uncertainty σ and combined hard-body radius (default 20 m):

```
Pc ≈ (HBR² / 2σ²) · exp(−d² / 2σ²)
```

**This is a screening heuristic, not an operational Pc.** TLEs carry no covariance. Roadmap: ingest CDMs / owner-operator ephemerides with real covariances and implement Foster's method (2-D integral over the hard-body disk).

## orbistra-server

- Live catalog fetch from CelesTrak GP API with graceful fallback to the bundled snapshot (`data/sample_tles.txt`) so the demo works offline.
- Screening runs on a `spawn_blocking` thread at startup; results are cached behind an `RwLock` and served immediately once ready (the API reports `computing` in the meantime).
- States endpoint propagates the whole catalog per request (milliseconds), converting to ECEF + geodetic for the UI.

## Console

Deliberately zero-build: one HTML file, ES modules from CDN, vanilla JS. Renders 16k objects as a single `THREE.Points` buffer (one draw call), colored by orbit regime. Falls back to an untextured globe if the texture CDN is unreachable.

## Design decisions

- **Rust workspace, library-first**: `core` and `sentry` are libraries with a thin CLI/server on top — reusable from future Python bindings (PyO3 is on the roadmap).
- **No database in Phase 1**: the catalog fits in memory and rebuilds in seconds; persistence buys nothing yet. Postgres + historical archive arrive with `orbistra-pulse`.
- **Determinism for tests**: engineered TLE fixtures with valid checksums exercise the full pipeline (a co-orbital trailing pair must produce conjunctions; a LEO/GEO pair must produce none).
