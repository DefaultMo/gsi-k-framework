// report.rs — Per-bar CSV reporting plus console status lines.

use std::path::Path;

pub struct ReportRow {
    pub block_index:     usize,
    pub timestamp:       String,
    pub close:           f64,
    pub atr:             Option<f64>,
    pub direction:       String, // signal emitted this bar
    pub active:          bool,   // position open after this bar
    pub position_qty:    f64,
    pub equity:          f64,
    pub drawdown_pct:    f64,
    pub p_measured:      Option<f64>,
    pub b_measured:      Option<f64>,
    pub n_trades:        u32,
    pub divergence_flag: bool,
}

pub struct CsvReport {
    writer: csv::Writer<std::fs::File>,
}

impl CsvReport {
    pub fn new(path: &Path) -> Result<Self, String> {
        let writer = csv::Writer::from_path(path).map_err(|e| e.to_string())?;
        let mut report = CsvReport { writer };
        report.writer.write_record([
            "block_index", "timestamp", "close", "atr", "direction", "active",
            "position_qty", "equity", "drawdown_pct",
            "p_measured", "b_measured", "n_trades", "divergence_flag",
        ]).map_err(|e| e.to_string())?;
        Ok(report)
    }

    pub fn write_row(&mut self, r: &ReportRow) -> Result<(), String> {
        self.writer.write_record(&[
            r.block_index.to_string(),
            r.timestamp.clone(),
            format!("{:.4}", r.close),
            r.atr.map_or("---".to_string(), |v| format!("{:.4}", v)),
            r.direction.clone(),
            r.active.to_string(),
            format!("{:.6}", r.position_qty),
            format!("{:.2}", r.equity),
            format!("{:.2}", r.drawdown_pct),
            r.p_measured.map_or("---".to_string(), |v| format!("{:.3}", v)),
            r.b_measured.map_or("---".to_string(), |v| format!("{:.2}", v)),
            r.n_trades.to_string(),
            r.divergence_flag.to_string(),
        ]).map_err(|e| e.to_string())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|e| e.to_string())
    }
}

/// Print a single-line block status to stdout, including the
/// convergence snapshot as of this bar.
pub fn print_block_status(r: &ReportRow) {
    println!(
        "Block {:5} | close={:9.4} atr={} | sig={:5} active={:5} qty={:9.4} | eq={:10.2} dd={:5.2}% | N={:3} p_m={} b_m={} div={}",
        r.block_index,
        r.close,
        r.atr.map_or("   ---  ".to_string(), |v| format!("{:8.4}", v)),
        r.direction,
        r.active,
        r.position_qty,
        r.equity,
        r.drawdown_pct,
        r.n_trades,
        r.p_measured.map_or("  ---".to_string(), |v| format!("{:.3}", v)),
        r.b_measured.map_or(" ---".to_string(), |v| format!("{:.2}", v)),
        r.divergence_flag
    );
}
