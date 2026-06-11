//! Small numeric helpers shared across the stats module.

pub fn mean(xs: &[f32]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let sum: f64 = xs.iter().map(|&x| x as f64).sum();
    sum / xs.len() as f64
}

/// Sample (Bessel-corrected) standard deviation. Returns 0.0 for n < 2.
pub fn sample_std(xs: &[f32]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let ss: f64 = xs.iter().map(|&x| (x as f64 - m).powi(2)).sum();
    (ss / (xs.len() - 1) as f64).sqrt()
}

pub fn sem(xs: &[f32]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    sample_std(xs) / (xs.len() as f64).sqrt()
}

/// Average ranks (with ties handled by mid-rank).
///
/// Returns rank values in the *original* order of `xs`.
pub fn average_ranks(xs: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut indexed: Vec<(f64, usize)> = xs
        .iter()
        .copied()
        .enumerate()
        .map(|(i, v)| (v, i))
        .collect();
    indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && (indexed[j].0 - indexed[i].0).abs() < f64::EPSILON {
            j += 1;
        }
        // Average rank for the run [i..j) is (i + j - 1) / 2 + 1 in 1-based.
        let avg = (i as f64 + j as f64 - 1.0) / 2.0 + 1.0;
        for k in i..j {
            ranks[indexed[k].1] = avg;
        }
        i = j;
    }
    ranks
}

/// Tie-correction term for rank-based tests:
/// `Σ (t³ − t)` over each tie group of size `t`.
pub fn tie_correction(xs: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut tie_sum = 0.0_f64;
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i + 1;
        while j < sorted.len() && (sorted[j] - sorted[i]).abs() < f64::EPSILON {
            j += 1;
        }
        let t = (j - i) as f64;
        if t > 1.0 {
            tie_sum += t.powi(3) - t;
        }
        i = j;
    }
    tie_sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_and_std_match_hand_calc() {
        let xs = [1.0_f32, 2.0, 3.0, 4.0, 5.0];
        assert!((mean(&xs) - 3.0).abs() < 1e-9);
        // Sample std of 1..5 is sqrt(2.5) ≈ 1.58113883
        assert!((sample_std(&xs) - 1.581_138_830_084_2).abs() < 1e-6);
    }

    #[test]
    fn ranks_with_ties_use_midrank() {
        let xs = [1.0_f64, 2.0, 2.0, 3.0];
        // 1 → 1, ties at value 2 → average of ranks 2 and 3 = 2.5, 3 → 4.
        let ranks = average_ranks(&xs);
        assert_eq!(ranks, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn tie_correction_zero_when_no_ties() {
        let xs = [1.0_f64, 2.0, 3.0];
        assert_eq!(tie_correction(&xs), 0.0);
    }
}
