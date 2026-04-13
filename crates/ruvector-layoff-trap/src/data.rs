//! Real-data loaders for FRED (St. Louis Fed). No API key required for the
//! `fredgraph.csv` export endpoint.
//!
//! The handful of series we care about for the demand-circulation model:
//!
//! | FRED id    | Name                                            | Use |
//! |------------|-------------------------------------------------|-----|
//! | `PAYEMS`   | Total nonfarm payroll employment (thousands)    | L   |
//! | `WASCUR`   | Compensation of employees: wages & salaries ($B)| W   |
//! | `PCEC`     | Personal consumption expenditures ($B, SAAR)    | C   |
//! | `GDP`      | Nominal GDP ($B, SAAR)                          | Y   |
//! | `UNRATE`   | Civilian unemployment rate (%)                  | u   |
//! | `CES0500000003` | Avg hourly earnings, private                | w̄   |
//!
//! These are publicly fetchable over HTTPS with no auth.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One (date, value) sample from a time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub date: String, // ISO-8601 YYYY-MM-DD
    pub value: f64,
}

/// A full FRED series after fetch + parse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: String,
    pub samples: Vec<Sample>,
}

impl Series {
    pub fn latest(&self) -> Option<&Sample> {
        self.samples.last()
    }

    pub fn latest_value(&self) -> Option<f64> {
        self.latest().map(|s| s.value)
    }

    /// Mean over the last `n` samples (NaN-safe).
    pub fn tail_mean(&self, n: usize) -> Option<f64> {
        let len = self.samples.len();
        if len == 0 {
            return None;
        }
        let start = len.saturating_sub(n);
        let slice = &self.samples[start..];
        let finite: Vec<f64> = slice
            .iter()
            .map(|s| s.value)
            .filter(|v| v.is_finite())
            .collect();
        if finite.is_empty() {
            None
        } else {
            Some(finite.iter().sum::<f64>() / finite.len() as f64)
        }
    }

    /// Year-over-year growth rate from the latest 13 monthly samples (or 5
    /// quarterly). Returns `None` if insufficient data.
    pub fn yoy(&self) -> Option<f64> {
        let n = self.samples.len();
        if n < 13 {
            return None;
        }
        let latest = self.samples[n - 1].value;
        let year_ago = self.samples[n - 13].value;
        if !year_ago.is_finite() || year_ago == 0.0 {
            None
        } else {
            Some((latest - year_ago) / year_ago)
        }
    }
}

/// All the macro aggregates we want calibrated from real FRED data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroSnapshot {
    pub payems: Series,   // employment
    pub wascur: Series,   // wages & salaries
    pub pcec: Series,     // PCE
    pub gdp: Series,      // GDP
    pub unrate: Series,   // unemployment rate
    pub ahetpi: Series,   // avg hourly earnings, private
}

impl MacroSnapshot {
    /// Derived real-data parameters for the circulation model.
    pub fn parameters(&self) -> Result<MacroParams> {
        let wages = self
            .wascur
            .latest_value()
            .ok_or_else(|| anyhow!("no WASCUR value"))?;
        let pce = self
            .pcec
            .latest_value()
            .ok_or_else(|| anyhow!("no PCEC value"))?;
        let gdp = self
            .gdp
            .latest_value()
            .ok_or_else(|| anyhow!("no GDP value"))?;
        let unrate = self
            .unrate
            .latest_value()
            .ok_or_else(|| anyhow!("no UNRATE value"))?;
        let payems = self
            .payems
            .latest_value()
            .ok_or_else(|| anyhow!("no PAYEMS value"))?;
        let ahe = self
            .ahetpi
            .latest_value()
            .ok_or_else(|| anyhow!("no AHETPI value"))?;

        // MPC proxy: marginal PCE response to wage income. Use the 1y change
        // in PCE vs the 1y change in wages. Falls back to the canonical 0.85
        // if data is degenerate.
        let dpce = self.pcec.yoy().unwrap_or(0.0);
        let dwage = self.wascur.yoy().unwrap_or(0.0);
        let mpc_est = if dwage.abs() > 0.005 {
            (dpce / dwage).clamp(0.5, 1.2)
        } else {
            0.85
        };

        Ok(MacroParams {
            wages_nominal_b: wages,
            pce_nominal_b: pce,
            gdp_nominal_b: gdp,
            unemployment_rate: unrate / 100.0,
            payroll_thousands: payems,
            avg_hourly_earnings: ahe,
            mpc_empirical: mpc_est,
            date: self
                .wascur
                .latest()
                .map(|s| s.date.clone())
                .unwrap_or_default(),
        })
    }
}

/// Real-data derived macro parameters for the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroParams {
    pub date: String,
    pub wages_nominal_b: f64,
    pub pce_nominal_b: f64,
    pub gdp_nominal_b: f64,
    pub unemployment_rate: f64,
    pub payroll_thousands: f64,
    pub avg_hourly_earnings: f64,
    pub mpc_empirical: f64,
}

/// Fetch a FRED series via the public `fredgraph.csv` endpoint, using the
/// system `curl` binary as the transport.
///
/// FRED's `fredgraph.csv` is an unauthenticated endpoint meant for chart
/// export, but returns canonical `observation_date,VALUE` rows which are
/// stable enough to parse with no CSV dependency.
pub fn fetch_fred(id: &str) -> Result<Series> {
    let url = format!("https://fred.stlouisfed.org/graph/fredgraph.csv?id={id}");
    let out = Command::new("curl")
        .args([
            "-sSL",
            "--max-time",
            "30",
            "--user-agent",
            "ruvector-layoff-trap/0.1 (+https://github.com/ruvnet/ruvector)",
            &url,
        ])
        .output()
        .with_context(|| format!("spawning curl for {id}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(anyhow!("curl failed for {id}: {stderr}"));
    }
    let body = String::from_utf8(out.stdout)
        .with_context(|| format!("non-utf8 body from FRED for {id}"))?;
    parse_fred_csv(id, &body)
}

/// Parse FRED CSV. Tolerates the "." placeholder FRED uses for missing obs.
pub fn parse_fred_csv(id: &str, body: &str) -> Result<Series> {
    let mut samples = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if i == 0 {
            // header: observation_date,ID
            continue;
        }
        let mut cols = line.split(',');
        let date = cols.next().unwrap_or("").trim().to_string();
        let raw = cols.next().unwrap_or("").trim();
        if date.is_empty() {
            continue;
        }
        let value = match raw {
            "" | "." | "NA" => f64::NAN,
            other => other.parse::<f64>().unwrap_or(f64::NAN),
        };
        samples.push(Sample { date, value });
    }
    if samples.is_empty() {
        return Err(anyhow!("no rows parsed for FRED id {id}"));
    }
    Ok(Series {
        id: id.to_string(),
        samples,
    })
}

const SERIES_IDS: [&str; 6] = [
    "PAYEMS",
    "WASCUR",
    "PCEC",
    "GDP",
    "UNRATE",
    "CES0500000003",
];

/// Fetch the full real-data bundle from FRED. On success, also cache as JSON
/// under `<cache_dir>/<id>.json` so subsequent runs are offline-capable.
pub fn fetch_snapshot(cache_dir: Option<&Path>) -> Result<MacroSnapshot> {
    let mut out = Vec::with_capacity(SERIES_IDS.len());
    for id in SERIES_IDS {
        let s = fetch_fred(id).with_context(|| format!("fetching {id}"))?;
        if let Some(dir) = cache_dir {
            let _ = fs::create_dir_all(dir);
            let path: PathBuf = dir.join(format!("{id}.json"));
            if let Ok(j) = serde_json::to_vec_pretty(&s) {
                let _ = fs::write(&path, j);
            }
        }
        out.push(s);
    }
    Ok(MacroSnapshot {
        payems: out[0].clone(),
        wascur: out[1].clone(),
        pcec: out[2].clone(),
        gdp: out[3].clone(),
        unrate: out[4].clone(),
        ahetpi: out[5].clone(),
    })
}

/// Load the cached snapshot from disk (offline fallback).
pub fn load_cached_snapshot(cache_dir: &Path) -> Result<MacroSnapshot> {
    let read_one = |id: &str| -> Result<Series> {
        let path = cache_dir.join(format!("{id}.json"));
        let bytes = fs::read(&path)
            .with_context(|| format!("reading cached {}", path.display()))?;
        let s: Series = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing cached {}", path.display()))?;
        Ok(s)
    };
    Ok(MacroSnapshot {
        payems: read_one("PAYEMS")?,
        wascur: read_one("WASCUR")?,
        pcec: read_one("PCEC")?,
        gdp: read_one("GDP")?,
        unrate: read_one("UNRATE")?,
        ahetpi: read_one("CES0500000003")?,
    })
}

/// Convenience: fetch if network is available, else fall back to cache.
pub fn load_snapshot(cache_dir: &Path) -> Result<MacroSnapshot> {
    match fetch_snapshot(Some(cache_dir)) {
        Ok(s) => Ok(s),
        Err(fetch_err) => {
            eprintln!("[warn] FRED fetch failed: {fetch_err:#}");
            match load_cached_snapshot(cache_dir) {
                Ok(s) => {
                    eprintln!("[info] falling back to cached snapshot");
                    Ok(s)
                }
                Err(cache_err) => Err(anyhow!(
                    "FRED fetch failed ({fetch_err:#}) and no cache at {}: {cache_err:#}",
                    cache_dir.display()
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_fred_csv() {
        let body = "observation_date,PAYEMS\n2024-01-01,158000\n2024-02-01,158100\n";
        let s = parse_fred_csv("PAYEMS", body).unwrap();
        assert_eq!(s.id, "PAYEMS");
        assert_eq!(s.samples.len(), 2);
        assert_eq!(s.latest_value(), Some(158100.0));
    }

    #[test]
    fn tolerates_missing() {
        let body = "observation_date,GDP\n2024-01-01,.\n2024-04-01,28000\n";
        let s = parse_fred_csv("GDP", body).unwrap();
        assert!(s.samples[0].value.is_nan());
        assert_eq!(s.samples[1].value, 28000.0);
    }
}
