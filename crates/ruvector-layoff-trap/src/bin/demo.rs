//! `layoff-trap-demo` — end-to-end runnable report.
//!
//! Fetches real data from FRED (PAYEMS, WASCUR, PCEC, GDP, UNRATE, AHETPI),
//! calibrates the demand-circulation model, and prints a comparative
//! trajectory for three policies.
//!
//! Usage:
//!     cargo run -p ruvector-layoff-trap --bin layoff-trap-demo
//!     cargo run -p ruvector-layoff-trap --bin layoff-trap-demo -- --offline
//!
//! Cached FRED JSON is written to `crates/ruvector-layoff-trap/data/`.

use anyhow::Result;
use ruvector_layoff_trap::{
    data::{load_cached_snapshot, load_snapshot, MacroSnapshot},
    externality_ratio, run_all_policies, sectors, simulation::Trajectory,
};
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")
}

fn banner(s: &str) {
    println!();
    println!("{}", "=".repeat(72));
    println!("  {s}");
    println!("{}", "=".repeat(72));
}

fn header(s: &str) {
    println!();
    println!("── {s} {}", "─".repeat(68 - s.len()));
}

fn print_real_data(snap: &MacroSnapshot) -> Result<()> {
    banner("1. Real macro snapshot (FRED)");
    let p = snap.parameters()?;
    println!("  as of          : {}", p.date);
    println!("  PAYEMS         : {:>12.0} thousand jobs", p.payroll_thousands);
    println!("  WASCUR (wages) : {:>12.1} $B SAAR", p.wages_nominal_b);
    println!("  PCEC           : {:>12.1} $B SAAR", p.pce_nominal_b);
    println!("  GDP            : {:>12.1} $B SAAR", p.gdp_nominal_b);
    println!("  UNRATE         : {:>12.2} %",       p.unemployment_rate * 100.0);
    println!("  AHE (private)  : {:>12.2} $/hr",   p.avg_hourly_earnings);
    println!("  MPC (empirical from YoY ΔPCE/ΔWages): {:.3}", p.mpc_empirical);
    println!();
    println!("  ΣWages / GDP   : {:>11.3}",  p.wages_nominal_b / p.gdp_nominal_b);
    println!("  ΣPCE   / GDP   : {:>11.3}",  p.pce_nominal_b  / p.gdp_nominal_b);
    Ok(())
}

fn print_externality(mpc: f64) {
    banner("2. Externality ratio η = C_soc / C_private at different firm sizes");
    let tau = 0.25;
    let mult = 1.0 / (1.0 - mpc * (1.0 - tau));
    println!("  μ (Keynesian multiplier) ≈ {:.2}", mult);
    println!("  {:<30} {:>10} {:>16}", "firm share s_i", "η", "cost captured");
    for s in [0.0001_f64, 0.001, 0.002, 0.005, 0.01, 0.05, 0.10] {
        let eta = externality_ratio(s, mpc, 0.30, mult);
        println!(
            "  {:<30} {:>10.1} {:>16.3}%",
            format!("{:.4}", s),
            eta,
            100.0 / eta
        );
    }
    println!();
    println!("  Read: an S&P-sized firm (s≈0.2%) captures <0.5% of the damage");
    println!("  it inflicts on the demand graph. The externality is not priced.");
}

fn print_sectors() {
    banner("3. BLS CES supersector partition (calibration inputs)");
    println!(
        "  {:<32} {:>6} {:>8} {:>7} {:>6}",
        "sector", "emp%", "$/wk", "cons%", "auto"
    );
    for i in 0..sectors::SUPERSECTORS.len() {
        println!(
            "  {:<32} {:>5.1}% {:>8.0} {:>6.1}% {:>5.2}",
            sectors::SUPERSECTORS[i],
            sectors::EMPLOYMENT_SHARE[i] * 100.0,
            sectors::AVG_WEEKLY_EARNINGS[i],
            sectors::CONSUMPTION_SHARE[i] * 100.0,
            sectors::AUTOMATION_EXPOSURE[i],
        );
    }
}

fn print_trajectory(t: &Trajectory) {
    header(&format!("policy: {}", t.policy));
    println!(
        "  {:>4} {:>8} {:>8} {:>9} {:>7} {:>9} {:>8}",
        "year", "wages", "cons", "cut", "solvent", "profit", "auto̅"
    );
    for r in &t.rows {
        // Print every 2 years to keep it compact.
        if r.year % 2 != 0 && r.year != t.rows.last().unwrap().year {
            continue;
        }
        println!(
            "  {:>4} {:>8.3} {:>8.3} {:>9.3} {:>7} {:>9.3} {:>8.3}",
            r.year,
            r.wages_agg,
            r.consumption_agg,
            r.cut_score,
            r.solvent_sectors,
            r.profit,
            r.mean_automation,
        );
    }
    println!(
        "  profit crossed baseline at year: {}",
        t.profit_crossing
            .map(|y| y.to_string())
            .unwrap_or_else(|| "never".into())
    );
    println!(
        "  mincut crossed floor at year   : {}",
        t.mincut_crossing
            .map(|y| y.to_string())
            .unwrap_or_else(|| "never".into())
    );
}

fn print_summary(trajs: &[Trajectory]) {
    banner("5. Summary — the trap is a graph property");
    println!(
        "  {:<14} {:>12} {:>14} {:>14}",
        "policy", "mincut@t*", "cut@horizon", "solvent@horizon"
    );
    for t in trajs {
        let cross = t
            .mincut_crossing
            .map(|y| y.to_string())
            .unwrap_or_else(|| "—".into());
        let last = t.rows.last().unwrap();
        println!(
            "  {:<14} {:>12} {:>14.3} {:>14}",
            t.policy, cross, last.cut_score, last.solvent_sectors
        );
    }
    println!();
    println!("  The proof-gated policy is the only one whose mincut does not");
    println!("  cross the fixed-cost floor. Retraining only delays the crossing.");
    println!();
    println!("  > You don't fix the trap by retraining the trapped.");
    println!("  > You fix it by pricing the trap.");
}

fn main() -> Result<()> {
    let offline = std::env::args().any(|a| a == "--offline");

    sectors::check_invariants();

    banner("AI Layoff Trap — demand-circulation mincut (Rust)");
    println!("  crate : ruvector-layoff-trap");
    println!("  data  : FRED (PAYEMS, WASCUR, PCEC, GDP, UNRATE, CES0500000003)");
    println!(
        "  mode  : {}",
        if offline { "offline (cached)" } else { "online (live fetch)" }
    );

    let snap = if offline {
        load_cached_snapshot(&cache_dir())?
    } else {
        load_snapshot(&cache_dir())?
    };
    print_real_data(&snap)?;

    let params = snap.parameters()?;
    print_externality(params.mpc_empirical);
    print_sectors();

    banner("4. Trajectories under 3 policies (20y horizon, α=5%/yr)");
    let trajs = run_all_policies(params.mpc_empirical, 20);
    for t in &trajs {
        print_trajectory(t);
    }

    print_summary(&trajs);

    Ok(())
}
