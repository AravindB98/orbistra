//! # orbistra-core
//!
//! High-performance orbit propagation engine for the ORBISTRA platform.
//!
//! Provides:
//! - TLE catalog parsing and validation ([`catalog`])
//! - Parallel SGP4 batch propagation over the full space catalog ([`propagator`])
//! - TEME → ECEF → geodetic coordinate transforms ([`frames`])
//!
//! ```no_run
//! use orbistra_core::{Catalog, Propagator};
//! use chrono::Utc;
//!
//! let tle_text = std::fs::read_to_string("data/sample_tles.txt").unwrap();
//! let catalog = Catalog::from_tle_str(&tle_text).unwrap();
//! let prop = Propagator::new(&catalog);
//! let states = prop.all_states(Utc::now()); // parallel across the catalog
//! println!("propagated {} objects", states.len());
//! ```

pub mod catalog;
pub mod frames;
pub mod propagator;

pub use catalog::{Catalog, SpaceObject};
pub use frames::{ecef_to_geodetic, gmst_rad, teme_to_ecef, Geodetic};
pub use propagator::{Propagator, State};

/// Earth gravitational parameter, km^3/s^2 (WGS-72, consistent with SGP4).
pub const MU_EARTH: f64 = 398600.8;
/// Earth equatorial radius, km (WGS-84).
pub const R_EARTH_KM: f64 = 6378.137;
