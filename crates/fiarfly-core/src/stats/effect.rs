//! Effect-size estimators.

use super::descriptive::{mean, sample_std};

/// Cohen's d for two independent samples (pooled SD).
pub fn cohens_d(a: &[f32], b: &[f32]) -> f64 {
    let na = a.len();
    let nb = b.len();
    if na < 2 || nb < 2 {
        return 0.0;
    }
    let ma = mean(a);
    let mb = mean(b);
    let sa = sample_std(a);
    let sb = sample_std(b);
    let pooled = (((na as f64 - 1.0) * sa.powi(2) + (nb as f64 - 1.0) * sb.powi(2))
        / (na as f64 + nb as f64 - 2.0))
        .sqrt();
    if pooled == 0.0 {
        return 0.0;
    }
    (ma - mb) / pooled
}

/// Cohen's dz for a paired design (mean diff / sd of diffs).
pub fn cohens_dz(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let diffs: Vec<f32> = a.iter().zip(b.iter()).map(|(x, y)| x - y).collect();
    let sd = sample_std(&diffs);
    if sd == 0.0 {
        return 0.0;
    }
    mean(&diffs) / sd
}

/// Simple rank-biserial correlation for two independent samples.
/// Equivalent to `(2 * U1 / (n1*n2)) − 1` ignoring ties.
pub fn rank_biserial(a: &[f32], b: &[f32]) -> f64 {
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return 0.0;
    }
    let mut wins = 0.0_f64;
    for &x in a {
        for &y in b {
            if x > y {
                wins += 1.0;
            } else if (x - y).abs() < f32::EPSILON {
                wins += 0.5;
            }
        }
    }
    2.0 * wins / (n1 as f64 * n2 as f64) - 1.0
}
