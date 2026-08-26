// sizing.rs — ATR-based bracket construction and position sizing.
//
// Execution-side plumbing: given a volatility estimate (ATR), build the
// bracket (profit target / stop / time-based rescue) and size the
// position so a stop-out risks a fixed fraction of equity.
//
// The multipliers used by the demo are conventional illustrative values,
// not tuned parameters.

use crate::collector::Bar;

/// Bracket distances in price units.
#[derive(Debug, Clone)]
pub struct BracketLevels {
    /// Base bracket unit (ATR x atr_mult).
    pub bracket_dist: f64,
    /// Profit target distance from entry.
    pub soft_dist:    f64,
    /// Stop-loss distance from entry.
    pub hard_dist:    f64,
}

#[derive(Debug, Clone)]
pub struct BracketConfig {
    /// ATR lookback period.
    pub atr_period:   usize,
    /// Base bracket = ATR x this multiplier.
    pub atr_mult:     f64,
    /// Target = bracket x this ratio.
    pub target_ratio: f64,
    /// Stop = bracket x this ratio.
    pub stop_ratio:   f64,
    /// Force-exit an open position after this many bars (time-based rescue).
    pub rescue_bars:  usize,
}

impl Default for BracketConfig {
    fn default() -> Self {
        BracketConfig {
            atr_period:   14,
            atr_mult:     1.0,
            target_ratio: 2.0,
            stop_ratio:   1.0,
            rescue_bars:  48,
        }
    }
}

/// True range of a bar given the previous close.
pub fn true_range(prev_close: f64, bar: &Bar) -> f64 {
    let hl = bar.high - bar.low;
    let hc = (bar.high - prev_close).abs();
    let lc = (bar.low - prev_close).abs();
    hl.max(hc).max(lc)
}

/// Wilder-smoothed ATR series aligned to `bars`.
/// Entry i is None until enough history exists (period bars of true range).
pub fn atr_series(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.len() <= period {
        return out;
    }

    // True range needs a previous close, so tr[i] exists for i >= 1.
    let mut tr = vec![0.0; bars.len()];
    for i in 1..bars.len() {
        tr[i] = true_range(bars[i - 1].close, &bars[i]);
    }

    // Seed: simple average of the first `period` true ranges.
    let seed: f64 = tr[1..=period].iter().sum::<f64>() / period as f64;
    out[period] = Some(seed);

    // Wilder smoothing thereafter.
    let mut prev = seed;
    for i in (period + 1)..bars.len() {
        let atr = (prev * (period as f64 - 1.0) + tr[i]) / period as f64;
        out[i] = Some(atr);
        prev = atr;
    }
    out
}

/// Build bracket levels from an ATR value.
pub fn compute_bracket(atr_value: f64, cfg: &BracketConfig) -> BracketLevels {
    let bracket_dist = atr_value * cfg.atr_mult;
    BracketLevels {
        bracket_dist,
        soft_dist: bracket_dist * cfg.target_ratio,
        hard_dist: bracket_dist * cfg.stop_ratio,
    }
}

/// Risk-based position size: quantity such that a stop-out loses
/// `risk_frac` of current equity.
pub fn position_size(equity: f64, risk_frac: f64, stop_dist: f64) -> f64 {
    if stop_dist <= 0.0 {
        return 0.0;
    }
    (equity * risk_frac) / stop_dist
}

/// Fractional-Kelly shrinkage: f_safe = k / (1 + k).
/// Generic utility — maps a raw Kelly percentage to a conservative
/// fraction that can never reach full Kelly.
#[allow(dead_code)] // utility for downstream signal integrations
pub fn f_safe(kelly_pct: f64) -> f64 {
    let k = kelly_pct / 100.0;
    k / (1.0 + k) * 100.0 // returns percentage
}

/// Position notional from wallet total and a safe fraction percentage.
#[allow(dead_code)] // utility for downstream signal integrations
pub fn safe_bet(wallet_total: f64, f_safe_pct: f64) -> f64 {
    wallet_total * f_safe_pct / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: usize, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar {
            index: i,
            timestamp: format!("t{}", i),
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 1.0,
        }
    }

    #[test]
    fn test_atr_constant_range() {
        // Constant 1.0-wide bars: ATR converges to 1.0.
        let bars: Vec<Bar> = (0..40)
            .map(|i| bar(i, 100.0, 100.5, 99.5, 100.0))
            .collect();
        let atr = atr_series(&bars, 14);
        assert!(atr[13].is_none());
        let last = atr.last().unwrap().unwrap();
        assert!((last - 1.0).abs() < 1e-9, "ATR should be 1.0, got {}", last);
    }

    #[test]
    fn test_position_size_risk() {
        // 10_000 equity, 0.5% risk, stop 2.0 away -> qty 25, loss at stop = 50 = 0.5%.
        let qty = position_size(10_000.0, 0.005, 2.0);
        assert!((qty - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_f_safe_below_kelly() {
        assert!(f_safe(50.0) < 50.0);
        assert!(f_safe(0.0).abs() < 1e-12);
    }
}
