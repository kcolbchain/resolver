//! Core intent types — protocol-agnostic representation.

use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};

/// Protocol that originated the intent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    UniswapX,
    Across,
    CowProtocol,
    Custom(String),
}

/// Chain identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Chain {
    Ethereum = 1,
    Arbitrum = 42161,
    Optimism = 10,
    Base = 8453,
    Polygon = 137,
}

impl Chain {
    pub fn chain_id(&self) -> u64 {
        *self as u64
    }

    pub fn from_id(id: u64) -> Option<Self> {
        match id {
            1 => Some(Chain::Ethereum),
            42161 => Some(Chain::Arbitrum),
            10 => Some(Chain::Optimism),
            8453 => Some(Chain::Base),
            137 => Some(Chain::Polygon),
            _ => None,
        }
    }
}

/// A normalized intent — the common format all protocols decode into.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique identifier (order hash or protocol-specific ID).
    pub id: String,

    /// Protocol that originated this intent.
    pub protocol: Protocol,

    /// Chain the intent originates on.
    pub source_chain: Chain,

    /// Chain the intent should be filled on (same chain for swaps, different for bridges).
    pub dest_chain: Chain,

    /// Token the user is selling.
    pub token_in: Address,

    /// Token the user wants to receive.
    pub token_out: Address,

    /// Amount the user is selling.
    pub amount_in: U256,

    /// Minimum amount the user accepts (worst case).
    pub min_amount_out: U256,

    /// Current amount the user would receive (for Dutch auctions, decays over time).
    pub current_amount_out: U256,

    /// Deadline (unix timestamp) — intent is invalid after this.
    pub deadline: u64,

    /// Address that will receive the output tokens.
    pub recipient: Address,

    /// Raw order data for on-chain submission.
    pub raw_order: Vec<u8>,

    /// When we first saw this intent.
    pub discovered_at: u64,
}

impl Intent {
    /// Is this a cross-chain intent?
    pub fn is_cross_chain(&self) -> bool {
        self.source_chain != self.dest_chain
    }

    /// Is this intent expired?
    pub fn is_expired(&self, now: u64) -> bool {
        now > self.deadline
    }

    /// How much surplus can the solver capture?
    /// surplus = current_amount_out - min_amount_out
    pub fn max_surplus(&self) -> U256 {
        if self.current_amount_out > self.min_amount_out {
            self.current_amount_out - self.min_amount_out
        } else {
            U256::ZERO
        }
    }

    /// Time remaining before expiry (in seconds).
    pub fn time_remaining(&self, now: u64) -> u64 {
        self.deadline.saturating_sub(now)
    }
}

/// Result of evaluating an intent for profitability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverQuote {
    pub intent_id: String,
    pub amount_out: U256,
    pub gas_cost_wei: U256,
    pub gas_cost_usd: f64,
    pub net_profit_wei: U256,
    pub net_profit_usd: f64,
    pub route: Vec<RouteStep>,
    pub profitable: bool,
}

/// A single step in a multi-hop route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStep {
    pub dex: String,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub expected_out: U256,
    pub pool: Address,
}
