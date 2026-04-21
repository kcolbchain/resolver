//! resolver CLI — scan, solve, and monitor intent filling.

use resolver::intents::{self, AcrossDecoder, IntentDecoder, UniswapXDecoder};
use resolver::monitor;
use resolver::solver::{SolverConfig, SolverEngine};

/// Parse a flag value like `--key value` from argv. Returns `None` if the
/// flag is absent or has no value; the default is applied by the caller.
fn flag<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn parse_chain(s: &str) -> intents::Chain {
    match s.to_ascii_lowercase().as_str() {
        "ethereum" | "eth" | "1" => intents::Chain::Ethereum,
        "arbitrum" | "arb" | "42161" => intents::Chain::Arbitrum,
        "optimism" | "op" | "10" => intents::Chain::Optimism,
        "base" | "8453" => intents::Chain::Base,
        "polygon" | "matic" | "137" => intents::Chain::Polygon,
        other => {
            eprintln!("unknown chain '{other}', falling back to base");
            intents::Chain::Base
        }
    }
}

fn build_decoder(protocol: &str, chain: intents::Chain) -> Box<dyn IntentDecoder> {
    match protocol.to_ascii_lowercase().as_str() {
        "across" => Box::new(AcrossDecoder::new(chain)),
        _ => Box::new(UniswapXDecoder::new(chain)),
    }
}

#[tokio::main]
async fn main() -> resolver::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("resolver=info".parse().unwrap()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("scan");

    let chain = flag(&args, "--chain")
        .map(parse_chain)
        .unwrap_or(intents::Chain::Base);
    let protocol = flag(&args, "--protocol").unwrap_or("uniswapx");

    match command {
        "scan" => {
            let decoder = build_decoder(protocol, chain);
            let open = decoder.fetch_open_intents().await?;

            println!(
                "\nOpen {} intents on {:?}: {}\n",
                decoder.protocol(),
                chain,
                open.len()
            );
            for (i, intent) in open.iter().take(10).enumerate() {
                let surplus = intent.max_surplus();
                println!(
                    "  {}. {} | in: {} | out_min: {} | surplus: {} | expires: {}s | dest: {:?}",
                    i + 1,
                    &intent.id[..16.min(intent.id.len())],
                    intent.amount_in,
                    intent.min_amount_out,
                    surplus,
                    intent.time_remaining(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                    ),
                    intent.dest_chain,
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
            engine.add_decoder(build_decoder(protocol, chain));

            println!("Running solver cycle...\n");
            let quotes = engine.cycle().await?;

            println!("\nProfitable intents: {}\n", quotes.len());
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
            println!("Usage: resolver <command> [--chain <name>] [--protocol <name>]");
            println!("  scan     — fetch and display open intents");
            println!("  solve    — run one solver cycle (simulation mode)");
            println!("  monitor  — display solver statistics");
            println!();
            println!("Flags:");
            println!("  --chain      ethereum | arbitrum | optimism | base | polygon");
            println!("  --protocol   uniswapx | across");
        }
    }

    Ok(())
}
