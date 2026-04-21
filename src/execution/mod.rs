//! Execution — build, sign, and submit fill transactions.
//!
//! In production, this submits to Flashbots Protect or a private mempool
//! to avoid frontrunning.

use crate::error::Result;
use crate::intents::SolverQuote;

/// Fill result from executing a solver quote on-chain.
#[derive(Debug)]
pub struct FillResult {
    pub tx_hash: String,
    pub intent_id: String,
    pub success: bool,
    pub gas_used: u64,
    pub actual_profit_usd: f64,
}

/// Executor trait — implemented per chain/submission strategy.
#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    /// Submit a fill transaction for a profitable quote.
    async fn fill(&self, quote: &SolverQuote) -> Result<FillResult>;

    /// Simulate a fill without submitting (dry run).
    async fn simulate(&self, quote: &SolverQuote) -> Result<FillResult>;
}
