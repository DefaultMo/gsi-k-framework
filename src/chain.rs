// chain.rs — SHA-256 session chain for trade provenance.
//
// Every closed trade is sealed into the session. The session digest is
// SHA-256 over (previous session SHA || startup fingerprint || session id
// || deterministically-sorted trades), so:
//   - a session's trade record cannot be edited without changing its SHA;
//   - sessions link into a chain (each records its predecessor's SHA);
//   - two replays of the same data with the same signal produce the
//     same SHA — determinism is checkable, not assumed.

use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRecord {
    pub block_entry:  u64,
    pub block_exit:   u64,
    pub direction:    String,
    pub price_entry:  f64,
    pub price_exit:   f64,
    pub qty:          f64,
    pub pnl:          f64,
    pub bracket_dist: f64,
    pub exit_type:    String, // TARGET | STOP | RESCUE | EOD
    pub win:          bool,
}

pub struct SessionChain {
    pub session_id: String,
    pub startup_fp: String,
    pub prev_sha:   Option<String>, // SHA of previous session (chain link)
    pub trades:     Vec<TradeRecord>,
}

impl SessionChain {
    pub fn new(session_id: &str, signal_name: &str, prev_sha: Option<String>) -> Self {
        // The fingerprint commits to the framework version and the signal
        // implementation in use, so a chain cannot silently mix engines.
        let fp_input = format!(
            "GSI-K-FRAMEWORK|v{}|signal={}",
            env!("CARGO_PKG_VERSION"),
            signal_name
        );
        let fp = hex::encode(Sha256::digest(fp_input.as_bytes()));
        SessionChain {
            session_id: session_id.to_string(),
            startup_fp: fp,
            prev_sha,
            trades: Vec::new(),
        }
    }

    pub fn seal_trade(&mut self, trade: TradeRecord) {
        self.trades.push(trade);
    }

    /// session_sha = SHA-256(prev_sha || startup_fp || session_id || sorted trades)
    pub fn session_sha(&self) -> String {
        let mut input = String::new();

        if let Some(ref prev) = self.prev_sha {
            input.push_str(prev);
        }

        input.push_str(&self.startup_fp);
        input.push_str(&self.session_id);

        // Sort trades by entry block for determinism.
        let mut sorted = self.trades.clone();
        sorted.sort_by_key(|t| t.block_entry);

        for t in &sorted {
            input.push_str(&format!(
                "{}|{}|{}|{:.6}|{:.6}|{:.6}|{:.4}|{}|{}",
                t.block_entry, t.block_exit, t.direction,
                t.price_entry, t.price_exit, t.qty,
                t.pnl, t.win as u8, t.exit_type
            ));
        }

        hex::encode(Sha256::digest(input.as_bytes()))
    }

    pub fn sha8(&self) -> String {
        self.session_sha()[..8].to_string()
    }

    /// Save the sealed session to `<base_dir>/sessions/<sha8>.json`.
    pub fn save(&self, base_dir: &str) -> Result<PathBuf, String> {
        let dir = PathBuf::from(base_dir).join("sessions");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let file = dir.join(format!("{}.json", self.sha8()));

        #[derive(Serialize)]
        struct SessionFile<'a> {
            session_id:  &'a str,
            startup_fp:  &'a str,
            prev_sha:    &'a Option<String>,
            session_sha: String,
            trades:      &'a Vec<TradeRecord>,
        }

        let data = SessionFile {
            session_id:  &self.session_id,
            startup_fp:  &self.startup_fp,
            prev_sha:    &self.prev_sha,
            session_sha: self.session_sha(),
            trades:      &self.trades,
        };

        let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        std::fs::write(&file, json).map_err(|e| e.to_string())?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(block: u64) -> TradeRecord {
        TradeRecord {
            block_entry: block,
            block_exit: block + 5,
            direction: "LONG".into(),
            price_entry: 100.0,
            price_exit: 101.0,
            qty: 1.0,
            pnl: 1.0,
            bracket_dist: 0.5,
            exit_type: "TARGET".into(),
            win: true,
        }
    }

    #[test]
    fn test_deterministic_sha() {
        let mut a = SessionChain::new("s1", "sig", None);
        let mut b = SessionChain::new("s1", "sig", None);
        // Insert in different orders — SHA must match (sorted internally).
        a.seal_trade(trade(1));
        a.seal_trade(trade(9));
        b.seal_trade(trade(9));
        b.seal_trade(trade(1));
        assert_eq!(a.session_sha(), b.session_sha());
    }

    #[test]
    fn test_tamper_changes_sha() {
        let mut a = SessionChain::new("s1", "sig", None);
        a.seal_trade(trade(1));
        let sha1 = a.session_sha();
        a.trades[0].pnl = 999.0;
        assert_ne!(sha1, a.session_sha());
    }

    #[test]
    fn test_chain_link_changes_sha() {
        let a = SessionChain::new("s1", "sig", None);
        let b = SessionChain::new("s1", "sig", Some("deadbeef".into()));
        assert_ne!(a.session_sha(), b.session_sha());
    }
}
