//! # ruvector-layoff-trap
//!
//! A Rust implementation of the demand-circulation mincut model of the
//! "AI Layoff Trap", calibrated on real FRED + BEA data.
//!
//! See `docs/research/ai-layoff-trap/model.md` for the theoretical note and
//! `src/bin/demo.rs` for a runnable end-to-end report.

pub mod circulation;
pub mod data;
pub mod sectors;
pub mod simulation;

pub use circulation::{
    compute_state, CirculationParams, CirculationState, ExogenousDemand, SectorState,
    N_SECTORS,
};
pub use data::{
    fetch_fred, fetch_snapshot, load_cached_snapshot, load_snapshot, MacroParams,
    MacroSnapshot, Series,
};
pub use sectors::{
    AUTOMATION_EXPOSURE, CONSUMPTION_SHARE, EMPLOYMENT_SHARE, SUPERSECTORS,
};
pub use simulation::{run, run_all_policies, Policy, SimConfig, Trajectory, TrajectoryRow};

/// Convenience: compute the externality ratio for a firm with market share
/// `s_i`, given an MPC estimate and a replacement rate.
#[inline]
pub fn externality_ratio(firm_share: f64, mpc: f64, ui: f64, multiplier: f64) -> f64 {
    if firm_share <= 0.0 {
        return f64::INFINITY;
    }
    let c_soc = multiplier * mpc * (1.0 - ui);
    c_soc / firm_share
}
