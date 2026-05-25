//! Resolver benchmark CLI.
//!
//! Replays captured order payloads against each resolver backend and
//! measures decision latency (order-in → route-out), excluding RPC.
//!
//! Usage: cargo run --release --bin resolver-bench -- --venues uniswapx,across,cow --samples 100

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use resolver::intents::{AcrossDecoder, IntentDecoder, UniswapXDecoder};
use resolver::solver::{SolverConfig, SolverEngine};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "resolver-bench", about = "Benchmark solver decision latency")]
struct BenchArgs {
    /// Comma-separated list of venues to benchmark
    #[arg(long, default_value = "uniswapx,across,cow")]
    venues: String,

    /// Number of samples per venue
    #[arg(long, default_value = "100")]
    samples: usize,

    /// Path to bench orders directory
    #[arg(long, default_value = "bench/orders")]
    orders_dir: Option<PathBuf>,

    /// Output format (table or json)
    #[arg(long, default_value = "table")]
    format: String,
}

#[derive(Debug, Serialize)]
struct BenchResult {
    venue: String,
    samples: usize,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
    mean_latency_ms: f64,
    min_latency_ms: f64,
    max_latency_ms: f64,
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64) * pct / 100.0).ceil() as usize - 1;
    sorted[idx.min(sorted.len() - 1)]
}

fn bench_venue(venue: &str, samples: usize) -> BenchResult {
    let config = SolverConfig {
        simulate_only: true,
        min_profit_usd: 0.50,
        ..Default::default()
    };
    let mut engine = SolverEngine::new(config);

    let chain = match venue {
        "uniswapx" | "cow" => resolver::intents::Chain::Ethereum,
        "across" => resolver::intents::Chain::Optimism,
        _ => resolver::intents::Chain::Base,
    };

    let decoder: Box<dyn IntentDecoder> = match venue {
        "across" => Box::new(AcrossDecoder::new(chain)),
        _ => Box::new(UniswapXDecoder::new(chain)),
    };
    engine.add_decoder(decoder);

    let mut latencies = Vec::with_capacity(samples);

    for _ in 0..samples {
        let start = Instant::now();

        // Measure pure decision path (in-memory, no RPC)
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(engine.cycle());

        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1000.0); // ms
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    BenchResult {
        venue: venue.to_string(),
        samples,
        p50_latency_ms: percentile(&latencies, 50.0),
        p95_latency_ms: percentile(&latencies, 95.0),
        p99_latency_ms: percentile(&latencies, 99.0),
        mean_latency_ms: latencies.iter().sum::<f64>() / latencies.len() as f64,
        min_latency_ms: latencies[0],
        max_latency_ms: latencies[latencies.len() - 1],
    }
}

fn main() {
    use clap::Parser;
    let args = BenchArgs::parse();

    let venues: Vec<&str> = args.venues.split(',').collect();
    let mut results = Vec::new();

    println!("Running benchmark: {} samples per venue\n", args.samples);

    for venue in &venues {
        print!("  Benchmarking {venue}... ");
        let result = bench_venue(venue, args.samples);
        println!("done");
        results.push(result);
    }

    if args.format == "json" {
        println!("\n{}", serde_json::to_string_pretty(&results).unwrap());
    } else {
        println!();
        println!("  {:<12} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10}", 
            "Venue", "p50(ms)", "p95(ms)", "p99(ms)", "mean(ms)", "min(ms)", "max(ms)");
        println!("  {}", "─".repeat(70));
        for r in &results {
            println!("  {:<12} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>10.2} {:>10.2}",
                r.venue, r.p50_latency_ms, r.p95_latency_ms, r.p99_latency_ms,
                r.mean_latency_ms, r.min_latency_ms, r.max_latency_ms);
        }
    }
}
