//! Coordinate frame transforms: TEME → ECEF → geodetic (WGS-84).
//!
//! SGP4 outputs state vectors in the TEME (True Equator, Mean Equinox)
//! frame. For display and ground-track purposes we rotate into ECEF using
//! GMST (IAU 1982 model) and convert to geodetic coordinates with Bowring's
//! closed-form method. Polar motion is neglected (≈ meters of error), which
//! is appropriate for TLE-class accuracy.

use chrono::{DateTime, Utc};
use serde::Serialize;

const WGS84_A: f64 = 6378.137; // km
const WGS84_F: f64 = 1.0 / 298.257223563;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Geodetic {
    /// Latitude, degrees.
    pub lat_deg: f64,
    /// Longitude, degrees, in [-180, 180].
    pub lon_deg: f64,
    /// Height above the WGS-84 ellipsoid, km.
    pub alt_km: f64,
}

/// Greenwich Mean Sidereal Time (IAU 1982), radians in [0, 2π).
pub fn gmst_rad(t: DateTime<Utc>) -> f64 {
    let jd = t.timestamp_millis() as f64 / 86_400_000.0 + 2_440_587.5;
    let d = jd - 2_451_545.0;
    let tc = d / 36_525.0;
    let gmst_deg = 280.460_618_37
        + 360.985_647_366_29 * d
        + 0.000_387_933 * tc * tc
        - tc * tc * tc / 38_710_000.0;
    gmst_deg.to_radians().rem_euclid(2.0 * std::f64::consts::PI)
}

/// Rotate a TEME position vector (km) into ECEF at time `t`.
pub fn teme_to_ecef(pos_teme: [f64; 3], t: DateTime<Utc>) -> [f64; 3] {
    let g = gmst_rad(t);
    let (s, c) = g.sin_cos();
    [
        c * pos_teme[0] + s * pos_teme[1],
        -s * pos_teme[0] + c * pos_teme[1],
        pos_teme[2],
    ]
}

/// ECEF (km) → geodetic (WGS-84) using Bowring's method.
pub fn ecef_to_geodetic(ecef: [f64; 3]) -> Geodetic {
    let [x, y, z] = ecef;
    let a = WGS84_A;
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let b = a * (1.0 - WGS84_F);
    let ep2 = (a * a - b * b) / (b * b);

    let p = (x * x + y * y).sqrt();
    let theta = (z * a).atan2(p * b);
    let (st, ct) = theta.sin_cos();
    let lat = (z + ep2 * b * st * st * st).atan2(p - e2 * a * ct * ct * ct);
    let n = a / (1.0 - e2 * lat.sin() * lat.sin()).sqrt();
    let alt = if p > 1e-9 {
        p / lat.cos() - n
    } else {
        z.abs() - b
    };

    Geodetic {
        lat_deg: lat.to_degrees(),
        lon_deg: y.atan2(x).to_degrees(),
        alt_km: alt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn gmst_matches_reference() {
        // Vallado, "Fundamentals of Astrodynamics", example 3-5:
        // 1992-08-20 12:14:00 UTC -> GMST = 152.578787886 deg
        let t = Utc.with_ymd_and_hms(1992, 8, 20, 12, 14, 0).unwrap();
        let gmst_deg = gmst_rad(t).to_degrees();
        assert!(
            (gmst_deg - 152.578_787_886).abs() < 1e-4,
            "gmst = {gmst_deg}"
        );
    }

    #[test]
    fn geodetic_round_trip_equator() {
        // A point on the equator at the prime meridian, 400 km up.
        let g = ecef_to_geodetic([WGS84_A + 400.0, 0.0, 0.0]);
        assert!(g.lat_deg.abs() < 1e-9);
        assert!(g.lon_deg.abs() < 1e-9);
        assert!((g.alt_km - 400.0).abs() < 1e-6);
    }

    #[test]
    fn geodetic_pole() {
        let b = WGS84_A * (1.0 - WGS84_F);
        let g = ecef_to_geodetic([0.0, 0.0, b + 100.0]);
        assert!((g.lat_deg - 90.0).abs() < 1e-6);
        assert!((g.alt_km - 100.0).abs() < 0.001);
    }
}
