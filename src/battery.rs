// battery.rs — Full test battery + multi-sheet Excel report.
//
// One command produces a workbook a human can audit:
//   Sheet 1: framework self-checks with PASS/FAIL cell formatting
//   Sheet 2: signal parameter sweep (illustrative SMA grid)
//   Sheet 3: trade-by-trade ledger of the default run
//   Sheet 4: per-bar equity curve and drawdown
//   Sheet 5: convergence projections (Wilson CI vs. trade count)
//   Sheet 6: notes — what this battery does and does not claim

use crate::backtest::{run, BacktestConfig};
use crate::chain::SessionChain;
use crate::collector::load_bars;
use crate::signal::{SignalProvider, SmaCross};
use crate::sizing::atr_series;
use rust_xlsxwriter::*;
use std::path::Path;

pub fn run_battery(data_path: &Path, output_path: &Path) -> Result<(), String> {
    println!("GSI-K FRAMEWORK BATTERY");
    println!();

    let bars = load_bars(data_path)?;
    println!("Loaded {} bars from {}", bars.len(), data_path.display());

    let mut wb = Workbook::new();

    // Formats
    let hdr_fmt = Format::new()
        .set_bold().set_font_color(Color::White)
        .set_background_color(Color::RGB(0x1F4E79))
        .set_align(FormatAlign::Center);
    let pass_fmt = Format::new().set_background_color(Color::RGB(0xC6EFCE));
    let fail_fmt = Format::new().set_background_color(Color::RGB(0xFFC7CE));
    let num4_fmt = Format::new().set_num_format("0.0000");
    let num2_fmt = Format::new().set_num_format("0.00");
    let bold_fmt = Format::new().set_bold().set_font_size(12);
    let title_fmt = Format::new().set_bold().set_font_size(13)
        .set_font_color(Color::RGB(0x1F4E79));

    // ───────────────────────────────────────────────
    // SHEET 1: FRAMEWORK SELF-CHECKS
    // ───────────────────────────────────────────────
    let mut ws1 = wb.add_worksheet();
    ws1.set_name("1. Checks").map_err(|e| e.to_string())?;
    ws1.set_tab_color(Color::RGB(0x1F4E79));

    let headers1 = ["Check", "Detail", "Status"];
    for (c, h) in headers1.iter().enumerate() {
        ws1.write_string_with_format(0, c as u16, *h, &hdr_fmt).ok();
    }
    ws1.set_column_width(0, 42).ok();
    ws1.set_column_width(1, 55).ok();
    ws1.set_column_width(2, 10).ok();

    let mut row: u32 = 1;
    let mut pass_count = 0_u32;
    let mut fail_count = 0_u32;

    let check = |ws: &mut Worksheet, row: &mut u32, name: &str, detail: String, ok: bool,
                     pass_count: &mut u32, fail_count: &mut u32| {
        if ok { *pass_count += 1; } else { *fail_count += 1; }
        ws.write_string(*row, 0, name).ok();
        ws.write_string(*row, 1, &detail).ok();
        let fmt = if ok { &pass_fmt } else { &fail_fmt };
        ws.write_string_with_format(*row, 2, if ok { "PASS" } else { "FAIL" }, fmt).ok();
        *row += 1;
    };

    // Data integrity
    check(&mut ws1, &mut row, "Bars loaded", format!("{} bars", bars.len()),
        !bars.is_empty(), &mut pass_count, &mut fail_count);
    let monotonic = bars.windows(2).all(|w| w[1].index == w[0].index + 1);
    check(&mut ws1, &mut row, "Bar index monotonic", "index[i+1] == index[i]+1".into(),
        monotonic, &mut pass_count, &mut fail_count);
    let ohlc_ok = bars.iter().all(|b| {
        b.high >= b.low && b.high >= b.open && b.high >= b.close
            && b.low <= b.open && b.low <= b.close
    });
    check(&mut ws1, &mut row, "OHLC consistency", "high >= open,close,low; low <= open,close".into(),
        ohlc_ok, &mut pass_count, &mut fail_count);
    let finite_ok = bars.iter().all(|b| {
        b.open.is_finite() && b.high.is_finite() && b.low.is_finite()
            && b.close.is_finite() && b.volume.is_finite()
    });
    check(&mut ws1, &mut row, "All values finite", "no NaN/inf in OHLCV".into(),
        finite_ok, &mut pass_count, &mut fail_count);

    // ATR plumbing
    let atr = atr_series(&bars, 14);
    let atr_ready = atr.iter().skip(14).all(|a| a.map_or(false, |v| v > 0.0));
    check(&mut ws1, &mut row, "ATR computable after warmup", "ATR(14) > 0 for all bars past warmup".into(),
        atr_ready, &mut pass_count, &mut fail_count);

    // Replay determinism: run twice, compare trades + equity + session SHA
    let cfg = BacktestConfig::default();
    let mut sig_a = SmaCross::new(10, 30).map_err(|e| e)?;
    let result_a = run(&bars, &mut sig_a, &cfg);
    let mut sig_b = SmaCross::new(10, 30).map_err(|e| e)?;
    let result_b = run(&bars, &mut sig_b, &cfg);

    let mut chain_a = SessionChain::new("battery", &sig_a.name(), None);
    for t in &result_a.trades { chain_a.seal_trade(t.clone()); }
    let mut chain_b = SessionChain::new("battery", &sig_b.name(), None);
    for t in &result_b.trades { chain_b.seal_trade(t.clone()); }

    check(&mut ws1, &mut row, "Replay deterministic (trades)",
        format!("run A: {} trades, run B: {} trades", result_a.trades.len(), result_b.trades.len()),
        result_a.trades.len() == result_b.trades.len(), &mut pass_count, &mut fail_count);
    check(&mut ws1, &mut row, "Replay deterministic (equity)",
        format!("A: {:.2}, B: {:.2}", result_a.final_equity, result_b.final_equity),
        (result_a.final_equity - result_b.final_equity).abs() < 1e-9,
        &mut pass_count, &mut fail_count);
    check(&mut ws1, &mut row, "Session SHA reproducible",
        format!("{}...", &chain_a.session_sha()[..16]),
        chain_a.session_sha() == chain_b.session_sha(), &mut pass_count, &mut fail_count);

    // Risk bound: worst stop-out as fraction of initial equity
    let worst_stop = result_a.trades.iter()
        .filter(|t| t.exit_type == "STOP")
        .map(|t| t.pnl)
        .fold(0.0_f64, f64::min);
    let risk_bound_ok = worst_stop.abs() <= cfg.initial_equity * cfg.risk_frac * 3.0;
    check(&mut ws1, &mut row, "Stop-out risk bounded",
        format!("worst stop {:.2} vs {:.2}% risk budget", worst_stop, cfg.risk_frac * 100.0),
        risk_bound_ok, &mut pass_count, &mut fail_count);

    println!("Sheet 1: Checks — {} PASS, {} FAIL", pass_count, fail_count);

    // ───────────────────────────────────────────────
    // SHEET 2: PARAMETER SWEEP (illustrative signal)
    // ───────────────────────────────────────────────
    let ws2 = wb.add_worksheet();
    ws2.set_name("2. Param Sweep").map_err(|e| e.to_string())?;
    let headers2 = ["fast", "slow", "signals", "trades", "wins", "win_rate",
        "total_pnl", "final_equity", "max_dd_pct"];
    for (c, h) in headers2.iter().enumerate() {
        ws2.write_string_with_format(0, c as u16, *h, &hdr_fmt).ok();
    }
    for c in 0..9 { ws2.set_column_width(c, 12).ok(); }

    let mut row2: u32 = 1;
    for &fast in &[5_usize, 10, 15, 20] {
        for &slow in &[20_usize, 30, 50, 100] {
            if fast >= slow { continue; }
            let mut sig = SmaCross::new(fast, slow)?;
            let r = run(&bars, &mut sig, &cfg);
            let n = r.trades.len() as u32;
            let wins = r.wins();
            let win_rate = if n > 0 { wins as f64 / n as f64 * 100.0 } else { 0.0 };

            ws2.write_number(row2, 0, fast as f64).ok();
            ws2.write_number(row2, 1, slow as f64).ok();
            ws2.write_number(row2, 2, r.signals_fired as f64).ok();
            ws2.write_number(row2, 3, n as f64).ok();
            ws2.write_number(row2, 4, wins as f64).ok();
            ws2.write_number_with_format(row2, 5, win_rate, &num2_fmt).ok();
            ws2.write_number_with_format(row2, 6, r.total_pnl(), &num2_fmt).ok();
            ws2.write_number_with_format(row2, 7, r.final_equity, &num2_fmt).ok();
            ws2.write_number_with_format(row2, 8, r.max_drawdown_pct, &num2_fmt).ok();
            row2 += 1;
        }
    }
    println!("Sheet 2: Param Sweep — {} configs", row2 - 1);

    // ───────────────────────────────────────────────
    // SHEET 3: TRADES (default run)
    // ───────────────────────────────────────────────
    let ws3 = wb.add_worksheet();
    ws3.set_name("3. Trades").map_err(|e| e.to_string())?;
    let headers3 = ["block_entry", "block_exit", "direction", "price_entry",
        "price_exit", "qty", "pnl", "bracket_dist", "exit_type", "win"];
    for (c, h) in headers3.iter().enumerate() {
        ws3.write_string_with_format(0, c as u16, *h, &hdr_fmt).ok();
    }
    for c in 0..10 { ws3.set_column_width(c, 12).ok(); }

    for (i, t) in result_a.trades.iter().enumerate() {
        let r = i as u32 + 1;
        ws3.write_number(r, 0, t.block_entry as f64).ok();
        ws3.write_number(r, 1, t.block_exit as f64).ok();
        ws3.write_string(r, 2, &t.direction).ok();
        ws3.write_number_with_format(r, 3, t.price_entry, &num4_fmt).ok();
        ws3.write_number_with_format(r, 4, t.price_exit, &num4_fmt).ok();
        ws3.write_number_with_format(r, 5, t.qty, &num4_fmt).ok();
        ws3.write_number_with_format(r, 6, t.pnl, &num2_fmt).ok();
        ws3.write_number_with_format(r, 7, t.bracket_dist, &num4_fmt).ok();
        ws3.write_string(r, 8, &t.exit_type).ok();
        let wfmt = if t.win { &pass_fmt } else { &fail_fmt };
        ws3.write_string_with_format(r, 9, if t.win { "WIN" } else { "LOSS" }, wfmt).ok();
    }
    println!("Sheet 3: Trades — {} rows", result_a.trades.len());

    // ───────────────────────────────────────────────
    // SHEET 4: EQUITY CURVE
    // ───────────────────────────────────────────────
    let ws4 = wb.add_worksheet();
    ws4.set_name("4. Equity").map_err(|e| e.to_string())?;
    let headers4 = ["block", "timestamp", "close", "atr", "equity", "drawdown_pct", "active"];
    for (c, h) in headers4.iter().enumerate() {
        ws4.write_string_with_format(0, c as u16, *h, &hdr_fmt).ok();
    }
    ws4.set_column_width(1, 20).ok();
    for c in 2..7 { ws4.set_column_width(c, 12).ok(); }

    for (i, b) in result_a.bar_rows.iter().enumerate() {
        let r = i as u32 + 1;
        ws4.write_number(r, 0, b.block_index as f64).ok();
        ws4.write_string(r, 1, &b.timestamp).ok();
        ws4.write_number_with_format(r, 2, b.close, &num4_fmt).ok();
        if let Some(a) = b.atr {
            ws4.write_number_with_format(r, 3, a, &num4_fmt).ok();
        }
        ws4.write_number_with_format(r, 4, b.equity, &num2_fmt).ok();
        ws4.write_number_with_format(r, 5, b.drawdown_pct, &num2_fmt).ok();
        ws4.write_boolean(r, 6, b.active).ok();
    }
    println!("Sheet 4: Equity — {} rows", result_a.bar_rows.len());

    // ───────────────────────────────────────────────
    // SHEET 5: CONVERGENCE PROJECTIONS
    // ───────────────────────────────────────────────
    let ws5 = wb.add_worksheet();
    ws5.set_name("5. Convergence").map_err(|e| e.to_string())?;
    let headers5 = ["n_trades", "CI_width", "CI_lower", "CI_upper", "div_threshold"];
    for (c, h) in headers5.iter().enumerate() {
        ws5.write_string_with_format(0, c as u16, *h, &hdr_fmt).ok();
    }
    for c in 0..5 { ws5.set_column_width(c, 16).ok(); }

    let p_hat = cfg.p_assumed;
    let z = 1.96_f64;
    for (idx, &n) in [5, 10, 20, 30, 40, 50, 75, 100, 150, 200, 500].iter().enumerate() {
        let nf = n as f64;
        let denom = 1.0 + z * z / nf;
        let margin = z * (p_hat * (1.0 - p_hat) / nf + z * z / (4.0 * nf * nf)).sqrt() / denom;
        let ci_lo = (p_hat - margin).max(0.0);
        let ci_hi = (p_hat + margin).min(1.0);
        let se = (p_hat * (1.0 - p_hat) / nf).sqrt();

        let r = idx as u32 + 1;
        ws5.write_number(r, 0, nf).ok();
        ws5.write_number_with_format(r, 1, 2.0 * margin, &num4_fmt).ok();
        ws5.write_number_with_format(r, 2, ci_lo, &num4_fmt).ok();
        ws5.write_number_with_format(r, 3, ci_hi, &num4_fmt).ok();
        ws5.write_number_with_format(r, 4, 2.0 * se, &num4_fmt).ok();
    }
    println!("Sheet 5: Convergence — 11 milestones");

    // ───────────────────────────────────────────────
    // SHEET 6: NOTES
    // ───────────────────────────────────────────────
    let ws6 = wb.add_worksheet();
    ws6.set_name("6. Notes").map_err(|e| e.to_string())?;
    ws6.set_column_width(0, 100).ok();

    let mut ar: u32 = 0;
    macro_rules! a {
        ($text:expr, bold) => {{
            ws6.write_string_with_format(ar, 0, $text, &bold_fmt).ok(); ar += 1;
        }};
        ($text:expr, title) => {{
            ws6.write_string_with_format(ar, 0, $text, &title_fmt).ok(); ar += 1;
        }};
        ($text:expr) => {{
            ws6.write_string(ar, 0, $text).ok(); ar += 1;
        }};
    }

    a!("GSI-K FRAMEWORK — BATTERY NOTES", title);
    a!("");
    a!("WHAT THIS WORKBOOK IS", bold);
    a!("An auditable snapshot of one deterministic replay: self-checks, a parameter");
    a!("sweep, the trade ledger, the equity curve, and convergence projections.");
    a!("");
    a!("WHAT IT IS NOT", bold);
    a!("The signal driving these trades is an illustrative SMA cross included only so");
    a!("the framework runs end-to-end. Its results carry no information about any");
    a!("production strategy. The production signal model and its parameters are");
    a!("proprietary and are not part of this repository.");
    a!("");
    a!("DETERMINISM", bold);
    a!(format!("Session SHA (this data, this signal, this config): {}", chain_a.session_sha()));
    a!("Re-running the battery on the same inputs reproduces this SHA exactly.");

    println!("Sheet 6: Notes ({} lines)", ar);

    // ═══════════════════════════════════════════════
    // SAVE
    // ═══════════════════════════════════════════════
    wb.save(output_path.to_str().unwrap()).map_err(|e| e.to_string())?;

    println!();
    println!("=== BATTERY COMPLETE ===");
    println!("Output: {}", output_path.display());
    println!("Checks: {} PASS, {} FAIL", pass_count, fail_count);
    println!("Session SHA: {}", &chain_a.session_sha()[..16]);

    Ok(())
}
