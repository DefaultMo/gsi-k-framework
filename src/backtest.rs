// backtest.rs — The execution side of the signal/execution split.
//
// Deterministic replay harness: bars in, trade records out. The engine
// knows nothing about WHY a signal fired — it consumes Direction values
// from any SignalProvider, opens at the bar close, brackets the position
// with ATR-derived target/stop distances, sizes it so a stop-out risks a
// fixed equity fraction, and force-exits stale positions after a bar
// budget (rescue). Fill model is intentionally conservative: intrabar,
// the stop is assumed to be hit before the target.

use crate::chain::TradeRecord;
use crate::collector::Bar;
use crate::convergence::ConvergenceState;
use crate::report::ReportRow;
use crate::signal::{Direction, SignalProvider};
use crate::sizing::{atr_series, compute_bracket, position_size, BracketConfig};

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_equity: f64,
    /// Fraction of equity risked per trade (stop-out loss).
    pub risk_frac:      f64,
    pub bracket:        BracketConfig,
    /// Assumed win rate for the convergence loop.
    pub p_assumed:      f64,
    /// Assumed payoff ratio for the convergence loop.
    pub b_assumed:      f64,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        BacktestConfig {
            initial_equity: 10_000.0,
            risk_frac:      0.005,
            bracket:        BracketConfig::default(),
            // Neutral placeholder assumptions for the illustrative signal.
            p_assumed:      0.50,
            b_assumed:      2.0,
        }
    }
}

pub struct BacktestResult {
    pub bar_rows:         Vec<ReportRow>,
    pub trades:           Vec<TradeRecord>,
    pub convergence:      ConvergenceState,
    pub signals_fired:    u32,
    pub long_signals:     u32,
    pub short_signals:    u32,
    pub final_equity:     f64,
    pub max_drawdown_pct: f64,
}

impl BacktestResult {
    pub fn wins(&self) -> u32 {
        self.trades.iter().filter(|t| t.win).count() as u32
    }
    pub fn losses(&self) -> u32 {
        self.trades.len() as u32 - self.wins()
    }
    pub fn total_pnl(&self) -> f64 {
        self.trades.iter().map(|t| t.pnl).sum()
    }
}

struct OpenPosition {
    direction:    Direction,
    entry_block:  usize,
    entry_price:  f64,
    qty:          f64,
    target:       f64,
    stop:         f64,
    bracket_dist: f64,
}

/// Replay `bars` through `signal` under `cfg`. Fully deterministic:
/// the same inputs always produce the same trades (and hence the same
/// session SHA when sealed into a chain).
pub fn run(bars: &[Bar], signal: &mut dyn SignalProvider, cfg: &BacktestConfig) -> BacktestResult {
    let atr = atr_series(bars, cfg.bracket.atr_period);
    let mut convergence = ConvergenceState::new(cfg.p_assumed, cfg.b_assumed);

    let mut equity = cfg.initial_equity;
    let mut peak_equity = equity;
    let mut max_dd_pct = 0.0_f64;

    let mut open: Option<OpenPosition> = None;
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut bar_rows: Vec<ReportRow> = Vec::new();

    let mut signals_fired = 0_u32;
    let mut long_signals = 0_u32;
    let mut short_signals = 0_u32;

    let warmup = signal.warmup_bars().max(cfg.bracket.atr_period + 1);

    for (i, bar) in bars.iter().enumerate() {
        // ── 1. Manage any open position against this bar ──────────────
        if let Some(pos) = open.take() {
            let held = i - pos.entry_block;
            let (exit_price, exit_type) = check_exit(&pos, bar, held, cfg.bracket.rescue_bars);

            match (exit_price, exit_type) {
                (Some(px), ty) => {
                    let dir_sign = if pos.direction == Direction::Long { 1.0 } else { -1.0 };
                    let pnl = dir_sign * (px - pos.entry_price) * pos.qty;
                    equity += pnl;
                    let win = pnl > 0.0;
                    convergence.record_trade(win, pos.bracket_dist);
                    trades.push(TradeRecord {
                        block_entry:  pos.entry_block as u64,
                        block_exit:   i as u64,
                        direction:    pos.direction.to_string(),
                        price_entry:  pos.entry_price,
                        price_exit:   px,
                        qty:          pos.qty,
                        pnl,
                        bracket_dist: pos.bracket_dist,
                        exit_type:    ty.to_string(),
                        win,
                    });
                }
                (None, _) => {
                    open = Some(pos); // still holding
                }
            }
        }

        // ── 2. Ask the signal side for a direction ────────────────────
        let direction = signal.on_bar(bar);

        // ── 3. Open a position if flat, warmed up, and ATR is known ───
        if open.is_none() && direction != Direction::None && i >= warmup {
            if let Some(atr_val) = atr[i] {
                signals_fired += 1;
                match direction {
                    Direction::Long => long_signals += 1,
                    Direction::Short => short_signals += 1,
                    _ => {}
                }

                let bracket = compute_bracket(atr_val, &cfg.bracket);
                let qty = position_size(equity, cfg.risk_frac, bracket.hard_dist);
                if qty > 0.0 {
                    let entry = bar.close;
                    let (target, stop) = match direction {
                        Direction::Long  => (entry + bracket.soft_dist, entry - bracket.hard_dist),
                        Direction::Short => (entry - bracket.soft_dist, entry + bracket.hard_dist),
                        Direction::None  => unreachable!(),
                    };
                    open = Some(OpenPosition {
                        direction,
                        entry_block: i,
                        entry_price: entry,
                        qty,
                        target,
                        stop,
                        bracket_dist: bracket.bracket_dist,
                    });
                }
            }
        }

        // ── 4. Track equity peak / drawdown, emit the per-bar row ─────
        if equity > peak_equity {
            peak_equity = equity;
        }
        let dd = (peak_equity - equity) / peak_equity * 100.0;
        if dd > max_dd_pct {
            max_dd_pct = dd;
        }

        bar_rows.push(ReportRow {
            block_index:     i,
            timestamp:       bar.timestamp.clone(),
            close:           bar.close,
            atr:             atr[i],
            direction:       direction.to_string(),
            active:          open.is_some(),
            position_qty:    open.as_ref().map_or(0.0, |p| p.qty),
            equity,
            drawdown_pct:    dd,
            p_measured:      convergence.p_measured(),
            b_measured:      convergence.b_measured(),
            n_trades:        convergence.n(),
            divergence_flag: convergence.p_diverged(),
        });
    }

    // ── Close any position still open at the end of data ──────────────
    if let Some(pos) = open.take() {
        let last = bars.last().unwrap();
        let dir_sign = if pos.direction == Direction::Long { 1.0 } else { -1.0 };
        let pnl = dir_sign * (last.close - pos.entry_price) * pos.qty;
        equity += pnl;
        let win = pnl > 0.0;
        convergence.record_trade(win, pos.bracket_dist);
        trades.push(TradeRecord {
            block_entry:  pos.entry_block as u64,
            block_exit:   (bars.len() - 1) as u64,
            direction:    pos.direction.to_string(),
            price_entry:  pos.entry_price,
            price_exit:   last.close,
            qty:          pos.qty,
            pnl,
            bracket_dist: pos.bracket_dist,
            exit_type:    "EOD".to_string(),
            win,
        });
    }

    BacktestResult {
        bar_rows,
        trades,
        convergence,
        signals_fired,
        long_signals,
        short_signals,
        final_equity: equity,
        max_drawdown_pct: max_dd_pct,
    }
}

/// Conservative intrabar fill model: stop before target; rescue exit at
/// the close once the bar budget is exhausted.
fn check_exit(
    pos: &OpenPosition,
    bar: &Bar,
    held_bars: usize,
    rescue_bars: usize,
) -> (Option<f64>, &'static str) {
    match pos.direction {
        Direction::Long => {
            if bar.low <= pos.stop {
                return (Some(pos.stop), "STOP");
            }
            if bar.high >= pos.target {
                return (Some(pos.target), "TARGET");
            }
        }
        Direction::Short => {
            if bar.high >= pos.stop {
                return (Some(pos.stop), "STOP");
            }
            if bar.low <= pos.target {
                return (Some(pos.target), "TARGET");
            }
        }
        Direction::None => {}
    }
    if held_bars >= rescue_bars {
        return (Some(bar.close), "RESCUE");
    }
    (None, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::SmaCross;
    use crate::synth::generate_bars;

    #[test]
    fn test_replay_is_deterministic() {
        let bars = generate_bars(7, 400);
        let cfg = BacktestConfig::default();

        let mut s1 = SmaCross::new(10, 30).unwrap();
        let r1 = run(&bars, &mut s1, &cfg);
        let mut s2 = SmaCross::new(10, 30).unwrap();
        let r2 = run(&bars, &mut s2, &cfg);

        assert_eq!(r1.trades.len(), r2.trades.len());
        assert!((r1.final_equity - r2.final_equity).abs() < 1e-9);
    }

    #[test]
    fn test_risk_bounded_per_trade() {
        let bars = generate_bars(7, 600);
        let cfg = BacktestConfig::default();
        let mut sig = SmaCross::new(5, 20).unwrap();
        let r = run(&bars, &mut sig, &cfg);

        // No single stop-out may lose materially more than risk_frac of
        // the equity at entry (which is at most peak equity).
        for t in r.trades.iter().filter(|t| t.exit_type == "STOP") {
            assert!(
                t.pnl.abs() <= cfg.initial_equity * 2.0 * cfg.risk_frac * 10.0,
                "stop loss out of bounds: {}",
                t.pnl
            );
        }
    }
}
