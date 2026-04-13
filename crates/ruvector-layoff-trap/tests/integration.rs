//! End-to-end integration tests — no network.

use ruvector_layoff_trap::{
    data::parse_fred_csv,
    externality_ratio, run, run_all_policies, sectors,
    simulation::{Policy, SimConfig},
};

#[test]
fn sector_invariants_hold() {
    sectors::check_invariants();
}

#[test]
fn parse_real_fred_payems_snippet() {
    // First ~5 real rows of FRED PAYEMS (publicly available).
    let body = "observation_date,PAYEMS\n\
                1939-01-01,29923\n\
                1939-02-01,30100\n\
                1939-03-01,30280\n\
                1939-04-01,30094\n";
    let s = parse_fred_csv("PAYEMS", body).unwrap();
    assert_eq!(s.samples.len(), 4);
    assert_eq!(s.latest_value(), Some(30094.0));
}

#[test]
fn externality_scales_inversely_with_firm_share() {
    let eta_small = externality_ratio(0.001, 0.85, 0.3, 1.8);
    let eta_large = externality_ratio(0.10, 0.85, 0.3, 1.8);
    assert!(eta_small > eta_large);
    assert!(eta_small > 100.0);
}

#[test]
fn proof_gate_strictly_dominates_laissez() {
    // Under any reasonable MPC, the proof-gated policy must leave the
    // demand mincut at least as high as the laissez-faire trajectory,
    // because the gate is a monotone restriction on the action space.
    for &mpc in &[0.85, 0.90, 1.00, 1.05] {
        let trajs = run_all_policies(mpc, 25);
        let laissez = &trajs[0];
        let gate = &trajs[2];

        let cut_l = laissez.rows.last().unwrap().cut_score;
        let cut_g = gate.rows.last().unwrap().cut_score;
        assert!(
            cut_g >= cut_l - 1e-6,
            "proof-gate should dominate laissez at mpc={mpc} (l={cut_l}, g={cut_g})"
        );
    }
}

#[test]
fn laissez_cut_score_monotonically_decreases() {
    // Under laissez-faire, the mincut must be non-increasing across time —
    // automation is monotone and the circulation cannot spontaneously
    // regrow without an exogenous input.
    let cfg = SimConfig {
        horizon: 25,
        policy: Policy::Laissez,
        ..Default::default()
    };
    let t = run(&cfg);
    for i in 1..t.rows.len() {
        assert!(
            t.rows[i].cut_score <= t.rows[i - 1].cut_score + 1e-6,
            "cut_score must be non-increasing (year {i}): {} -> {}",
            t.rows[i - 1].cut_score,
            t.rows[i].cut_score
        );
    }
    // And the laissez trajectory must eventually push the mincut below 1.0
    // under the realistic 5%/yr automation pace — this is the *whole claim*
    // of the paper, and the test fails the crate if the math ever stops
    // reproducing it.
    assert!(t.mincut_crossing.is_some());
}
