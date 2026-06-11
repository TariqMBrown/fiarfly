//! Multiple-comparisons p-value correction.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultipleComparisonsCorrection {
    None,
    Bonferroni,
    /// Benjamini-Hochberg false-discovery rate.
    BenjaminiHochberg,
}

/// Adjust raw p-values in-place semantics — returns a new vector of the
/// same length, in the *original* order.
pub fn adjust(ps: &[f64], method: MultipleComparisonsCorrection) -> Vec<f64> {
    match method {
        MultipleComparisonsCorrection::None => ps.to_vec(),
        MultipleComparisonsCorrection::Bonferroni => bonferroni(ps),
        MultipleComparisonsCorrection::BenjaminiHochberg => benjamini_hochberg(ps),
    }
}

pub fn bonferroni(ps: &[f64]) -> Vec<f64> {
    let m = ps.len() as f64;
    ps.iter().map(|p| (p * m).min(1.0)).collect()
}

/// Benjamini-Hochberg FDR-adjusted p-values, monotone-corrected.
///
/// Same algorithm as `p.adjust(method="BH")` in R / `multipletests` in
/// statsmodels.
pub fn benjamini_hochberg(ps: &[f64]) -> Vec<f64> {
    let m = ps.len();
    if m == 0 {
        return Vec::new();
    }
    // Sort ascending.
    let mut indexed: Vec<(usize, f64)> = ps.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Raw BH-adjusted: q_i = p_i * m / rank
    let mut adj = vec![0.0_f64; m];
    for (rank0, (_orig, p)) in indexed.iter().enumerate() {
        let rank = rank0 + 1; // 1-based
        adj[rank0] = (*p * m as f64 / rank as f64).min(1.0);
    }
    // Monotone fix: walk from the largest p downward, taking running min.
    for i in (0..m - 1).rev() {
        if adj[i + 1] < adj[i] {
            adj[i] = adj[i + 1];
        }
    }
    // Scatter back to original order.
    let mut out = vec![0.0_f64; m];
    for (rank0, (orig, _)) in indexed.iter().enumerate() {
        out[*orig] = adj[rank0];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bonferroni_scales_and_caps() {
        let ps = [0.01, 0.04, 0.5];
        let out = bonferroni(&ps);
        assert!((out[0] - 0.03).abs() < 1e-9);
        assert!((out[1] - 0.12).abs() < 1e-9);
        assert!((out[2] - 1.0).abs() < 1e-9); // capped
    }

    #[test]
    fn bh_matches_known_values() {
        // R: p.adjust(c(0.01, 0.04, 0.03, 0.005), method="BH")
        // → 0.020, 0.040, 0.040, 0.020
        let ps = [0.01, 0.04, 0.03, 0.005];
        let out = benjamini_hochberg(&ps);
        let expected = [0.02, 0.04, 0.04, 0.02];
        for (a, e) in out.iter().zip(expected.iter()) {
            assert!((a - e).abs() < 1e-9, "got {a}, expected {e}");
        }
    }

    #[test]
    fn bh_is_monotone() {
        let ps = [0.001, 0.01, 0.02, 0.5, 0.6];
        let out = benjamini_hochberg(&ps);
        for w in out.windows(2) {
            assert!(w[0] <= w[1] + 1e-12, "non-monotone: {out:?}");
        }
    }

    #[test]
    fn empty_inputs_return_empty() {
        assert!(benjamini_hochberg(&[]).is_empty());
        assert!(bonferroni(&[]).is_empty());
    }
}
