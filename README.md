# GSI-K Framework

The engineering skeleton of a live trading system, published as a standalone
framework: a signal/execution-separated backtesting harness with deterministic
replay, ATR-based bracket sizing, SHA-256 session provenance, and an auditable
multi-sheet Excel test battery — all in Rust.

**Why it's interesting:** every replay is a sealed, reproducible artifact — the
same data, signal, and config always produce the same trades and the same
SHA-256 session digest, so a backtest result can be verified instead of trusted.

## Architecture

The core design decision is a hard split between *deciding* and *doing*:

```
            signal side                          execution side
      ┌─────────────────────┐             ┌──────────────────────────┐
      │  SignalProvider     │  Direction  │  backtest.rs             │
      │  (trait, signal.rs) ├────────────>│  ATR brackets (sizing.rs)│
      │                     │             │  risk-based position size│
      └─────────────────────┘             │  conservative fill model │
                                          └───────────┬──────────────┘
                                                      │ TradeRecord
                                                      v
                       ┌──────────────┐   ┌──────────────────────────┐
                       │ convergence  │<──┤  SHA-256 session chain   │
                       │ p/b vs truth │   │  (chain.rs)              │
                       └──────────────┘   └──────────────────────────┘
```

| Module           | Role                                                                  |
| ---------------- | --------------------------------------------------------------------- |
| `signal.rs`      | `SignalProvider` trait — the only door between signal and execution   |
| `backtest.rs`    | Deterministic replay: bars in, bracketed risk-sized trades out        |
| `sizing.rs`      | Wilder ATR, bracket construction, risk-fraction position sizing       |
| `chain.rs`       | SHA-256 session chain: tamper-evident, linkable trade provenance      |
| `convergence.rs` | Measured win rate / payoff vs. assumptions, Wilson CI, divergence flag|
| `collector.rs`   | Section-delimited report parser (multi-section files, not pure CSV)   |
| `report.rs`      | Per-bar CSV reporting                                                 |
| `battery.rs`     | One-command multi-sheet Excel audit workbook                          |
| `synth.rs`       | Seeded synthetic OHLCV generator (no real market data ships here)     |

## Quick start

Requires a Rust toolchain (stable, 2021 edition).

```sh
cargo build
cargo test

# Replay the bundled synthetic session (1000 hourly bars, seed 42).
# Pinning --session-id makes the session SHA reproducible run-to-run.
cargo run -- --replay data/sample_session.csv --session-id demo

# Regenerate that exact file from its seed (byte-identical)
cargo run -- --gen-data data/sample_session.csv --seed 42 --bars 1000

# Sweep the illustrative signal's parameter grid
cargo run -- --replay data/sample_session.csv --sweep

# Full battery: 6-sheet Excel audit workbook
cargo run -- --replay data/sample_session.csv --battery
```

## Demo (actual output)

Replay of the bundled synthetic data through the illustrative SMA-cross signal:

```
=== REPLAY MODE === signal=sma_cross fast=10 slow=30
Loaded 1000 bars

Block     0 | close= 100.1851 atr=   ---   | sig=NONE  active=false qty=   0.0000 | eq=  10000.00 dd= 0.00% | N=  0 p_m=  --- b_m= --- div=false
Block    31 | close= 102.1508 atr=  1.1399 | sig=SHORT active=true  qty=  43.8640 | eq=  10000.00 dd= 0.00% | N=  0 p_m=  --- b_m= --- div=false
...
Block   997 | close=  89.6247 atr=  0.9616 | sig=LONG  active=true  qty=  52.7273 | eq=  10140.89 dd= 0.56% | N= 39 p_m=0.359 b_m=0.42 div=false

=== REPLAY SUMMARY ===
Bars processed:    1000
Signal:            sma_cross_10_30 (illustrative)
Signals fired:     40 (LONG: 20, SHORT: 20)
Trades closed:     40 (15 wins, 25 losses)
Total PnL:         218.32
Final equity:      10218.32 (start 10000.00)
Max drawdown:      4.90%
Convergence:       N=40 p_a=0.50 p_m=0.375 CI=[0.242,0.530] b_a=2.0 b_m=0.45 div=false
Session SHA:       3c79085c1c74fffb
Output:            session_report.csv
Session saved:     data\sessions\3c79085c.json
```

Run it again with the same `--session-id` and you get the same
`3c79085c1c74fffb` — that is the point.

The parameter sweep over the same data:

```
 fast  slow | signals  trades  wins win_rate |  total_pnl final_equity  max_dd%
-------------------------------------------------------------------------------------
    5    20 |      53      53    18    34.0% |      36.74     10036.74     4.96
    5    30 |      42      42    18    42.9% |     605.68     10605.68     3.45
   10    30 |      40      40    15    37.5% |     218.32     10218.32     4.90
   20   100 |      12      12     6    50.0% |     300.70     10300.70     1.49
```

And the battery produces a six-sheet workbook (self-checks with PASS/FAIL
formatting, parameter sweep, trade ledger, equity curve, convergence
projections, notes):

```
Sheet 1: Checks — 9 PASS, 0 FAIL
Sheet 2: Param Sweep — 15 configs
Sheet 3: Trades — 40 rows
Sheet 4: Equity — 1000 rows
Sheet 5: Convergence — 11 milestones
Sheet 6: Notes (15 lines)
```

## What this is NOT

This repository is the **framework only**, published deliberately without its
edge:

- The **production signal model is not here.** The bundled SMA cross is a
  textbook illustration so the harness runs end-to-end; it carries zero
  information about the proprietary signal, its features, or its parameters.
- **No tuned parameters** — every constant in this repo is a conventional
  placeholder (ATR 14, 2:1 target/stop, 0.5% risk), not a calibrated value.
- **No exchange credentials, endpoints, or live-trading code.** There is no
  order-routing here at all.
- **No real market data** — the bundled data is a seeded synthetic random walk.

## Limitations (honest)

- The illustrative signal is roughly a coin flip by design; the demo PnL is a
  property of the synthetic data, not evidence of anything.
- Fill model is simplistic: entries at bar close, conservative intrabar
  ordering (stop assumed hit before target), no fees, slippage, or funding.
- One position at a time; signals while in a position are ignored.
- The session chain proves *integrity* (this exact trade set, unmodified), not
  *quality*.
- This is a backtesting scaffold, not investment software, and nothing here is
  financial advice.

## License

MIT — see [LICENSE](LICENSE).

Part of the [vbounds.com](https://vbounds.com) project line.
