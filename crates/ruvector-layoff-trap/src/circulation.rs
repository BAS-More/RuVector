//! Demand circulation graph + mincut over a sector partition.
//!
//! The graph is a closed wage ⇌ consumption loop:
//!
//! ```text
//! wages[i] ──► households[j] ──► consumption[k] ──► firms[k]
//! ```
//!
//! where firms[k] pay wages[k] in the next period, closing the circulation.
//!
//! The "mincut" we care about is not the max-flow graph-theoretic mincut —
//! it is the **minimum aggregate consumption** that still keeps every sector
//! above its fixed-cost floor. Concretely:
//!
//! ```text
//! K(G_t) = max aggregate consumption C such that
//!          s_k · C >= FC_k  for all sectors k that are still solvent.
//! ```
//!
//! Under automation, wages are removed from sectors, MPC contracts their
//! downstream consumption edges, and certain sectors drop below their floor
//! — at which point they stop purchasing (cascade). The mincut is the
//! *remaining* solvent throughput.

use crate::sectors::{
    consumption_share_normalized, employment_share_normalized, AUTOMATION_EXPOSURE,
    AVG_WEEKLY_EARNINGS, FIXED_COST_FLOOR, SUPERSECTORS,
};
use serde::{Deserialize, Serialize};

/// Number of CES supersectors.
pub const N_SECTORS: usize = 15;

/// Exogenous (non-wage) components of demand — government spending + net
/// exports + investment + transfers. These do not contract with layoffs and
/// act as the floor of the circulation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExogenousDemand {
    /// Fraction of baseline PCE replaced by government transfers in the
    /// steady state (unemployment insurance + Social Security + SNAP …).
    pub transfer_floor: f64,
    /// Government direct spending as a fraction of baseline PCE.
    pub government: f64,
    /// Net exports as a fraction of baseline PCE (can be negative).
    pub net_exports: f64,
}

impl Default for ExogenousDemand {
    fn default() -> Self {
        // Rough 2023 calibration: transfers ≈ 17% of PCE, gov consumption ≈
        // 18%, NX ≈ −4%.
        Self {
            transfer_floor: 0.17,
            government: 0.18,
            net_exports: -0.04,
        }
    }
}

/// Parameters of the circulation model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CirculationParams {
    /// Marginal propensity to consume out of labor income.
    pub mpc: f64,
    /// Fraction of lost wage replaced by UI / transfers.
    pub unemployment_insurance: f64,
    /// Keynesian multiplier μ ≈ 1 / (1 − MPC·(1 − τ)).
    pub multiplier: f64,
    /// Marginal tax on wages (effective, federal+state+FICA).
    pub effective_tax: f64,
}

impl CirculationParams {
    pub fn from_macro(mpc: f64) -> Self {
        let tau = 0.25;
        let eff_mpc = mpc * (1.0 - tau);
        let mult = 1.0 / (1.0 - eff_mpc).max(1e-6);
        Self {
            mpc,
            unemployment_insurance: 0.30,
            multiplier: mult,
            effective_tax: tau,
        }
    }
}

/// State of one sector at time t.
#[derive(Debug, Clone, Serialize)]
pub struct SectorState {
    pub name: String,
    /// Fraction of baseline employment still employed (1.0 = baseline).
    pub employment: f64,
    /// Fraction of baseline wages still being paid.
    pub wages: f64,
    /// Revenue as fraction of baseline.
    pub revenue: f64,
    /// True if revenue has fallen below the fixed-cost floor (insolvent).
    pub insolvent: bool,
    /// Cumulative automation applied to this sector (0..=exposure cap).
    pub automation: f64,
}

/// Full circulation state at one time-step.
#[derive(Debug, Clone, Serialize)]
pub struct CirculationState {
    pub t: usize,
    pub sectors: [SectorState; N_SECTORS],
    /// Aggregate wages (fraction of baseline).
    pub wages_agg: f64,
    /// Aggregate consumption (fraction of baseline).
    pub consumption_agg: f64,
    /// Mincut score — min(rev_k / FC_k) over solvent sectors. ≤ 1 means
    /// the weakest surviving sector is at its floor.
    pub cut_score: f64,
    /// Number of solvent sectors.
    pub solvent: usize,
    /// Aggregate profit proxy (revenue − wages − capex).
    pub profit: f64,
}

/// Baseline wage mass vector per sector (relative, sums to 1).
fn baseline_wage_mass() -> [f64; N_SECTORS] {
    let emp = employment_share_normalized();
    let mut out = [0.0; N_SECTORS];
    let mut total = 0.0;
    for i in 0..N_SECTORS {
        out[i] = emp[i] * AVG_WEEKLY_EARNINGS[i];
        total += out[i];
    }
    for v in out.iter_mut() {
        *v /= total;
    }
    out
}

/// Compute the circulation mincut / state given current employment and
/// automation levels per sector.
pub fn compute_state(
    t: usize,
    employment: &[f64; N_SECTORS],
    automation: &[f64; N_SECTORS],
    params: &CirculationParams,
    exog: &ExogenousDemand,
) -> CirculationState {
    let wage_mass_0 = baseline_wage_mass();

    // 1. Wages per sector: employment × wage rate × (1 − automation drag on
    //    headcount is already baked into employment, but automation also
    //    applies downward pressure on residual wage levels as bargaining
    //    power erodes — a 0.5× haircut at full exposure).
    let mut wages = [0.0; N_SECTORS];
    let mut wages_agg = 0.0;
    for i in 0..N_SECTORS {
        let bargaining = 1.0 - 0.5 * automation[i];
        wages[i] = employment[i] * bargaining * wage_mass_0[i];
        wages_agg += wages[i];
    }

    // 2. Household income = wages + UI backfill on lost wages + transfers.
    let lost_wages = (1.0 - wages_agg).max(0.0);
    let ui_income = params.unemployment_insurance * lost_wages;
    let transfers = exog.transfer_floor;
    let disposable_from_labor = wages_agg * (1.0 - params.effective_tax) + ui_income;

    // 3. Consumption = MPC · disposable + exogenous demand routes.
    //    We also include multiplier feedback implicitly by letting revenue
    //    → wages → consumption iterate across time steps.
    let consumption_private = params.mpc * disposable_from_labor;
    let consumption_exog = transfers + exog.government + exog.net_exports;
    let consumption_agg = (consumption_private + consumption_exog).max(0.0);

    // 4. Revenue per sector = consumption_agg × sector share (+ cross-sector
    //    I/O demand approximated as proportional to downstream revenue).
    let cons_share = consumption_share_normalized();
    let mut sectors: Vec<SectorState> = Vec::with_capacity(N_SECTORS);
    let mut min_floor_ratio = f64::INFINITY;
    let mut solvent = 0usize;
    let mut profit_acc = 0.0;

    for i in 0..N_SECTORS {
        let revenue = consumption_agg * cons_share[i];
        let floor = FIXED_COST_FLOOR[i] * cons_share[i];
        let insolvent = revenue < floor * 0.98;
        if !insolvent {
            solvent += 1;
        }
        // IMPORTANT: compute cut_score over ALL sectors, not only the still
        // solvent ones. Dropping the weakest sector out of the min would
        // make the reported cut_score non-monotone: losing a sector would
        // appear to *raise* the mincut. We want the mincut to capture the
        // weakest link of the full circulation at all times.
        let ratio = revenue / floor.max(1e-9);
        if ratio < min_floor_ratio {
            min_floor_ratio = ratio;
        }
        // Profit proxy: revenue − wages − capex for automation
        let capex = 0.08 * automation[i] * wage_mass_0[i];
        let profit = revenue - wages[i] - capex;
        profit_acc += profit;

        sectors.push(SectorState {
            name: SUPERSECTORS[i].to_string(),
            employment: employment[i],
            wages: wages[i] / wage_mass_0[i].max(1e-9),
            revenue: revenue / cons_share[i].max(1e-9),
            insolvent,
            automation: automation[i],
        });
    }

    let cut_score = if min_floor_ratio.is_finite() {
        min_floor_ratio
    } else {
        0.0
    };

    CirculationState {
        t,
        sectors: sectors.try_into().unwrap(),
        wages_agg,
        consumption_agg,
        cut_score,
        solvent,
        profit: profit_acc,
    }
}

/// Phenomenological automation schedule: each sector automates toward its
/// exposure cap at rate `alpha_base · competitive_pressure`, where the
/// competitive pressure rises as rival sectors automate.
pub fn automation_step(
    current_automation: &mut [f64; N_SECTORS],
    alpha_base: f64,
    competitive_coupling: f64,
) {
    let mean_auto: f64 =
        current_automation.iter().sum::<f64>() / N_SECTORS as f64;
    for i in 0..N_SECTORS {
        let cap = AUTOMATION_EXPOSURE[i];
        if current_automation[i] >= cap {
            continue;
        }
        let pressure = 1.0 + competitive_coupling * mean_auto;
        let step = alpha_base * pressure * (cap - current_automation[i]);
        current_automation[i] = (current_automation[i] + step).min(cap);
    }
}

/// Update employment given new automation level. Employment in sector i
/// equals (1 − automation[i]) modulo a re-insertion rate for workers that
/// get rehired into non-exposed roles.
pub fn employment_from_automation(
    automation: &[f64; N_SECTORS],
    reinsertion_rate: f64,
) -> [f64; N_SECTORS] {
    let mut out = [0.0; N_SECTORS];
    for i in 0..N_SECTORS {
        let displaced = automation[i];
        let remaining = 1.0 - displaced * (1.0 - reinsertion_rate);
        out[i] = remaining.max(0.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_state_is_solvent() {
        let emp = [1.0; N_SECTORS];
        let auto = [0.0; N_SECTORS];
        let p = CirculationParams::from_macro(0.85);
        let e = ExogenousDemand::default();
        let s = compute_state(0, &emp, &auto, &p, &e);
        assert_eq!(s.solvent, N_SECTORS);
        assert!(s.cut_score > 1.0, "baseline should clear the floor");
    }

    #[test]
    fn full_automation_collapses_mincut() {
        let auto = AUTOMATION_EXPOSURE;
        let emp = employment_from_automation(&auto, 0.2);
        let p = CirculationParams::from_macro(0.85);
        let e = ExogenousDemand::default();
        let s = compute_state(10, &emp, &auto, &p, &e);
        assert!(s.cut_score < 1.0, "full automation should push below floor");
        assert!(s.solvent < N_SECTORS);
    }
}
