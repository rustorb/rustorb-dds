//! Summaries of the measurements.

/// The distribution of a set of durations, in ns.
#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    /// How many.
    pub count: usize,
    /// The smallest.
    pub min: u64,
    /// The mean.
    pub mean: u64,
    /// The median.
    pub p50: u64,
    /// The 90th percentile.
    pub p90: u64,
    /// The 99th percentile.
    pub p99: u64,
    /// The 99.9th percentile.
    pub p999: u64,
    /// The largest.
    pub max: u64,
}

impl Summary {
    /// Summarizes `samples` (sorting them). Keeping them all and
    /// sorting them gives exact percentiles without a dependency.
    pub fn of(samples: &mut [u64]) -> Summary {
        if samples.is_empty() {
            return Summary::default();
        }
        samples.sort_unstable();
        let n = samples.len();
        // The nearest-rank percentile: the smallest value with at least
        // p of the samples at or below it.
        let at = |p: f64| samples[((p * n as f64).ceil() as usize).clamp(1, n) - 1];
        Summary {
            count: n,
            min: samples[0],
            mean: (samples.iter().map(|&s| s as u128).sum::<u128>() / n as u128) as u64,
            p50: at(0.50),
            p90: at(0.90),
            p99: at(0.99),
            p999: at(0.999),
            max: samples[n - 1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let mut v: Vec<u64> = (1..=1000).rev().collect();
        let s = Summary::of(&mut v);
        assert_eq!(s.count, 1000);
        assert_eq!((s.min, s.max), (1, 1000));
        assert_eq!((s.p50, s.p90, s.p99, s.p999), (500, 900, 990, 999));
        assert_eq!(s.mean, 500);
        let mut one = [7];
        let s = Summary::of(&mut one);
        assert_eq!((s.p50, s.p999, s.max), (7, 7, 7));
        assert_eq!(Summary::of(&mut []).count, 0);
    }
}
