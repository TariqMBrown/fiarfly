//! Parametric tests: paired t, Welch's t, one-way ANOVA.

use statrs::distribution::{ContinuousCDF, FisherSnedecor, StudentsT};

use super::descriptive::{mean, sample_std};
use super::effect::{cohens_d, cohens_dz};
use super::TestResult;
use crate::FiarflyError;

/// Paired (within-subject) Student's t-test on `a − b`.
pub fn paired_t_test(a: &[f32], b: &[f32]) -> Result<TestResult, FiarflyError> {
    if a.len() != b.len() {
        return Err(FiarflyError::InvalidParameter(
            "paired_t_test: sample lengths must match".into(),
        ));
    }
    let n = a.len();
    if n < 2 {
        return Err(FiarflyError::InvalidParameter(
            "paired_t_test: need ≥ 2 pairs".into(),
        ));
    }
    let diffs: Vec<f32> = a.iter().zip(b.iter()).map(|(x, y)| x - y).collect();
    let m = mean(&diffs);
    let sd = sample_std(&diffs);
    if sd == 0.0 {
        return Ok(TestResult {
            test_name: "paired t",
            statistic: 0.0,
            p_value: 1.0,
            n: vec![n],
            effect_size: Some(0.0),
            effect_kind: Some("Cohen's dz"),
            note: Some("zero within-pair variance".into()),
        });
    }
    let t = m / (sd / (n as f64).sqrt());
    let df = (n - 1) as f64;
    let p = two_sided_t_p(t, df);
    let dz = cohens_dz(a, b);
    Ok(TestResult {
        test_name: "paired t",
        statistic: t,
        p_value: p,
        n: vec![n],
        effect_size: Some(dz),
        effect_kind: Some("Cohen's dz"),
        note: None,
    })
}

/// Welch's two-sample t-test (unequal variances, unequal n).
pub fn welch_t_test(a: &[f32], b: &[f32]) -> Result<TestResult, FiarflyError> {
    let na = a.len();
    let nb = b.len();
    if na < 2 || nb < 2 {
        return Err(FiarflyError::InvalidParameter(
            "welch_t_test: each group needs n ≥ 2".into(),
        ));
    }
    let ma = mean(a);
    let mb = mean(b);
    let va = sample_std(a).powi(2);
    let vb = sample_std(b).powi(2);
    if va == 0.0 && vb == 0.0 {
        let p = if (ma - mb).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        };
        return Ok(TestResult {
            test_name: "Welch t",
            statistic: 0.0,
            p_value: p,
            n: vec![na, nb],
            effect_size: Some(0.0),
            effect_kind: Some("Cohen's d"),
            note: Some("zero variance in both groups".into()),
        });
    }
    let se = (va / na as f64 + vb / nb as f64).sqrt();
    let t = (ma - mb) / se;
    // Welch–Satterthwaite df.
    let num = (va / na as f64 + vb / nb as f64).powi(2);
    let den =
        (va / na as f64).powi(2) / (na as f64 - 1.0) + (vb / nb as f64).powi(2) / (nb as f64 - 1.0);
    let df = if den > 0.0 {
        num / den
    } else {
        (na + nb - 2) as f64
    };
    let p = two_sided_t_p(t, df);
    let d = cohens_d(a, b);
    Ok(TestResult {
        test_name: "Welch t",
        statistic: t,
        p_value: p,
        n: vec![na, nb],
        effect_size: Some(d),
        effect_kind: Some("Cohen's d"),
        note: None,
    })
}

/// One-way ANOVA across ≥ 3 groups.
pub fn one_way_anova(groups: &[&[f32]]) -> Result<TestResult, FiarflyError> {
    if groups.len() < 2 {
        return Err(FiarflyError::InvalidParameter(
            "one_way_anova: need ≥ 2 groups".into(),
        ));
    }
    let total_n: usize = groups.iter().map(|g| g.len()).sum();
    if total_n < groups.len() + 1 {
        return Err(FiarflyError::InvalidParameter(
            "one_way_anova: insufficient observations".into(),
        ));
    }
    // Grand mean.
    let grand_sum: f64 = groups
        .iter()
        .flat_map(|g| g.iter())
        .map(|&x| x as f64)
        .sum();
    let grand_mean = grand_sum / total_n as f64;

    // Between- and within-group sums of squares.
    let mut ss_between = 0.0_f64;
    let mut ss_within = 0.0_f64;
    for g in groups {
        if g.is_empty() {
            continue;
        }
        let m = mean(g);
        ss_between += g.len() as f64 * (m - grand_mean).powi(2);
        for &x in g.iter() {
            ss_within += (x as f64 - m).powi(2);
        }
    }
    let k = groups.len() as f64;
    let df_between = k - 1.0;
    let df_within = total_n as f64 - k;
    if df_within <= 0.0 || ss_within <= 0.0 {
        return Ok(TestResult {
            test_name: "one-way ANOVA",
            statistic: f64::INFINITY,
            p_value: if ss_between > 0.0 { 0.0 } else { 1.0 },
            n: groups.iter().map(|g| g.len()).collect(),
            effect_size: Some(if ss_between > 0.0 { 1.0 } else { 0.0 }),
            effect_kind: Some("η²"),
            note: Some("zero within-group variance".into()),
        });
    }
    let ms_between = ss_between / df_between;
    let ms_within = ss_within / df_within;
    let f = ms_between / ms_within;
    let dist = FisherSnedecor::new(df_between, df_within)
        .map_err(|e| FiarflyError::InvalidParameter(format!("F-dist: {e}")))?;
    let p = 1.0 - dist.cdf(f);
    let eta_sq = ss_between / (ss_between + ss_within);
    Ok(TestResult {
        test_name: "one-way ANOVA",
        statistic: f,
        p_value: p,
        n: groups.iter().map(|g| g.len()).collect(),
        effect_size: Some(eta_sq),
        effect_kind: Some("η²"),
        note: None,
    })
}

fn two_sided_t_p(t: f64, df: f64) -> f64 {
    let dist = match StudentsT::new(0.0, 1.0, df) {
        Ok(d) => d,
        Err(_) => return f64::NAN,
    };
    let cdf = dist.cdf(-t.abs());
    (2.0 * cdf).clamp(0.0, 1.0)
}
