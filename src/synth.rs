// synth.rs — Deterministic synthetic market data generator.
//
// Seeded LCG random walk with a mid-series volatility regime shift, so
// the demo data exercises the ATR sizing under changing conditions.
// Same seed -> byte-identical data -> identical replay -> identical
// session SHA. No real market data ships with this repository.

use crate::collector::Bar;
use std::path::Path;

/// Minimal 64-bit LCG (Knuth constants). Not cryptographic — it only
/// needs to be deterministic and well-spread for demo data.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Approximately standard normal (Irwin–Hall sum of 6 uniforms).
    pub fn next_gauss(&mut self) -> f64 {
        let s: f64 = (0..6).map(|_| self.next_f64()).sum();
        (s - 3.0) * (12.0_f64 / 6.0).sqrt()
    }
}

/// Generate `n` deterministic hourly OHLCV bars from `seed`.
pub fn generate_bars(seed: u64, n: usize) -> Vec<Bar> {
    let mut rng = Lcg::new(seed);
    let mut bars = Vec::with_capacity(n);

    let t0 = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();

    let mut close = 100.0_f64;

    for i in 0..n {
        // Volatility regime shift in the middle third of the series.
        let base_vol = if i >= n / 3 && i < 2 * n / 3 { 0.016 } else { 0.008 };
        let drift = 0.0001;

        let open = close;
        let ret = drift + base_vol * rng.next_gauss();
        close = (open * (1.0 + ret)).max(0.01);

        let wick_hi = open.max(close) * (1.0 + 0.35 * base_vol * rng.next_f64());
        let wick_lo = open.min(close) * (1.0 - 0.35 * base_vol * rng.next_f64());
        let volume = 1000.0 + 2500.0 * rng.next_f64();

        let ts = t0 + chrono::Duration::hours(i as i64);
        bars.push(Bar {
            index: i,
            timestamp: ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            open,
            high: wick_hi,
            low: wick_lo,
            close,
            volume,
        });
    }
    bars
}

/// Write a section-delimited sample report file containing the synthetic
/// bars — the on-disk format the collector parses.
pub fn write_sample(path: &Path, seed: u64, n: usize) -> Result<(), String> {
    let bars = generate_bars(seed, n);

    let mut out = String::new();
    out.push_str("=== SESSION HEADER ===\n");
    out.push_str("generator,seed,bars\n");
    out.push_str(&format!("synthetic-lcg,{},{}\n", seed, n));
    out.push('\n');
    out.push_str("=== MARKET DATA ===\n");
    out.push_str("index,timestamp,open,high,low,close,volume\n");
    for b in &bars {
        out.push_str(&format!(
            "{},{},{:.4},{:.4},{:.4},{:.4},{:.1}\n",
            b.index, b.timestamp, b.open, b.high, b.low, b.close, b.volume
        ));
    }

    std::fs::write(path, out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic() {
        let a = generate_bars(42, 100);
        let b = generate_bars(42, 100);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.close, y.close);
        }
    }

    #[test]
    fn test_ohlc_sane() {
        for b in generate_bars(1, 500) {
            assert!(b.high >= b.open && b.high >= b.close);
            assert!(b.low <= b.open && b.low <= b.close);
            assert!(b.close > 0.0);
        }
    }
}
