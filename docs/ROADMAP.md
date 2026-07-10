# ORBISTRA Roadmap

One platform, four phases. Each phase ships something independently useful.

## Phase 1 — Space Situational Awareness ✅

- [x] `orbistra-core`: TLE catalog, parallel SGP4, frame transforms
- [x] `orbistra-sentry`: full-catalog conjunction screening + screening-grade Pc
- [x] `orbistra-server`: REST API, live CelesTrak ingestion, offline fallback
- [x] `console`: 3-D globe, catalog point cloud, conjunction watch
- [ ] Maneuver detection: flag objects whose successive TLEs imply a ΔV (ML classifier over element history)
- [ ] PyO3 bindings (`orbistra-py`) for notebook use
- [ ] CDM ingestion + Foster's method Pc (operational-grade probability)

## Phase 2 — Fleet Health (`orbistra-pulse`)

- [ ] Streaming telemetry pipeline over NASA's public SMAP/MSL anomaly dataset
- [ ] Time-series anomaly detection (seasonal decomposition baseline → learned models)
- [ ] LLM ops agent: explain anomalies, draft operator reports, propose checklists
- [ ] Alerting integration into the console

## Phase 3 — Spacecraft I/O

- [ ] `orbistra-link`: CCSDS TM/TC Space Packet + COP-1 in Rust, fuzz-tested
- [ ] `orbistra-vision`: quantized cloud/ship detection models (Sentinel-2 training data) + C++ inference runtime sized for radiation-tolerant-class edge hardware

## Phase 4 — Earth Data Products

- [ ] `orbistra-earth`: change-detection API over a geospatial foundation model, tile server, map UI
- [ ] Docs site, benchmarks page, one-command Docker Compose deploy
