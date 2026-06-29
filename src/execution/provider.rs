//! Provider abstraction for the executor.
//!
//! [`FillProvider`] is the seam between the executor's deterministic decision
//! logic and the outside world. Production code backs it with a real RPC
//! client; tests back it with [`MockProvider`] so simulation and fill paths
//! run with zero network access and fully reproducible inputs.

use alloy::primitives::U256;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::FillPlan;
use crate::error::Result;

/// A snapshot of the on-chain state the executor needs to decide fillability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnChainState {
    /// Current block timestamp (unix seconds) — the executor's clock.
    pub now: u64,
    /// Current gas price in wei.
    pub gas_price_wei: U256,
    /// Executor's balance of the intent's output token (its sellable
    /// inventory used to satisfy the fill).
    pub output_token_balance: U256,
}

/// The chain-facing operations the executor depends on. Implementors must be
/// `Send + Sync` so the executor can be shared across the solver's workers.
#[async_trait]
pub trait FillProvider: Send + Sync + 'static {
    /// Fetch the current on-chain state relevant to a fill.
    async fn state(&self) -> Result<OnChainState>;

    /// Broadcast a fill plan and return the resulting transaction hash.
    async fn submit(&self, plan: &FillPlan) -> Result<String>;
}

/// Deterministic, in-memory provider for tests and dry runs. Records every
/// submitted [`FillPlan`] so assertions can inspect what the executor would
/// broadcast.
pub struct MockProvider {
    state: OnChainState,
    /// Plans passed to [`FillProvider::submit`], in order.
    submitted: Mutex<Vec<FillPlan>>,
    /// Monotonic counter so each submitted plan gets a distinct fake tx hash.
    nonce: AtomicU64,
    /// When set, [`submit`] returns this error instead of succeeding, to
    /// exercise broadcast-failure handling.
    submit_error: Option<String>,
}

impl MockProvider {
    /// A provider with deep inventory, cheap gas, and a far-future clock —
    /// the "happy path" baseline for fillable-quote tests.
    pub fn fillable() -> Self {
        Self::new(OnChainState {
            now: 1_700_000_000,
            gas_price_wei: U256::from(1_000_000_000u64), // 1 gwei
            output_token_balance: U256::MAX,
        })
    }

    pub fn new(state: OnChainState) -> Self {
        Self {
            state,
            submitted: Mutex::new(Vec::new()),
            nonce: AtomicU64::new(0),
            submit_error: None,
        }
    }

    /// Override the on-chain clock.
    pub fn with_now(mut self, now: u64) -> Self {
        self.state.now = now;
        self
    }

    /// Override the gas price (wei).
    pub fn with_gas_price_wei(mut self, gas_price_wei: U256) -> Self {
        self.state.gas_price_wei = gas_price_wei;
        self
    }

    /// Override the executor's output-token inventory.
    pub fn with_output_balance(mut self, balance: U256) -> Self {
        self.state.output_token_balance = balance;
        self
    }

    /// Make [`submit`] fail with `msg` to test broadcast-error handling.
    pub fn with_submit_error(mut self, msg: impl Into<String>) -> Self {
        self.submit_error = Some(msg.into());
        self
    }

    /// All plans submitted so far, in submission order.
    pub fn submitted_plans(&self) -> Vec<FillPlan> {
        self.submitted.lock().expect("mock lock poisoned").clone()
    }

    /// Number of plans submitted (broadcast count).
    pub fn submit_count(&self) -> usize {
        self.submitted.lock().expect("mock lock poisoned").len()
    }
}

#[async_trait]
impl FillProvider for MockProvider {
    async fn state(&self) -> Result<OnChainState> {
        Ok(self.state.clone())
    }

    async fn submit(&self, plan: &FillPlan) -> Result<String> {
        if let Some(msg) = &self.submit_error {
            return Err(crate::error::ResolverError::Execution(msg.clone()));
        }
        self.submitted
            .lock()
            .expect("mock lock poisoned")
            .push(plan.clone());
        let n = self.nonce.fetch_add(1, Ordering::SeqCst);
        Ok(format!("0x{n:064x}"))
    }
}
