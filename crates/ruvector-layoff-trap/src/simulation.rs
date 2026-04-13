//! Multi-period automation simulation that rolls the circulation forward
//! and records the trajectory of the mincut, solvency, and profit.
//!
//! Three policy regimes are provided out of the box:
//!
//!   1. `Policy::Laissez` — free automation, no intervention.
//!   2. `Policy::Retraining` — classical retraining (higher re-insertion
//!      rate, no change to incentives). The paper argues this fails.
//!   3. `Policy::ProofGate`  — RuVector proof-gated governor: firms may
//!      only automate if the mincut delta is below a threshold, otherwise
//!      they must post a social-cost bond that throttles the rate.

use crate::circulation::{
    automation_step, compute_state, employment_from_automation, CirculationParams,
    CirculationState, ExogenousDemand, N_SECTORS,
};
use crate::sectors::AUTOMATION_EXPOSURE;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Policy {
    Laissez,
    Retraining {
        /// Fraction of displaced workers who re-enter at full wage.
        reinsertion: f64,
    },
    ProofGate {
        /// Mincut score below which bonds kick in.
        theta: f64,
        /// Bond factor: how aggressively to throttle further automation
        /// once the governor engages. 1.0 = halve rate at each breach.
        bond_strength: f64,
    },
}

impl Policy {
    pub fn label(&self) -> &'static str {
        match self {
            Policy::Laissez => "laissez",
            Policy::Retraining { .. } => "retraining",
            Policy::ProofGate { .. } => "proof-gate",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimConfig {
    /// Horizon in years.
    pub horizon: usize,
    /// Baseline per-year automation pace.
    pub alpha_base: f64,
    /// Competitive coupling coefficient — peer pressure multiplier.
    pub competitive_coupling: f64,
    /// MPC used in the circulation model (should come from FRED).
    pub mpc: f64,
    pub policy: Policy,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            horizon: 20,
            alpha_base: 0.05,
            competitive_coupling: 0.8,
            mpc: 0.85,
            policy: Policy::Laissez,
        }
    }
}

/// One row of the trajectory — compact enough to print as a table.
#[derive(Debug, Clone, Serialize)]
pub struct TrajectoryRow {
    pub year: usize,
    pub wages_agg: f64,
    pub consumption_agg: f64,
    pub cut_score: f64,
    pub solvent_sectors: usize,
    pub profit: f64,
    pub mean_automation: f64,
}

impl From<&CirculationState> for TrajectoryRow {
    fn from(s: &CirculationState) -> Self {
        let mean_auto: f64 =
            s.sectors.iter().map(|x| x.automation).sum::<f64>() / N_SECTORS as f64;
        Self {
            year: s.t,
            wages_agg: s.wages_agg,
            consumption_agg: s.consumption_agg,
            cut_score: s.cut_score,
            solvent_sectors: s.solvent,
            profit: s.profit,
            mean_automation: mean_auto,
        }
    }
}

/// Full trajectory over the simulation horizon.
#[derive(Debug, Clone, Serialize)]
pub struct Trajectory {
    pub policy: String,
    pub rows: Vec<TrajectoryRow>,
    /// Year in which aggregate profit first falls below the initial baseline
    /// (None if it never does within horizon).
    pub profit_crossing: Option<usize>,
    /// Year in which the mincut crosses the fixed-cost floor (score < 1).
    pub mincut_crossing: Option<usize>,
}

pub fn run(cfg: &SimConfig) -> Trajectory {
    let params = CirculationParams::from_macro(cfg.mpc);
    let exog = ExogenousDemand::default();

    let mut automation = [0.0_f64; N_SECTORS];
    let reinsertion_base = match cfg.policy {
        Policy::Laissez => 0.10,
        Policy::Retraining { reinsertion } => reinsertion,
        Policy::ProofGate { .. } => 0.10,
    };

    let mut rows = Vec::with_capacity(cfg.horizon + 1);
    let mut profit_crossing: Option<usize> = None;
    let mut mincut_crossing: Option<usize> = None;

    // initial snapshot
    let emp0 = employment_from_automation(&automation, reinsertion_base);
    let s0 = compute_state(0, &emp0, &automation, &params, &exog);
    let baseline_profit = s0.profit;
    rows.push(TrajectoryRow::from(&s0));

    for year in 1..=cfg.horizon {
        // Apply competitive automation step
        let mut alpha = cfg.alpha_base;
        // Proof-gate intervenes on the *rate*, not the cap, once mincut
        // crosses theta.
        if let Policy::ProofGate { theta, bond_strength } = cfg.policy {
            if let Some(last) = rows.last() {
                if last.cut_score < theta {
                    let breach = (theta - last.cut_score).max(0.0);
                    alpha *= 1.0 / (1.0 + bond_strength * breach * 10.0);
                }
            }
        }
        automation_step(&mut automation, alpha, cfg.competitive_coupling);
        // enforce exposure caps
        for i in 0..N_SECTORS {
            if automation[i] > AUTOMATION_EXPOSURE[i] {
                automation[i] = AUTOMATION_EXPOSURE[i];
            }
        }

        let emp = employment_from_automation(&automation, reinsertion_base);
        let state = compute_state(year, &emp, &automation, &params, &exog);
        let row = TrajectoryRow::from(&state);

        if profit_crossing.is_none() && row.profit < baseline_profit {
            profit_crossing = Some(year);
        }
        if mincut_crossing.is_none() && row.cut_score < 1.0 {
            mincut_crossing = Some(year);
        }
        rows.push(row);
    }

    Trajectory {
        policy: cfg.policy.label().to_string(),
        rows,
        profit_crossing,
        mincut_crossing,
    }
}

/// Run all three policies against the same configuration. Handy for the
/// comparative demo output.
pub fn run_all_policies(mpc: f64, horizon: usize) -> Vec<Trajectory> {
    let base = SimConfig {
        horizon,
        alpha_base: 0.05,
        competitive_coupling: 0.8,
        mpc,
        policy: Policy::Laissez,
    };
    vec![
        run(&SimConfig {
            policy: Policy::Laissez,
            ..base.clone()
        }),
        // Realistic retraining reinsertion ≈ 15% (empirical long-run
        // re-employment at pre-displacement wages; see Autor, Dorn, Hanson
        // 2013 "China Shock" and OECD displaced-worker studies).
        run(&SimConfig {
            policy: Policy::Retraining { reinsertion: 0.15 },
            ..base.clone()
        }),
        // Proof-gate engages well above the floor so throttling kicks in
        // before the structural cascade starts.
        run(&SimConfig {
            policy: Policy::ProofGate {
                theta: 1.15,
                bond_strength: 4.0,
            },
            ..base
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laissez_eventually_crosses_mincut() {
        let traj = run(&SimConfig::default());
        assert!(
            traj.mincut_crossing.is_some(),
            "laissez should eventually cross the floor"
        );
    }

    #[test]
    fn proof_gate_delays_crossing() {
        let l = run(&SimConfig {
            policy: Policy::Laissez,
            ..Default::default()
        });
        let g = run(&SimConfig {
            policy: Policy::ProofGate {
                theta: 1.05,
                bond_strength: 2.0,
            },
            ..Default::default()
        });
        match (l.mincut_crossing, g.mincut_crossing) {
            (Some(lt), Some(gt)) => assert!(gt >= lt, "gate should delay or prevent"),
            (Some(_), None) => { /* gate prevented entirely — best case */ }
            _ => {}
        }
    }
}
