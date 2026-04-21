//! Intent parsing, validation, and management.
//!
//! Supports ERC-7683 cross-chain intents and protocol-specific order formats
//! (UniswapX Dutch orders, Across deposit orders, CoW Protocol GPv2 orders).

mod across;
mod types;
mod uniswapx;

pub use across::{AcrossDecoder, RoutingPreferences, V3DepositEvent};
pub use types::*;
pub use uniswapx::UniswapXDecoder;

use crate::error::Result;
use async_trait::async_trait;

/// Trait for decoding protocol-specific intents into a common format.
#[async_trait]
pub trait IntentDecoder: Send + Sync {
    /// Fetch open (unfilled) intents from the protocol.
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>>;

    /// Decode a raw on-chain order into our Intent format.
    fn decode(&self, raw: &[u8]) -> Result<Intent>;

    /// Protocol name.
    fn protocol(&self) -> &str;
}
