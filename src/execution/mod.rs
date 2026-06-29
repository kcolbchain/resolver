//! Execution — build, sign, and submit fill transactions.
//!
//! In production, this submits to Flashbots Protect or a private mempool
//! to avoid frontrunning. For testing and dry runs, the executor evaluates
//! fillability against an injectable [`FillProvider`] so it never touches a
//! live RPC.

mod provider;

pub use provider::{FillProvider, MockProvider, OnChainState};

use alloy::primitives::U256;
use async_trait::async_trait;
use std::sync::Arc;

use crate::error::{ResolverError, Result};
use crate::intents::SolverQuote;

/// Fill result from executing a solver quote on-chain.
#[derive(Debug, Clone, PartialEq)]
pub struct FillResult {
    pub tx_hash: String,
    pub intent_id: String,
    pub success: bool,
    pub gas_used: u64,
    pub actual_profit_usd: f64,
}

/// A concrete, ordered set of actions the executor would take to fill an
/// intent. Produced by [`Executor::fill`] before (or instead of) broadcasting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillPlan {
    pub intent_id: String,
    /// Output token amount the executor must deliver to satisfy the intent.
    pub output_amount: U256,
    /// Gas the fill is budgeted to consume.
    pub gas_limit: u64,
    /// Gas price (wei) the executor will pay.
    pub gas_price_wei: U256,
    /// Ordered route/legs the fill executes.
    pub steps: Vec<FillStep>,
}

/// A single on-chain action in a [`FillPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillStep {
    pub kind: FillStepKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillStepKind {
    /// Approve the settlement contract to pull the executor's output token.
    Approve,
    /// Swap along a route leg to source the output token.
    Swap,
    /// Settle the intent on the protocol's settlement contract.
    Settle,
}

/// Why a simulated fill cannot proceed. Surfaced via [`ResolverError`] so the
/// solver can log and skip without aborting the cycle.
#[derive(Debug, Clone, PartialEq)]
pub enum NotFillable {
    Expired { deadline: u64, now: u64 },
    InsufficientInventory { have: U256, need: U256 },
    GasPriceTooHigh { gas_price_wei: U256, cap_wei: U256 },
    Unprofitable { net_profit_usd: f64 },
}

impl std::fmt::Display for NotFillable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotFillable::Expired { deadline, now } => {
                write!(f, "intent expired (deadline {deadline}, now {now})")
            }
            NotFillable::InsufficientInventory { have, need } => {
                write!(f, "insufficient inventory (have {have}, need {need})")
            }
            NotFillable::GasPriceTooHigh {
                gas_price_wei,
                cap_wei,
            } => write!(
                f,
                "gas price too high ({gas_price_wei} wei > cap {cap_wei} wei)"
            ),
            NotFillable::Unprofitable { net_profit_usd } => {
                write!(f, "unprofitable (net ${net_profit_usd:.4})")
            }
        }
    }
}

/// Executor trait — implemented per chain/submission strategy.
#[async_trait]
pub trait Executor: Send + Sync {
    /// Submit a fill transaction for a profitable quote.
    async fn fill(&self, quote: &SolverQuote) -> Result<FillResult>;

    /// Simulate a fill without submitting (dry run).
    async fn simulate(&self, quote: &SolverQuote) -> Result<FillResult>;
}

/// Limits the executor enforces when deciding whether a quote is fillable.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Reject quotes whose net profit is below this many USD.
    pub min_profit_usd: f64,
    /// Reject quotes whose gas price exceeds this many gwei.
    pub max_gas_gwei: u64,
    /// Gas the executor budgets for a fill when the quote carries no route.
    pub default_gas_limit: u64,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            min_profit_usd: 0.0,
            max_gas_gwei: 100,
            default_gas_limit: 200_000,
        }
    }
}

/// The default executor: deterministically evaluates fillability against an
/// injectable [`FillProvider`] and builds a [`FillPlan`]. Generic over the
/// provider so tests can supply a [`MockProvider`] with no network access.
pub struct LocalExecutor<P: FillProvider> {
    provider: Arc<P>,
    config: ExecutorConfig,
}

impl<P: FillProvider> LocalExecutor<P> {
    pub fn new(provider: P) -> Self {
        Self::with_config(provider, ExecutorConfig::default())
    }

    pub fn with_config(provider: P, config: ExecutorConfig) -> Self {
        Self {
            provider: Arc::new(provider),
            config,
        }
    }

    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }

    /// Evaluate, deterministically, whether `quote` can be filled given the
    /// current on-chain state from the provider. Returns the validated plan,
    /// or the specific [`NotFillable`] reason.
    pub async fn evaluate(&self, quote: &SolverQuote) -> Result<FillPlan> {
        let state = self.provider.state().await?;
        self.evaluate_against(quote, &state)
    }

    /// Pure decision function — no I/O. Splitting this out keeps the
    /// fillability logic fully deterministic and trivially unit-testable.
    fn evaluate_against(
        &self,
        quote: &SolverQuote,
        state: &OnChainState,
    ) -> Result<FillPlan> {
        // 1. Deadline check against the provider's clock (not wall-clock, so
        //    tests are deterministic).
        if let Some(deadline) = quote_deadline(quote) {
            if state.now > deadline {
                return Err(not_fillable(
                    quote,
                    NotFillable::Expired {
                        deadline,
                        now: state.now,
                    },
                ));
            }
        }

        // 2. Inventory: the executor must hold enough of the output token to
        //    deliver `amount_out` to the user.
        let need = quote.amount_out;
        let have = state.output_token_balance;
        if have < need {
            return Err(not_fillable(
                quote,
                NotFillable::InsufficientInventory { have, need },
            ));
        }

        // 3. Gas price cap.
        let cap_wei = U256::from(self.config.max_gas_gwei) * U256::from(1_000_000_000u64);
        if state.gas_price_wei > cap_wei {
            return Err(not_fillable(
                quote,
                NotFillable::GasPriceTooHigh {
                    gas_price_wei: state.gas_price_wei,
                    cap_wei,
                },
            ));
        }

        // 4. Profitability — the solver already computed this, but we re-gate
        //    here so the executor never fills a quote that lost its edge.
        if !quote.profitable || quote.net_profit_usd < self.config.min_profit_usd {
            return Err(not_fillable(
                quote,
                NotFillable::Unprofitable {
                    net_profit_usd: quote.net_profit_usd,
                },
            ));
        }

        Ok(self.build_plan(quote, state))
    }

    /// Build the ordered fill plan from a validated quote.
    fn build_plan(&self, quote: &SolverQuote, state: &OnChainState) -> FillPlan {
        let mut steps = Vec::new();

        steps.push(FillStep {
            kind: FillStepKind::Approve,
            description: "approve settlement to pull output token".into(),
        });

        // One swap leg per route step the solver found; the settlement leg
        // delivers the output token to the recipient.
        for step in &quote.route {
            steps.push(FillStep {
                kind: FillStepKind::Swap,
                description: format!(
                    "{}: {} -> {} ({} in)",
                    step.dex, step.token_in, step.token_out, step.amount_in
                ),
            });
        }

        steps.push(FillStep {
            kind: FillStepKind::Settle,
            description: format!("settle intent {}", quote.intent_id),
        });

        let gas_limit = self.config.default_gas_limit;

        FillPlan {
            intent_id: quote.intent_id.clone(),
            output_amount: quote.amount_out,
            gas_limit,
            gas_price_wei: state.gas_price_wei,
            steps,
        }
    }
}

#[async_trait]
impl<P: FillProvider> Executor for LocalExecutor<P> {
    async fn fill(&self, quote: &SolverQuote) -> Result<FillResult> {
        let plan = self.evaluate(quote).await?;
        let gas_used = plan.gas_limit;
        let tx_hash = self.provider.submit(&plan).await?;

        Ok(FillResult {
            tx_hash,
            intent_id: quote.intent_id.clone(),
            success: true,
            gas_used,
            actual_profit_usd: quote.net_profit_usd,
        })
    }

    async fn simulate(&self, quote: &SolverQuote) -> Result<FillResult> {
        // Dry run: validate fillability and report the would-be result without
        // submitting anything.
        let plan = self.evaluate(quote).await?;

        Ok(FillResult {
            tx_hash: String::new(), // no broadcast in simulation
            intent_id: quote.intent_id.clone(),
            success: true,
            gas_used: plan.gas_limit,
            actual_profit_usd: quote.net_profit_usd,
        })
    }
}

/// Intent deadline (unix seconds) carried on the quote, or `None` when the
/// solver did not record one (in which case the expiry check is skipped). The
/// provider's `now` is the source of truth for the current time.
fn quote_deadline(quote: &SolverQuote) -> Option<u64> {
    quote.deadline
}

fn not_fillable(quote: &SolverQuote, reason: NotFillable) -> ResolverError {
    ResolverError::Simulation(format!("intent {}: {}", quote.intent_id, reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intents::SolverQuote;

    /// A profitable quote that asks for 1_000 units of the output token and
    /// expires far in the future. The default [`MockProvider::fillable`] has
    /// enough inventory, cheap gas, and a clock before the deadline.
    fn profitable_quote() -> SolverQuote {
        SolverQuote {
            intent_id: "0xintent".into(),
            amount_out: U256::from(1_000u64),
            gas_cost_wei: U256::from(21_000u64) * U256::from(1_000_000_000u64),
            gas_cost_usd: 0.05,
            net_profit_wei: U256::from(500u64),
            net_profit_usd: 12.34,
            route: vec![],
            profitable: true,
            deadline: Some(1_800_000_000),
        }
    }

    #[tokio::test]
    async fn simulate_fillable_quote_succeeds_without_broadcasting() {
        let provider = MockProvider::fillable();
        let executor = LocalExecutor::new(provider);
        let quote = profitable_quote();

        let result = executor.simulate(&quote).await.expect("should be fillable");

        assert!(result.success);
        assert_eq!(result.intent_id, "0xintent");
        // Simulation reports the budgeted gas but does NOT broadcast.
        assert_eq!(result.gas_used, ExecutorConfig::default().default_gas_limit);
        assert_eq!(result.tx_hash, "", "simulate must not broadcast a tx");
        assert_eq!(result.actual_profit_usd, 12.34);
    }

    #[tokio::test]
    async fn simulate_reports_correct_plan_via_evaluate() {
        let route_quote = SolverQuote {
            route: vec![crate::intents::RouteStep {
                dex: "UniV3".into(),
                token_in: alloy::primitives::Address::ZERO,
                token_out: alloy::primitives::Address::ZERO,
                amount_in: U256::from(1_000u64),
                expected_out: U256::from(1_010u64),
                pool: alloy::primitives::Address::ZERO,
            }],
            ..profitable_quote()
        };
        let executor = LocalExecutor::new(MockProvider::fillable());

        let plan = executor.evaluate(&route_quote).await.expect("fillable");

        assert_eq!(plan.intent_id, "0xintent");
        assert_eq!(plan.output_amount, U256::from(1_000u64));
        // Approve + one Swap (from the route) + Settle.
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].kind, FillStepKind::Approve);
        assert_eq!(plan.steps[1].kind, FillStepKind::Swap);
        assert_eq!(plan.steps[2].kind, FillStepKind::Settle);
        assert_eq!(plan.gas_price_wei, U256::from(1_000_000_000u64));
    }

    #[tokio::test]
    async fn not_fillable_when_expired() {
        // Clock is set past the quote's deadline.
        let provider = MockProvider::fillable().with_now(1_900_000_000);
        let executor = LocalExecutor::new(provider);

        let err = executor
            .simulate(&profitable_quote())
            .await
            .expect_err("expired quote must not be fillable");

        let msg = err.to_string();
        assert!(msg.contains("expired"), "got: {msg}");
    }

    #[tokio::test]
    async fn not_fillable_when_inventory_insufficient() {
        // Executor holds fewer output tokens than the quote needs (1_000).
        let provider = MockProvider::fillable().with_output_balance(U256::from(999u64));
        let executor = LocalExecutor::new(provider);

        let err = executor
            .simulate(&profitable_quote())
            .await
            .expect_err("insufficient inventory must not be fillable");

        let msg = err.to_string();
        assert!(msg.contains("insufficient inventory"), "got: {msg}");
    }

    #[tokio::test]
    async fn not_fillable_when_gas_price_exceeds_cap() {
        // Default cap is 100 gwei; set the chain to 200 gwei.
        let provider = MockProvider::fillable()
            .with_gas_price_wei(U256::from(200u64) * U256::from(1_000_000_000u64));
        let executor = LocalExecutor::new(provider);

        let err = executor
            .simulate(&profitable_quote())
            .await
            .expect_err("gas over cap must not be fillable");

        let msg = err.to_string();
        assert!(msg.contains("gas price too high"), "got: {msg}");
    }

    #[tokio::test]
    async fn not_fillable_when_unprofitable() {
        let mut quote = profitable_quote();
        quote.profitable = false;
        quote.net_profit_usd = -1.0;
        let executor = LocalExecutor::new(MockProvider::fillable());

        let err = executor
            .simulate(&quote)
            .await
            .expect_err("unprofitable quote must not be fillable");

        let msg = err.to_string();
        assert!(msg.contains("unprofitable"), "got: {msg}");
    }

    #[tokio::test]
    async fn fill_broadcasts_and_returns_tx_hash() {
        let provider = std::sync::Arc::new(MockProvider::fillable());
        // Share the provider so we can inspect what was submitted.
        let executor = LocalExecutor {
            provider: provider.clone(),
            config: ExecutorConfig::default(),
        };
        let quote = profitable_quote();

        let result = executor.fill(&quote).await.expect("fill should succeed");

        assert!(result.success);
        assert_eq!(result.intent_id, "0xintent");
        assert!(
            result.tx_hash.starts_with("0x"),
            "fill must return a tx hash, got: {}",
            result.tx_hash
        );
        assert_eq!(result.gas_used, ExecutorConfig::default().default_gas_limit);

        // Exactly one plan should have been broadcast.
        assert_eq!(provider.submit_count(), 1);
        let plans = provider.submitted_plans();
        assert_eq!(plans[0].intent_id, "0xintent");
        assert_eq!(plans[0].output_amount, U256::from(1_000u64));
    }

    #[tokio::test]
    async fn simulate_does_not_broadcast() {
        let provider = std::sync::Arc::new(MockProvider::fillable());
        let executor = LocalExecutor {
            provider: provider.clone(),
            config: ExecutorConfig::default(),
        };

        executor.simulate(&profitable_quote()).await.unwrap();

        assert_eq!(
            provider.submit_count(),
            0,
            "simulate must never broadcast a plan"
        );
    }

    #[tokio::test]
    async fn fill_propagates_broadcast_error() {
        let provider = MockProvider::fillable().with_submit_error("mempool rejected");
        let executor = LocalExecutor::new(provider);

        let err = executor
            .fill(&profitable_quote())
            .await
            .expect_err("submit error must propagate");

        assert!(err.to_string().contains("mempool rejected"), "got: {err}");
    }
}
