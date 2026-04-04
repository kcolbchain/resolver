//! Solver engine — the brain. Takes intents, simulates routes, picks profitable ones.

use alloy::primitives::U256;
use dashmap::DashMap;
use std::sync::Arc;

use crate::intents::{Intent, IntentDecoder, SolverQuote};
use crate::error::{ResolverError, Result};

/// Configuration for the solver engine.
#[derive(Debug, Clone)]
pub struct SolverConfig {
    /// Minimum profit in USD to consider filling an intent.
    pub min_profit_usd: f64,
    /// Maximum gas price in gwei the solver is willing to pay.
    pub max_gas_gwei: u64,
    /// How many intents to evaluate in parallel.
    pub parallelism: usize,
    /// Whether to actually submit transactions or just simulate.
    pub simulate_only: bool,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            min_profit_usd: 0.50,
            max_gas_gwei: 50,
            parallelism: 10,
            simulate_only: true,
        }
    }
}

/// The solver engine — orchestrates intent discovery, evaluation, and filling.
pub struct SolverEngine {
    config: SolverConfig,
    decoders: Vec<Box<dyn IntentDecoder>>,
    /// Track intents we've already seen to avoid duplicate work.
    seen: Arc<DashMap<String, u64>>,
    /// Performance stats.
    stats: SolverStats,
}

#[derive(Debug, Default)]
pub struct SolverStats {
    pub intents_seen: u64,
    pub intents_evaluated: u64,
    pub intents_profitable: u64,
    pub intents_filled: u64,
    pub total_profit_usd: f64,
    pub total_gas_spent_usd: f64,
}

impl SolverEngine {
    pub fn new(config: SolverConfig) -> Self {
        Self {
            config,
            decoders: Vec::new(),
            seen: Arc::new(DashMap::new()),
            stats: SolverStats::default(),
        }
    }

    /// Register a protocol decoder (UniswapX, Across, etc.)
    pub fn add_decoder(&mut self, decoder: Box<dyn IntentDecoder>) {
        tracing::info!("Registered decoder: {}", decoder.protocol());
        self.decoders.push(decoder);
    }

    /// Run one cycle: fetch intents → evaluate → fill profitable ones.
    pub async fn cycle(&mut self) -> Result<Vec<SolverQuote>> {
        let mut all_intents = Vec::new();

        // Fetch from all registered protocols
        for decoder in &self.decoders {
            match decoder.fetch_open_intents().await {
                Ok(intents) => {
                    tracing::info!("{}: {} open intents", decoder.protocol(), intents.len());
                    all_intents.extend(intents);
                }
                Err(e) => {
                    tracing::warn!("{}: failed to fetch: {}", decoder.protocol(), e);
                }
            }
        }

        self.stats.intents_seen += all_intents.len() as u64;

        // Filter out already-seen and expired intents
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let new_intents: Vec<Intent> = all_intents
            .into_iter()
            .filter(|i| !i.is_expired(now))
            .filter(|i| !self.seen.contains_key(&i.id))
            .collect();

        // Mark as seen
        for intent in &new_intents {
            self.seen.insert(intent.id.clone(), now);
        }

        // Evaluate each intent for profitability
        let mut profitable = Vec::new();
        for intent in &new_intents {
            self.stats.intents_evaluated += 1;
            match self.evaluate(intent).await {
                Ok(quote) if quote.profitable => {
                    self.stats.intents_profitable += 1;
                    tracing::info!(
                        "💰 Profitable: {} | profit: ${:.2} | gas: ${:.2}",
                        intent.id, quote.net_profit_usd, quote.gas_cost_usd
                    );
                    profitable.push(quote);
                }
                Ok(_) => {} // not profitable, skip
                Err(e) => {
                    tracing::debug!("Failed to evaluate {}: {}", intent.id, e);
                }
            }
        }

        // Sort by profit (highest first)
        profitable.sort_by(|a, b| b.net_profit_usd.partial_cmp(&a.net_profit_usd).unwrap());

        Ok(profitable)
    }

    /// Evaluate a single intent for profitability.
    async fn evaluate(&self, intent: &Intent) -> Result<SolverQuote> {
        // Calculate surplus the solver can capture
        let surplus = intent.max_surplus();

        // Estimate gas cost (simplified — in production, simulate the actual tx)
        let estimated_gas = 200_000u64; // typical swap gas
        let gas_price_gwei = 0.1; // L2 gas price
        let eth_price_usd = 3000.0; // hardcoded for now
        let gas_cost_wei = U256::from(estimated_gas) * U256::from((gas_price_gwei * 1e9) as u64);
        let gas_cost_usd = (estimated_gas as f64 * gas_price_gwei * 1e-9) * eth_price_usd;

        // Estimate profit (simplified — surplus minus gas)
        // In production: simulate the full swap path and compute exact output
        let surplus_usd = if surplus > U256::ZERO {
            // Rough estimation: assume 6 decimal stablecoin output
            let surplus_f64 = surplus.to::<u128>() as f64 / 1e6;
            surplus_f64
        } else {
            0.0
        };

        let net_profit_usd = surplus_usd - gas_cost_usd;
        let profitable = net_profit_usd >= self.config.min_profit_usd;

        Ok(SolverQuote {
            intent_id: intent.id.clone(),
            amount_out: intent.current_amount_out,
            gas_cost_wei,
            gas_cost_usd,
            net_profit_wei: if surplus > gas_cost_wei { surplus - gas_cost_wei } else { U256::ZERO },
            net_profit_usd,
            route: vec![], // populated by route finder in production
            profitable,
        })
    }

    /// Get solver stats.
    pub fn stats(&self) -> &SolverStats {
        &self.stats
    }

    /// Number of unique intents tracked.
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }
}
