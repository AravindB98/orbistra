//! ORBISTRA Sentry CLI — screen a TLE catalog for upcoming conjunctions.
//!
//! Usage:
//!   orbistra-sentry --tle data/sample_tles.txt --hours 24 --threshold-km 5 --json out.json

use anyhow::{bail, Context, Result};
use orbistra_core::{Catalog, Propagator};
use orbistra_sentry::{screen, ScreeningConfig};

struct Args {
    tle_path: String,
    hours: f64,
    threshold_km: f64,
    coarse_step_s: f64,
    json_out: Option<String>,
    limit: usize,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        tle_path: "data/sample_tles.txt".to_string(),
        hours: 24.0,
        threshold_km: 5.0,
        coarse_step_s: 60.0,
        json_out: None,
        limit: 25,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = |name: &str| -> Result<String> {
            it.next().with_context(|| format!("missing value for {name}"))
        };
        match flag.as_str() {
            "--tle" => args.tle_path = val("--tle")?,
            "--hours" => args.hours = val("--hours")?.parse()?,
            "--threshold-km" => args.threshold_km = val("--threshold-km")?.parse()?,
            "--coarse-step-s" => args.coarse_step_s = val("--coarse-step-s")?.parse()?,
            "--json" => args.json_out = Some(val("--json")?),
            "--limit" => args.limit = val("--limit")?.parse()?,
            "--help" | "-h" => {
                println!(
                    "orbistra-sentry — conjunction screening\n\n\
                     OPTIONS:\n\
                     \x20 --tle <path>            TLE catalog file (default: data/sample_tles.txt)\n\
                     \x20 --hours <f64>           screening horizon in hours (default: 24)\n\
                     \x20 --threshold-km <f64>    report threshold in km (default: 5)\n\
                     \x20 --coarse-step-s <f64>   coarse sweep step in seconds (default: 60)\n\
                     \x20 --json <path>           also write results as JSON\n\
                     \x20 --limit <usize>         max rows to print (default: 25)"
                );
                std::process::exit(0);
            }
            other => bail!("unknown flag: {other} (try --help)"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let started = std::time::Instant::now();

    let text = std::fs::read_to_string(&args.tle_path)
        .with_context(|| format!("cannot read TLE file {}", args.tle_path))?;
    let catalog = Catalog::from_tle_str(&text)?;
    let prop = Propagator::new(&catalog);
    eprintln!(
        "[sentry] catalog: {} objects parsed, {} initialized for SGP4",
        catalog.len(),
        prop.len()
    );

    let cfg = ScreeningConfig {
        horizon_hours: args.hours,
        threshold_km: args.threshold_km,
        coarse_step_s: args.coarse_step_s,
        ..Default::default()
    };
    eprintln!(
        "[sentry] screening {} h window, threshold {} km, coarse step {} s ...",
        cfg.horizon_hours, cfg.threshold_km, cfg.coarse_step_s
    );

    let results = screen(&prop, &cfg);
    eprintln!(
        "[sentry] {} conjunctions found in {:.1} s",
        results.len(),
        started.elapsed().as_secs_f64()
    );

    println!(
        "{:<20} {:<24} {:<24} {:>10} {:>10} {:>10}",
        "TCA (UTC)", "OBJECT A", "OBJECT B", "MISS km", "VREL km/s", "Pc"
    );
    for c in results.iter().take(args.limit) {
        println!(
            "{:<20} {:<24} {:<24} {:>10.3} {:>10.2} {:>10.2e}",
            c.tca.format("%Y-%m-%d %H:%M:%S"),
            truncate(&c.name_a, 24),
            truncate(&c.name_b, 24),
            c.miss_distance_km,
            c.relative_speed_km_s,
            c.probability
        );
    }

    if let Some(path) = args.json_out {
        std::fs::write(&path, serde_json::to_string_pretty(&results)?)?;
        eprintln!("[sentry] wrote {}", path);
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}
