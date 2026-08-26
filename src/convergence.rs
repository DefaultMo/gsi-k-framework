// convergence.rs — The measurement loop.
//
// A backtest asserts an assumed win rate (p) and payoff ratio (b).
// As real trades accumulate, p_measured and b_measured converge toward
// the truth; a divergence flag fires when measurement drifts outside
// 2 standard errors of the assumption. Wilson CI narrows with n:
// 50 trades -> roughly +-0.14, 100 -> +-0.10, 200 -> +-0.07.

#[derive(Debug, Clone)]
pub struct ConvergenceState {
    pub p_assumed: f64,
    pub b_assumed: f64,

    pub wins:               u32,
    pub losses:             u32,
    pub total_win_bracket:  f64,
    pub total_loss_bracket: f64,
}

impl ConvergenceState {
    pub fn new(p_assumed: f64, b_assumed: f64) -> Self {
        ConvergenceState {
            p_assumed,
            b_assumed,
            wins: 0,
            losses: 0,
            total_win_bracket: 0.0,
            total_loss_bracket: 0.0,
        }
    }

    pub fn record_trade(&mut self, win: bool, bracket_dist: f64) {
        if win {
            self.wins += 1;
            self.total_win_bracket += bracket_dist;
        } else {
            self.losses += 1;
            self.total_loss_bracket += bracket_dist;
        }
    }

    pub fn n(&self) -> u32 {
        self.wins + self.losses
    }

    pub fn p_measured(&self) -> Option<f64> {
        let n = self.n();
        if n < 5 { None } else { Some(self.wins as f64 / n as f64) }
    }

    pub fn b_measured(&self) -> Option<f64> {
        if self.losses == 0 || self.total_loss_bracket == 0.0 {
            None
        } else {
            Some(self.total_win_bracket / self.total_loss_bracket)
        }
    }

    /// Wilson score confidence interval for p.
    pub fn p_ci(&self) -> Option<(f64, f64)> {
        let n = self.n() as f64;
        if n < 5.0 { return None; }
        let p = self.p_measured()?;
        let z = 1.96_f64;
        let z2 = z * z;
        let denom = 1.0 + z2 / n;
        let centre = (p + z2 / (2.0 * n)) / denom;
        let margin = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
        Some(((centre - margin).max(0.0), (centre + margin).min(1.0)))
    }

    /// Divergence: p_measured is outside 2 standard errors of p_assumed.
    pub fn p_diverged(&self) -> bool {
        let n = self.n() as f64;
        if n < 10.0 { return false; }
        let p = match self.p_measured() {
            Some(p) => p,
            None => return false,
        };
        let se = (self.p_assumed * (1.0 - self.p_assumed) / n).sqrt();
        (p - self.p_assumed).abs() > 2.0 * se
    }

    /// EV per trade using measured values where available.
    #[allow(dead_code)] // utility for downstream signal integrations
    pub fn ev_per_trade(&self, bracket_dist: f64) -> f64 {
        let p = self.p_measured().unwrap_or(self.p_assumed);
        let q = 1.0 - p;
        let b = self.b_measured().unwrap_or(1.0);
        (p * b - q) * bracket_dist
    }

    pub fn report(&self) -> String {
        let p = self.p_measured();
        let b = self.b_measured();
        let (ci_lo, ci_hi) = self.p_ci().unwrap_or((0.0, 0.0));
        format!(
            "N={} p_a={:.2} p_m={} CI=[{:.3},{:.3}] b_a={:.1} b_m={} div={}",
            self.n(),
            self.p_assumed,
            p.map_or("---".to_string(), |v| format!("{:.3}", v)),
            ci_lo, ci_hi,
            self.b_assumed,
            b.map_or("---".to_string(), |v| format!("{:.2}", v)),
            self.p_diverged()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ci_narrows() {
        let mut c = ConvergenceState::new(0.5, 1.0);
        for i in 0..50 {
            c.record_trade(i % 2 == 0, 1.0);
        }
        let (lo50, hi50) = c.p_ci().unwrap();
        for i in 0..150 {
            c.record_trade(i % 2 == 0, 1.0);
        }
        let (lo200, hi200) = c.p_ci().unwrap();
        assert!(hi200 - lo200 < hi50 - lo50, "CI must narrow with n");
    }

    #[test]
    fn test_divergence_fires() {
        let mut c = ConvergenceState::new(0.9, 1.0); // assume 90% win rate
        for _ in 0..30 {
            c.record_trade(false, 1.0); // lose every trade
        }
        assert!(c.p_diverged());
    }
}
