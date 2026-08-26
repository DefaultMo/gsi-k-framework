// signal.rs — The signal side of the signal/execution split.
//
// The framework is deliberately agnostic about WHAT decides to trade.
// A signal provider consumes bars and emits a direction; the execution
// side (backtest.rs) turns directions into positions, brackets, and
// trade records. The two sides communicate only through this trait.
//
// ─────────────────────────────────────────────────────────────────
// NOTE: The signal shipped here (SmaCross) is ILLUSTRATIVE ONLY.
// It is a textbook moving-average cross included so the framework
// compiles and the replay harness runs end-to-end. The production
// signal model, its parameters, and its research are proprietary
// and are not part of this repository — by design.
// ─────────────────────────────────────────────────────────────────

use crate::collector::Bar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Long,
    Short,
    None,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Long  => write!(f, "LONG"),
            Direction::Short => write!(f, "SHORT"),
            Direction::None  => write!(f, "NONE"),
        }
    }
}

/// A signal provider observes bars one at a time and emits a direction.
/// Implementations must be deterministic: the same bar sequence must
/// always produce the same signal sequence (this is what makes replay
/// and the SHA-256 session chain meaningful).
pub trait SignalProvider {
    /// Human-readable name, embedded in the session fingerprint.
    fn name(&self) -> String;

    /// Number of bars the provider needs before it can emit signals.
    fn warmup_bars(&self) -> usize;

    /// Observe the next bar; return a direction (or None).
    fn on_bar(&mut self, bar: &Bar) -> Direction;
}

/// Illustrative toy signal: simple moving average cross.
/// Long when the fast SMA crosses above the slow SMA, short when it
/// crosses below. This is NOT the production signal.
pub struct SmaCross {
    fast:      usize,
    slow:      usize,
    closes:    Vec<f64>,
    prev_diff: Option<f64>,
}

impl SmaCross {
    pub fn new(fast: usize, slow: usize) -> Result<Self, String> {
        if fast == 0 || slow == 0 {
            return Err("SMA periods must be positive".into());
        }
        if fast >= slow {
            return Err(format!("fast ({}) must be < slow ({})", fast, slow));
        }
        Ok(SmaCross { fast, slow, closes: Vec::new(), prev_diff: None })
    }

    fn sma(&self, period: usize) -> Option<f64> {
        let n = self.closes.len();
        if n < period {
            return None;
        }
        Some(self.closes[n - period..].iter().sum::<f64>() / period as f64)
    }
}

impl SignalProvider for SmaCross {
    fn name(&self) -> String {
        format!("sma_cross_{}_{} (illustrative)", self.fast, self.slow)
    }

    fn warmup_bars(&self) -> usize {
        self.slow + 1
    }

    fn on_bar(&mut self, bar: &Bar) -> Direction {
        self.closes.push(bar.close);

        let (fast, slow) = match (self.sma(self.fast), self.sma(self.slow)) {
            (Some(f), Some(s)) => (f, s),
            _ => return Direction::None,
        };

        let diff = fast - slow;
        let signal = match self.prev_diff {
            Some(prev) if prev <= 0.0 && diff > 0.0 => Direction::Long,
            Some(prev) if prev >= 0.0 && diff < 0.0 => Direction::Short,
            _ => Direction::None,
        };
        self.prev_diff = Some(diff);
        signal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: usize, close: f64) -> Bar {
        Bar {
            index: i,
            timestamp: format!("t{}", i),
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        }
    }

    #[test]
    fn test_cross_fires_long() {
        let mut sig = SmaCross::new(2, 4).unwrap();
        // Falling then rising: expect a long cross at some point.
        let prices = [10.0, 9.0, 8.0, 7.0, 6.0, 7.5, 9.0, 11.0, 13.0];
        let mut fired = Vec::new();
        for (i, p) in prices.iter().enumerate() {
            let d = sig.on_bar(&bar(i, *p));
            if d != Direction::None {
                fired.push(d);
            }
        }
        assert!(fired.contains(&Direction::Long));
    }

    #[test]
    fn test_invalid_params() {
        assert!(SmaCross::new(30, 10).is_err());
        assert!(SmaCross::new(0, 10).is_err());
    }
}
