//! Monitor — P&L tracking, fill rate, gas analytics, terminal dashboard.

use crate::solver::SolverEngine;

/// Print solver stats to terminal.
pub fn print_stats(engine: &SolverEngine) {
    let stats = engine.stats();
    println!("╔══════════════════════════════════════╗");
    println!("║         resolver — solver stats       ║");
    println!("╠══════════════════════════════════════╣");
    println!("║  Intents seen:       {:>14}  ║", stats.intents_seen);
    println!("║  Evaluated:          {:>14}  ║", stats.intents_evaluated);
    println!("║  Profitable:         {:>14}  ║", stats.intents_profitable);
    println!("║  Filled:             {:>14}  ║", stats.intents_filled);
    println!("║  Total profit:    ${:>13.2}  ║", stats.total_profit_usd);
    println!("║  Total gas:       ${:>13.2}  ║", stats.total_gas_spent_usd);
    println!("║  Net P&L:         ${:>13.2}  ║",
             stats.total_profit_usd - stats.total_gas_spent_usd);
    println!("║  Unique intents:     {:>14}  ║", engine.seen_count());
    println!("╚══════════════════════════════════════╝");
}
