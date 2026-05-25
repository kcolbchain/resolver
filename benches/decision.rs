//! Criterion benchmark for the pure decision path of each resolver backend.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use resolver::intents::{AcrossDecoder, IntentDecoder, UniswapXDecoder};
use resolver::solver::{SolverConfig, SolverEngine};

fn bench_uniswapx_decision(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("decision: uniswapx", |b| {
        b.iter(|| {
            let config = SolverConfig {
                simulate_only: true,
                min_profit_usd: 0.50,
                ..Default::default()
            };
            let mut engine = SolverEngine::new(black_box(config));
            engine.add_decoder(Box::new(UniswapXDecoder::new(
                resolver::intents::Chain::Ethereum,
            )));
            let _ = rt.block_on(engine.cycle());
        });
    });
}

fn bench_across_decision(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("decision: across", |b| {
        b.iter(|| {
            let config = SolverConfig {
                simulate_only: true,
                min_profit_usd: 0.50,
                ..Default::default()
            };
            let mut engine = SolverEngine::new(black_box(config));
            engine.add_decoder(Box::new(AcrossDecoder::new(
                resolver::intents::Chain::Optimism,
            )));
            let _ = rt.block_on(engine.cycle());
        });
    });
}

criterion_group!(benches, bench_uniswapx_decision, bench_across_decision);
criterion_main!(benches);
