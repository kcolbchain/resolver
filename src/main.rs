//! resolver CLI — scan, solve, and monitor intent filling.

use resolver::intents::{self, IntentDecoder, UniswapXDecoder};
use resolver::solver::{SolverConfig, SolverEngine};
use resolver::monitor;

#[tokio::main]
async fn main() -> resolver::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("resolver=info".parse().unwrap())
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("scan");

    match command {
        "scan" => {
            let chain = intents::Chain::Base;
            let decoder = UniswapXDecoder::new(chain);
            let intents = decoder.fetch_open_intents().await?;

            println!("\n📋 Open UniswapX intents on {:?}: {}\n", chain, intents.len());
            for (i, intent) in intents.iter().take(10).enumerate() {
                let surplus = intent.max_surplus();
                println!(
                    "  {}. {} | in: {} | out_min: {} | surplus: {} | expires: {}s",
                    i + 1,
                    &intent.id[..16],
                    intent.amount_in,
                    intent.min_amount_out,
                    surplus,
                    intent.time_remaining(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                    ),
                );
            }
        }

        "solve" => {
            let config = SolverConfig {
                simulate_only: true,
                min_profit_usd: 0.50,
                ..Default::default()
            };
            let mut engine = SolverEngine::new(config);
            engine.add_decoder(Box::new(UniswapXDecoder::new(intents::Chain::Base)));

            println!("🔄 Running solver cycle...\n");
            let quotes = engine.cycle().await?;

            println!("\n💰 Profitable intents: {}\n", quotes.len());
            for q in &quotes {
                println!(
                    "  {} | profit: ${:.2} | gas: ${:.2}",
                    &q.intent_id[..16.min(q.intent_id.len())],
                    q.net_profit_usd,
                    q.gas_cost_usd,
                );
            }

            println!();
            monitor::print_stats(&engine);
        }

        "monitor" => {
            let engine = SolverEngine::new(SolverConfig::default());
            monitor::print_stats(&engine);
        }

        _ => {
            println!("Usage: resolver [scan|solve|monitor]");
            println!("  scan     — fetch and display open intents");
            println!("  solve    — run one solver cycle (simulation mode)");
            println!("  monitor  — display solver statistics");
        }
    }

    Ok(())
}
