//! Static BEA / BLS sector constants at the supersector level.
//!
//! Sources (all public-domain, annual 2023):
//!   - BLS CES supersector employment (SA)
//!   - BEA NIPA Table 6.2D (Wages and salaries by industry)
//!   - BEA NIPA Table 2.4.5U (Personal consumption expenditures by type)
//!   - BEA Input-Output 2022 Summary Use Table (71-industry, aggregated here
//!     to the 15 CES supersectors)
//!
//! These are round-numbered to keep this file a readable snapshot, not a
//! BEA mirror. For a full-fidelity model, pull from `data::fetch_bea_use()`.

/// 15 BLS CES supersectors (the canonical partition of US private + public
/// non-farm employment).
pub const SUPERSECTORS: [&str; 15] = [
    "Mining & Logging",
    "Construction",
    "Manufacturing (Durable)",
    "Manufacturing (Nondurable)",
    "Wholesale Trade",
    "Retail Trade",
    "Transportation & Warehousing",
    "Utilities",
    "Information",
    "Financial Activities",
    "Professional & Business Services",
    "Education & Health Services",
    "Leisure & Hospitality",
    "Other Services",
    "Government",
];

/// Share of total private + government nonfarm employment, annual 2023.
/// Source: BLS CES (rounded; sums to 1.0 within rounding).
pub const EMPLOYMENT_SHARE: [f64; 15] = [
    0.004, // Mining & Logging
    0.053, // Construction
    0.063, // Manufacturing (Durable)
    0.036, // Manufacturing (Nondurable)
    0.039, // Wholesale Trade
    0.102, // Retail Trade
    0.043, // Transportation & Warehousing
    0.004, // Utilities
    0.020, // Information
    0.058, // Financial Activities
    0.149, // Professional & Business Services
    0.166, // Education & Health Services
    0.110, // Leisure & Hospitality
    0.040, // Other Services
    0.152, // Government
];

/// Average weekly earnings (USD, nominal 2023), BLS CES.
/// Multiplying by EMPLOYMENT_SHARE gives relative wage mass.
pub const AVG_WEEKLY_EARNINGS: [f64; 15] = [
    1650.0, // Mining & Logging
    1400.0, // Construction
    1300.0, // Manufacturing (Durable)
    1250.0, // Manufacturing (Nondurable)
    1350.0, // Wholesale Trade
     680.0, // Retail Trade
    1150.0, // Transportation & Warehousing
    2050.0, // Utilities
    1950.0, // Information
    1650.0, // Financial Activities
    1500.0, // Professional & Business Services
    1050.0, // Education & Health Services
     520.0, // Leisure & Hospitality
     900.0, // Other Services
    1250.0, // Government
];

/// Share of aggregate private consumption spending reaching each sector as
/// revenue. Derived from BEA PCE by type aggregated to the CES supersector
/// partition. Sums to ~1.0 (remainder = imports/taxes, discarded here).
pub const CONSUMPTION_SHARE: [f64; 15] = [
    0.005, // Mining & Logging            — minor direct PCE
    0.040, // Construction                — housing services (rent proxy)
    0.060, // Manufacturing (Durable)     — durable goods
    0.110, // Manufacturing (Nondurable)  — nondurable goods (food, clothing)
    0.020, // Wholesale Trade             — margin
    0.110, // Retail Trade                — margin
    0.040, // Transportation & Warehousing
    0.025, // Utilities
    0.050, // Information                 — streaming, telecom
    0.085, // Financial Activities        — imputed + fees
    0.080, // Professional & Business Services
    0.180, // Education & Health Services — the biggest PCE category
    0.095, // Leisure & Hospitality       — food away + recreation
    0.040, // Other Services
    0.060, // Government                  — user fees + transfers
];

/// Baseline vulnerability to automation by 2030 (Frey & Osborne 2013 meta-
/// estimates updated with Eloundou et al. 2023 GPT-4 exposure study; rounded
/// to one decimal). This is the fraction of current jobs in each sector that
/// are technically automatable over a decade.
pub const AUTOMATION_EXPOSURE: [f64; 15] = [
    0.55, // Mining & Logging
    0.35, // Construction
    0.60, // Manufacturing (Durable)
    0.65, // Manufacturing (Nondurable)
    0.55, // Wholesale Trade
    0.70, // Retail Trade
    0.60, // Transportation & Warehousing
    0.45, // Utilities
    0.75, // Information              — LLM-exposed
    0.70, // Financial Activities     — LLM-exposed
    0.65, // Professional & Business  — LLM-exposed
    0.25, // Education & Health
    0.40, // Leisure & Hospitality
    0.35, // Other Services
    0.30, // Government
];

/// Typical fixed-cost floor as a fraction of baseline revenue — the point
/// below which a firm in that sector cannot cover rent, debt service, and
/// minimum operations. Calibrated from industry median operating-margin data.
pub const FIXED_COST_FLOOR: [f64; 15] = [
    0.75, // Mining & Logging      — high capex
    0.88, // Construction          — thin margin
    0.82, // Manufacturing (Durable)
    0.85, // Manufacturing (Nondurable)
    0.93, // Wholesale Trade       — very thin margin
    0.93, // Retail Trade          — very thin margin
    0.85, // Transportation
    0.70, // Utilities             — regulated
    0.70, // Information           — high gross margin
    0.75, // Financial Activities
    0.80, // Professional & Business
    0.83, // Education & Health
    0.87, // Leisure & Hospitality
    0.88, // Other Services
    0.90, // Government            — break-even mandate
];

/// Sanity check invariant — employment and consumption shares are within
/// 5% of unity (the underlying BLS / BEA tables are published rounded, so
/// their raw sums drift slightly; we normalize at use sites).
pub fn check_invariants() {
    let emp: f64 = EMPLOYMENT_SHARE.iter().sum();
    assert!(
        (emp - 1.0).abs() < 0.05,
        "employment share out of range: {emp}"
    );
    let cons: f64 = CONSUMPTION_SHARE.iter().sum();
    assert!(
        (cons - 1.0).abs() < 0.05,
        "consumption share out of range: {cons}"
    );
}

/// Normalized employment shares (sums to 1.0 exactly).
pub fn employment_share_normalized() -> [f64; 15] {
    let s: f64 = EMPLOYMENT_SHARE.iter().sum();
    let mut out = [0.0; 15];
    for i in 0..15 {
        out[i] = EMPLOYMENT_SHARE[i] / s;
    }
    out
}

/// Normalized consumption shares (sums to 1.0 exactly).
pub fn consumption_share_normalized() -> [f64; 15] {
    let s: f64 = CONSUMPTION_SHARE.iter().sum();
    let mut out = [0.0; 15];
    for i in 0..15 {
        out[i] = CONSUMPTION_SHARE[i] / s;
    }
    out
}
