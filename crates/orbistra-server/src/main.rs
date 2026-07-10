//! ORBISTRA API server.
//!
//! Loads a TLE catalog (live from CelesTrak or from a local file), serves
//! propagated states and conjunction screening results over HTTP, and hosts
//! the 3-D console UI.
//!
//! Endpoints:
//!   GET /api/health
//!   GET /api/summary
//!   GET /api/states?limit=N
//!   GET /api/conjunctions
//!   (static) /  -> console/

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State as AxumState},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use chrono::Utc;
use orbistra_core::{ecef_to_geodetic, teme_to_ecef, Catalog, Propagator};
use orbistra_sentry::{screen, Conjunction, ScreeningConfig};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

struct AppState {
    propagator: Propagator,
    source: String,
    loaded_at: chrono::DateTime<Utc>,
    conjunctions: RwLock<ConjunctionCache>,
    screen_hours: f64,
    threshold_km: f64,
}

#[derive(Default)]
struct ConjunctionCache {
    status: String,
    results: Vec<Conjunction>,
    computed_at: Option<chrono::DateTime<Utc>>,
    elapsed_s: f64,
}

struct Args {
    tle_path: String,
    group: Option<String>,
    port: u16,
    screen_hours: f64,
    threshold_km: f64,
}

fn parse_args() -> Args {
    let mut args = Args {
        tle_path: "data/sample_tles.txt".to_string(),
        group: None,
        port: 8080,
        screen_hours: 12.0,
        threshold_km: 5.0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        match (flag.as_str(), it.next()) {
            ("--tle", Some(v)) => args.tle_path = v,
            ("--group", Some(v)) => args.group = Some(v),
            ("--port", Some(v)) => args.port = v.parse().unwrap_or(8080),
            ("--screen-hours", Some(v)) => args.screen_hours = v.parse().unwrap_or(12.0),
            ("--threshold-km", Some(v)) => args.threshold_km = v.parse().unwrap_or(5.0),
            _ => {}
        }
    }
    args
}

async fn fetch_celestrak(group: &str) -> Result<String> {
    let url = format!("https://celestrak.org/NORAD/elements/gp.php?GROUP={group}&FORMAT=tle");
    eprintln!("[server] fetching live TLEs: {url}");
    let text = reqwest::get(&url).await?.error_for_status()?.text().await?;
    Ok(text)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();

    // Load catalog: live group if requested (with file fallback), else file.
    let (text, source) = match &args.group {
        Some(group) => match fetch_celestrak(group).await {
            Ok(t) => (t, format!("celestrak:{group}")),
            Err(e) => {
                eprintln!(
                    "[server] live fetch failed ({e}); falling back to {}",
                    args.tle_path
                );
                let t = std::fs::read_to_string(&args.tle_path)
                    .with_context(|| format!("cannot read {}", args.tle_path))?;
                (t, format!("file:{}", args.tle_path))
            }
        },
        None => {
            let t = std::fs::read_to_string(&args.tle_path)
                .with_context(|| format!("cannot read {}", args.tle_path))?;
            (t, format!("file:{}", args.tle_path))
        }
    };

    let catalog = Catalog::from_tle_str(&text)?;
    let propagator = Propagator::new(&catalog);
    eprintln!(
        "[server] catalog loaded from {source}: {} objects ({} SGP4-ready)",
        catalog.len(),
        propagator.len()
    );

    let state = Arc::new(AppState {
        propagator,
        source,
        loaded_at: Utc::now(),
        conjunctions: RwLock::new(ConjunctionCache {
            status: "computing".into(),
            ..Default::default()
        }),
        screen_hours: args.screen_hours,
        threshold_km: args.threshold_km,
    });

    // Kick off conjunction screening on a blocking thread.
    {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let started = std::time::Instant::now();
            let cfg = ScreeningConfig {
                horizon_hours: state.screen_hours,
                threshold_km: state.threshold_km,
                ..Default::default()
            };
            eprintln!(
                "[server] screening {} h window at {} km threshold ...",
                cfg.horizon_hours, cfg.threshold_km
            );
            let results = screen(&state.propagator, &cfg);
            let elapsed = started.elapsed().as_secs_f64();
            eprintln!(
                "[server] screening done: {} conjunctions in {elapsed:.1} s",
                results.len()
            );
            let mut cache = state.conjunctions.write().unwrap();
            cache.status = "ready".into();
            cache.results = results;
            cache.computed_at = Some(Utc::now());
            cache.elapsed_s = elapsed;
        });
    }

    let console_dir = if std::path::Path::new("console/index.html").exists() {
        "console"
    } else {
        "../console"
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/summary", get(summary))
        .route("/api/states", get(states))
        .route("/api/conjunctions", get(conjunctions))
        .fallback_service(tower_http::services::ServeDir::new(console_dir))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", args.port);
    eprintln!("[server] ORBISTRA console: http://localhost:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Serialize)]
struct Summary {
    objects: usize,
    source: String,
    loaded_at: String,
    screening_status: String,
    conjunction_count: usize,
    screen_hours: f64,
    threshold_km: f64,
}

async fn summary(AxumState(state): AxumState<Arc<AppState>>) -> Json<Summary> {
    let cache = state.conjunctions.read().unwrap();
    Json(Summary {
        objects: state.propagator.len(),
        source: state.source.clone(),
        loaded_at: state.loaded_at.to_rfc3339(),
        screening_status: cache.status.clone(),
        conjunction_count: cache.results.len(),
        screen_hours: state.screen_hours,
        threshold_km: state.threshold_km,
    })
}

#[derive(Deserialize)]
struct StatesParams {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct StateOut {
    id: u64,
    name: String,
    /// ECEF position, km — ready for globe rendering.
    ecef_km: [f64; 3],
    lat_deg: f64,
    lon_deg: f64,
    alt_km: f64,
}

async fn states(
    AxumState(state): AxumState<Arc<AppState>>,
    Query(params): Query<StatesParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let t = Utc::now();
    let state2 = state.clone();
    let mut all = tokio::task::spawn_blocking(move || state2.propagator.all_states(t))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(limit) = params.limit {
        all.truncate(limit);
    }
    let out: Vec<StateOut> = all
        .into_iter()
        .map(|s| {
            let ecef = teme_to_ecef(s.position_km, t);
            let g = ecef_to_geodetic(ecef);
            StateOut {
                id: s.norad_id,
                name: s.name,
                ecef_km: ecef,
                lat_deg: g.lat_deg,
                lon_deg: g.lon_deg,
                alt_km: g.alt_km,
            }
        })
        .collect();
    Ok(Json(serde_json::json!({
        "t": t.to_rfc3339(),
        "count": out.len(),
        "states": out,
    })))
}

async fn conjunctions(AxumState(state): AxumState<Arc<AppState>>) -> Json<serde_json::Value> {
    let cache = state.conjunctions.read().unwrap();
    Json(serde_json::json!({
        "status": cache.status,
        "computed_at": cache.computed_at.map(|t| t.to_rfc3339()),
        "elapsed_s": cache.elapsed_s,
        "conjunctions": cache.results,
    }))
}
