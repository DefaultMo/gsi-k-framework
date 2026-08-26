// GSI-K Framework — signal/execution-separated backtesting scaffold.
//
// The signal side (signal.rs) decides direction. The execution side
// (backtest.rs) turns directions into bracketed, risk-sized positions.
// Everything in between — replay, ATR sizing, CSV/Excel reporting,
// SHA-256 session provenance — is the framework this repository ships.
//
// The bundled signal is an illustrative SMA cross. The production
// signal is proprietary and intentionally absent.

mod signal;
mod sizing;
mod chain;
mod convergence;
mod collector;
mod report;
mod backtest;
mod battery;
mod synth;

use clap::Parser;
use std::path::PathBuf;

use backtest::BacktestConfig;
use chain::SessionChain;
use collector::load_bars;
use report::{CsvReport, print_block_status};
use signal::{SignalProvider, SmaCross};

#[derive(Parser)]
#[command(
    name = "gsi-k-framework",
    about = "Signal/execution-separated backtesting framework with deterministic replay"
)]
struct Cli {
    /// Path to a section-delimited session report to replay
    #[arg(long)]
    replay: Option<PathBuf>,

    /// Fast SMA period for the illustrative signal
    #[arg(long, default_value = "10")]
    fast: usize,

    /// Slow SMA period for the illustrative signal
    #[arg(long, default_value = "30")]
    slow: usize,

    /// Output CSV path for the per-bar report
    #[arg(long, default_value = "session_report.csv")]
    output: String,

    /// Run the signal parameter sweep instead of a single replay
    #[arg(long)]
    sweep: bool,

    /// Run the full battery and generate the multi-sheet Excel report
    #[arg(long)]
    battery: bool,

    /// Excel output path for the battery
    #[arg(long, default_value = "battery_report.xlsx")]
    excel: String,

    /// Previous session SHA for chain linking
    #[arg(long)]
    prev_sha: Option<String>,

    /// Session id (default: timestamped). Pin this to make the session
    /// SHA fully reproducible across runs.
    #[arg(long)]
    session_id: Option<String>,

    /// Generate a deterministic synthetic data file at this path, then exit
    #[arg(long)]
    gen_data: Option<PathBuf>,

    /// Seed for synthetic data generation
    #[arg(long, default_value = "42")]
    seed: u64,

    /// Number of bars for synthetic data generation
    #[arg(long, default_value = "1000")]
    bars: usize,
}

fn main() {
    let cli = Cli::parse();

    println!("GSI-K FRAMEWORK | deterministic replay harness");
    println!();

    if let Some(ref path) = cli.gen_data {
        match synth::write_sample(path, cli.seed, cli.bars) {
            Ok(()) => {
                println!("Wrote {} synthetic bars (seed={}) to {}", cli.bars, cli.seed, path.display());
                return;
            }
            Err(e) => {
                eprintln!("gen-data failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    let replay_path = match cli.replay {
        Some(ref p) => p.clone(),
        None => {
            eprintln!("No mode selected. Use --replay <path> [--sweep|--battery], or --gen-data <path>.");
            std::process::exit(1);
        }
    };

    if cli.battery {
        let excel_path = PathBuf::from(&cli.excel);
        if let Err(e) = battery::run_battery(&replay_path, &excel_path) {
            eprintln!("Battery failed: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if cli.sweep {
        run_sweep(&replay_path);
    } else {
        run_replay(&replay_path, cli.fast, cli.slow, &cli.output, cli.prev_sha, cli.session_id);
    }
}

fn run_replay(
    path: &PathBuf,
    fast: usize,
    slow: usize,
    output: &str,
    prev_sha: Option<String>,
    session_id: Option<String>,
) {
    println!("=== REPLAY MODE === signal=sma_cross fast={} slow={}", fast, slow);

    let bars = match load_bars(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to load: {}", e);
            std::process::exit(1);
        }
    };
    println!("Loaded {} bars", bars.len());
    println!();

    let mut sig = match SmaCross::new(fast, slow) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Bad signal params: {}", e);
            std::process::exit(1);
        }
    };
    let cfg = BacktestConfig::default();

    let result = backtest::run(&bars, &mut sig, &cfg);

    let output_path = PathBuf::from(output);
    let mut csv_report = match CsvReport::new(&output_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to create report: {}", e);
            std::process::exit(1);
        }
    };

    for row in &result.bar_rows {
        if row.block_index % 100 == 0 || row.direction != "NONE" {
            print_block_status(row);
        }
        csv_report.write_row(row).ok();
    }
    csv_report.flush().ok();

    // Seal the session: every trade into the SHA-256 chain.
    let session_id = session_id.unwrap_or_else(|| {
        format!("replay_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S"))
    });
    let mut chain_state = SessionChain::new(&session_id, &sig.name(), prev_sha);
    for t in &result.trades {
        chain_state.seal_trade(t.clone());
    }

    println!();
    println!("=== REPLAY SUMMARY ===");
    println!("Bars processed:    {}", bars.len());
    println!("Signal:            {}", sig.name());
    println!("Signals fired:     {} (LONG: {}, SHORT: {})",
        result.signals_fired, result.long_signals, result.short_signals);
    println!("Trades closed:     {} ({} wins, {} losses)",
        result.trades.len(), result.wins(), result.losses());
    println!("Total PnL:         {:.2}", result.total_pnl());
    println!("Final equity:      {:.2} (start {:.2})", result.final_equity, cfg.initial_equity);
    println!("Max drawdown:      {:.2}%", result.max_drawdown_pct);
    println!("Convergence:       {}", result.convergence.report());
    println!("Session SHA:       {}", &chain_state.session_sha()[..16]);
    println!("Output:            {}", output);

    let base = path.parent().map(|p| p.to_str().unwrap_or(".")).unwrap_or(".");
    match chain_state.save(base) {
        Ok(p) => println!("Session saved:     {}", p.display()),
        Err(e) => eprintln!("Save failed: {}", e),
    }
}

fn run_sweep(path: &PathBuf) {
    println!("=== PARAMETER SWEEP === (illustrative SMA grid)");
    println!();

    let bars = match load_bars(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to load: {}", e);
            std::process::exit(1);
        }
    };
    println!("Loaded {} bars", bars.len());
    println!();

    let cfg = BacktestConfig::default();

    println!(
        "{:>5} {:>5} | {:>7} {:>7} {:>5} {:>8} | {:>10} {:>12} {:>8}",
        "fast", "slow", "signals", "trades", "wins", "win_rate",
        "total_pnl", "final_equity", "max_dd%"
    );
    println!("{}", "-".repeat(85));

    for &fast in &[5_usize, 10, 15, 20] {
        for &slow in &[20_usize, 30, 50, 100] {
            if fast >= slow {
                continue;
            }
            let mut sig = match SmaCross::new(fast, slow) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let r = backtest::run(&bars, &mut sig, &cfg);
            let n = r.trades.len();
            let wins = r.wins();
            let win_rate = if n > 0 { wins as f64 / n as f64 * 100.0 } else { 0.0 };

            println!(
                "{:>5} {:>5} | {:>7} {:>7} {:>5} {:>7.1}% | {:>10.2} {:>12.2} {:>8.2}",
                fast, slow, r.signals_fired, n, wins, win_rate,
                r.total_pnl(), r.final_equity, r.max_drawdown_pct
            );
        }
    }

    println!();
    println!("Sweep of the ILLUSTRATIVE signal only — this is harness plumbing,");
    println!("not strategy research. Plug in your own SignalProvider.");
}
