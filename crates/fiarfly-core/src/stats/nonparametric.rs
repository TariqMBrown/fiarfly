//! Non-parametric tests: Mann-Whitney U, Wilcoxon signed-rank,
//! Kruskal-Wallis. Normal-approximation p-values with tie correction;
//! suitable for typical calcium-imaging sample sizes (n ≥ 8 per group).

use statrs::distribution::{ChiSquared, ContinuousCDF, Normal};

use super::descriptive::{average_ranks, tie_correction};
use super::effect::rank_biserial;
use super::TestResult;
use crate::FiarflyError;

/// Mann-Whitney U (independent two-sample, two-sided).
///
/// p-value uses the normal approximation with continuity correction and
/// tie correction; matches SciPy's `mannwhitneyu(..., method="asymptotic")`.
pub fn mann_whitney_u(a: &[f32], b: &[f32]) -> Result<TestResult, FiarflyError> {
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return Err(FiarflyError::InvalidParameter(
            "mann_whitney_u: empty group".into(),
        ));
    }
    let mut combined: Vec<f64> = Vec::with_capacity(n1 + n2);
    combined.extend(a.iter().map(|&x| x as f64));
    combined.extend(b.iter().map(|&x| x as f64));
    let ranks = average_ranks(&combined);
    let r1: f64 = ranks[..n1].iter().sum();
    let u1 = r1 - n1 as f64 * (n1 as f64 + 1.0) / 2.0;
    let u2 = n1 as f64 * n2 as f64 - u1;
    let u = u1.min(u2);

    let n = n1 as f64 + n2 as f64;
    let mu = n1 as f64 * n2 as f64 / 2.0;
    let tie_term = tie_correction(&combined);
    let variance = n1 as f64 * n2 as f64 / 12.0
        * ((n + 1.0) - tie_term / (n * (n - 1.0)));
    let p = if variance <= 0.0 {
        1.0
    } else {
        let sigma = variance.sqrt();
        let z = ((u - mu).abs() - 0.5) / sigma; // continuity correction
        let normal = Normal::new(0.0, 1.0).unwrap();
        2.0 * (1.0 - normal.cdf(z))
    };
    let r = rank_biserial(a, b);
    Ok(TestResult {
        test_name: "Mann-Whitney U",
        statistic: u,
        p_value: p.clamp(0.0, 1.0),
        n: vec![n1, n2],
        effect_size: Some(r),
        effect_kind: Some("rank-biserial"),
        note: None,
    })
}

/// Wilcoxon signed-rank test (paired, two-sided).
pub fn wilcoxon_signed_rank(a: &[f32], b: &[f32]) -> Result<TestResult, FiarflyError> {
    if a.len() != b.len() {
        return Err(FiarflyError::InvalidParameter(
            "wilcoxon: lengths must match".into(),
        ));
    }
    if a.is_empty() {
        return Err(FiarflyError::InvalidParameter(
            "wilcoxon: empty inputs".into(),
        ));
    }

    // Drop zero differences.
    let pairs: Vec<(f64, f64)> = a
        .iter()
        .zip(b.iter())
        .filter_map(|(x, y)| {
            let d = *x as f64 - *y as f64;
            if d == 0.0 { None } else { Some((d.abs(), d)) }
        })
        .collect();
    let n = pairs.len();
    if n < 2 {
        return Err(FiarflyError::InvalidParameter(
            "wilcoxon: need ≥ 2 non-zero differences".into(),
        ));
    }

    let abs_diffs: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    let ranks = average_ranks(&abs_diffs);
    let mut w_plus = 0.0_f64;
    let mut w_minus = 0.0_f64;
    for (i, (_, signed)) in pairs.iter().enumerate() {
        if *signed > 0.0 {
            w_plus += ranks[i];
        } else {
            w_minus += ranks[i];
        }
    }
    let w = w_plus.min(w_minus);

    let mu = n as f64 * (n as f64 + 1.0) / 4.0;
    let tie_term = tie_correction(&abs_diffs);
    let variance = n as f64 * (n as f64 + 1.0) * (2.0 * n as f64 + 1.0) / 24.0
        - tie_term / 48.0;
    let p = if variance <= 0.0 {
        1.0
    } else {
        let sigma = variance.sqrt();
        let z = ((w - mu).abs() - 0.5) / sigma;
        let normal = Normal::new(0.0, 1.0).unwrap();
        2.0 * (1.0 - normal.cdf(z))
    };

    // Matched-pairs rank-biserial: r = (W+ − W−) / (n(n+1)/2)
    let total = n as f64 * (n as f64 + 1.0) / 2.0;
    let r = if total > 0.0 { (w_plus - w_minus) / total } else { 0.0 };
    Ok(TestResult {
        test_name: "Wilcoxon signed-rank",
        statistic: w,
        p_value: p.clamp(0.0, 1.0),
        n: vec![n],
        effect_size: Some(r),
        effect_kind: Some("rank-biserial"),
        note: None,
    })
}

/// Kruskal-Wallis test (≥ 3 independent groups).
pub fn kruskal_wallis(groups: &[&[f32]]) -> Result<TestResult, FiarflyError> {
    if groups.len() < 2 {
        return Err(FiarflyError::InvalidParameter(
            "kruskal_wallis: need ≥ 2 groups".into(),
        ));
    }
    let mut combined: Vec<f64> = Vec::new();
    let mut sizes: Vec<usize> = Vec::with_capacity(groups.len());
    for g in groups {
        sizes.push(g.len());
        combined.extend(g.iter().map(|&x| x as f64));
    }
    let n: usize = sizes.iter().sum();
    if n < groups.len() + 1 {
        return Err(FiarflyError::InvalidParameter(
            "kruskal_wallis: insufficient data".into(),
        ));
    }
    let ranks = average_ranks(&combined);

    // Sum of ranks per group.
    let mut h = 0.0_f64;
    let mut idx = 0_usize;
    for &nj in &sizes {
        if nj == 0 {
            continue;
        }
        let r_sum: f64 = ranks[idx..idx + nj].iter().sum();
        h += r_sum.powi(2) / nj as f64;
        idx += nj;
    }
    let nf = n as f64;
    h = (12.0 / (nf * (nf + 1.0))) * h - 3.0 * (nf + 1.0);

    // Tie correction.
    let tie_term = tie_correction(&combined);
    let denom = 1.0 - tie_term / (nf.powi(3) - nf);
    if denom > 0.0 {
        h /= denom;
    }

    let df = (groups.len() - 1) as f64;
    let dist = ChiSquared::new(df).map_err(|e| {
        FiarflyError::InvalidParameter(format!("chi-squared df={df}: {e}"))
    })?;
    let p = 1.0 - dist.cdf(h);
    // Eta-squared from H (Tomczak & Tomczak 2014).
    let eta_sq = ((h - groups.len() as f64 + 1.0) / (nf - groups.len() as f64))
        .max(0.0);
    Ok(TestResult {
        test_name: "Kruskal-Wallis H",
        statistic: h,
        p_value: p.clamp(0.0, 1.0),
        n: sizes,
        effect_size: Some(eta_sq),
        effect_kind: Some("η²H"),
        note: None,
    })
}
