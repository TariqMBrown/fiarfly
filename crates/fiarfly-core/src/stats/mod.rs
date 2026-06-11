//! Statistical tests for comparing metrics across windows or groups.
//!
//! Pure functions on `&[f32]`. No randomness, no allocation beyond
//! the obvious working buffers. Distributions for p-values come from
//! `statrs`.

mod correction;
mod descriptive;
mod effect;
mod nonparametric;
mod parametric;

pub use correction::{benjamini_hochberg, bonferroni, MultipleComparisonsCorrection};
pub use descriptive::{mean, sample_std, sem};
pub use effect::{cohens_d, cohens_dz, rank_biserial};
pub use nonparametric::{kruskal_wallis, mann_whitney_u, wilcoxon_signed_rank};
pub use parametric::{one_way_anova, paired_t_test, welch_t_test};

use crate::FiarflyError;

// ────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────

/// What the user is asking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// Two paired samples (same ROIs at two windows).
    PairedTwo,
    /// Two independent samples (two ROI groups, one window).
    UnpairedTwo,
    /// 3+ groups (one-way).
    Multi,
}

/// Which statistical test family to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestKind {
    Auto,
    Parametric,
    NonParametric,
}

/// Result of a single hypothesis test.
#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: &'static str,
    pub statistic: f64,
    pub p_value: f64,
    /// Sample sizes per group (1 entry for paired tests, 2 for two-sample,
    /// k for multi-group).
    pub n: Vec<usize>,
    /// Effect-size point estimate (test-appropriate; see `effect_kind`).
    pub effect_size: Option<f64>,
    pub effect_kind: Option<&'static str>,
    /// Optional human-readable diagnostic about assumptions / fallbacks.
    pub note: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────
// High-level dispatch
// ────────────────────────────────────────────────────────────────────────

/// Run the appropriate test for `samples` given `comparison` and `kind`.
///
/// `samples`:
///   - `PairedTwo`     → exactly 2 groups, equal length.
///   - `UnpairedTwo`   → exactly 2 groups (any length).
///   - `Multi`         → ≥ 3 groups.
pub fn run_test(
    samples: &[&[f32]],
    comparison: Comparison,
    kind: TestKind,
) -> Result<TestResult, FiarflyError> {
    let want_parametric = matches!(kind, TestKind::Parametric | TestKind::Auto);
    match comparison {
        Comparison::PairedTwo => {
            if samples.len() != 2 {
                return Err(FiarflyError::InvalidParameter(
                    "PairedTwo requires exactly 2 groups".into(),
                ));
            }
            if samples[0].len() != samples[1].len() {
                return Err(FiarflyError::InvalidParameter(
                    "Paired groups must have equal length".into(),
                ));
            }
            if want_parametric {
                paired_t_test(samples[0], samples[1])
            } else {
                wilcoxon_signed_rank(samples[0], samples[1])
            }
        }
        Comparison::UnpairedTwo => {
            if samples.len() != 2 {
                return Err(FiarflyError::InvalidParameter(
                    "UnpairedTwo requires exactly 2 groups".into(),
                ));
            }
            if want_parametric {
                welch_t_test(samples[0], samples[1])
            } else {
                mann_whitney_u(samples[0], samples[1])
            }
        }
        Comparison::Multi => {
            if samples.len() < 3 {
                return Err(FiarflyError::InvalidParameter(
                    "Multi requires ≥ 3 groups".into(),
                ));
            }
            if want_parametric {
                one_way_anova(samples)
            } else {
                kruskal_wallis(samples)
            }
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Welch's t against a closed-form reference: for the two samples
    /// below, mean diff = -2, pooled SE = 1, df = 8 ⇒ t = -2, two-sided
    /// p = 2 * P(T₈ ≤ -2) ≈ 0.08051.
    #[test]
    fn welch_matches_reference() {
        let a = [1.0_f32, 2.0, 3.0, 4.0, 5.0];
        let b = [3.0_f32, 4.0, 5.0, 6.0, 7.0];
        let r = welch_t_test(&a, &b).unwrap();
        assert!((r.statistic + 2.0).abs() < 1e-6, "t={}", r.statistic);
        assert!((r.p_value - 0.080_51).abs() < 1e-3, "p={}", r.p_value);
        // Cohen's d should be negative; standardised mean diff ≈ −1.26.
        assert!(r.effect_size.unwrap() < -1.0);
    }

    /// Paired t against a known reference: identical sequences with a
    /// constant +1 shift → t = ∞ in theory; with sd=0 we expose p=1 and
    /// statistic=0 by convention.
    #[test]
    fn paired_t_zero_variance() {
        let a = [1.0_f32, 2.0, 3.0];
        let b = [0.0_f32, 1.0, 2.0];
        let r = paired_t_test(&a, &b).unwrap();
        assert_eq!(r.p_value, 1.0);
        assert_eq!(r.statistic, 0.0);
    }

    /// Paired t with a real difference.
    #[test]
    fn paired_t_detects_shift() {
        let a = [10.0_f32, 11.0, 12.0, 9.0, 13.0];
        let b = [8.0_f32, 9.5, 10.5, 7.0, 11.0];
        let r = paired_t_test(&a, &b).unwrap();
        assert!(r.statistic > 0.0);
        assert!(r.p_value < 0.01);
        assert!(r.effect_size.unwrap() > 1.0); // strong dz
    }

    /// Mann-Whitney on two clearly separated samples — should be
    /// near-zero p, U at the boundary.
    #[test]
    fn mann_whitney_separated_samples() {
        let a = [1.0_f32, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0_f32, 11.0, 12.0, 13.0, 14.0];
        let r = mann_whitney_u(&a, &b).unwrap();
        assert!(r.p_value < 0.05, "p was {}", r.p_value);
        // Smaller-U convention: U = 0 when no overlap.
        assert_eq!(r.statistic, 0.0);
        // Effect size near -1 (a < b throughout).
        assert!(r.effect_size.unwrap() < -0.9);
    }

    /// Wilcoxon on a positive shift.
    #[test]
    fn wilcoxon_detects_consistent_shift() {
        let a = [10.0_f32, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0];
        let b = [9.0_f32, 10.5, 11.5, 12.0, 13.0, 14.5, 15.5, 16.0];
        let r = wilcoxon_signed_rank(&a, &b).unwrap();
        assert!(r.p_value < 0.05);
    }

    /// One-way ANOVA: 3 groups with mean separation — F should be large.
    #[test]
    fn anova_detects_group_means() {
        let g1 = [1.0_f32, 2.0, 1.5, 1.8, 2.1];
        let g2 = [4.0_f32, 5.0, 4.5, 4.8, 5.1];
        let g3 = [8.0_f32, 9.0, 8.5, 8.8, 9.1];
        let r = one_way_anova(&[&g1[..], &g2[..], &g3[..]]).unwrap();
        assert!(r.statistic > 50.0);
        assert!(r.p_value < 1e-6);
        // η² near 1 — almost all variance is between-group.
        assert!(r.effect_size.unwrap() > 0.9);
    }

    /// Kruskal-Wallis matches χ² intuition for clearly separated groups.
    #[test]
    fn kruskal_detects_separated_groups() {
        let g1 = [1.0_f32, 2.0, 3.0];
        let g2 = [10.0_f32, 11.0, 12.0];
        let g3 = [20.0_f32, 21.0, 22.0];
        let r = kruskal_wallis(&[&g1[..], &g2[..], &g3[..]]).unwrap();
        assert!(r.p_value < 0.05);
        assert!(r.statistic > 5.0);
    }

    /// Auto-dispatch picks parametric path; non-parametric path on the
    /// same data should still reject H0.
    #[test]
    fn dispatch_paired_paths_agree_qualitatively() {
        let a = [10.0_f32, 11.0, 12.0, 9.0, 13.0];
        let b = [8.0_f32, 9.5, 10.5, 7.0, 11.0];
        let p_param = run_test(&[&a[..], &b[..]], Comparison::PairedTwo, TestKind::Parametric)
            .unwrap()
            .p_value;
        let p_np = run_test(&[&a[..], &b[..]], Comparison::PairedTwo, TestKind::NonParametric)
            .unwrap()
            .p_value;
        assert!(p_param < 0.05);
        assert!(p_np < 0.10);
    }

    /// Bonferroni vs BH on the same vector: BH ≤ Bonferroni elementwise.
    #[test]
    fn bh_never_exceeds_bonferroni() {
        let ps = [0.001, 0.01, 0.02, 0.05, 0.10, 0.20];
        let bonf = bonferroni(&ps);
        let bh = benjamini_hochberg(&ps);
        for (a, b) in bh.iter().zip(bonf.iter()) {
            assert!(*a <= *b + 1e-12, "BH={a} > Bonferroni={b}");
        }
    }
}
