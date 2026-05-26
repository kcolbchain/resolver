//! CoW Protocol v2 intent decoder — parses GPv2 orders from the CoW Protocol API.
//!
//! CoW Protocol is a batch auction-based DEX aggregator: solvers compete to
//! produce the best settlement for a batch of orders. Unlike UniswapX (Dutch
//! auctions that decay over time), CoW orders are limit-price orders settled
//! in periodic batches.
//!
//! This decoder fetches open (unfilled) orders from the production CoW API
//! and normalises them into the common Intent format the solver consumes.

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use serde::Deserialize;

use super::{Chain, Intent, IntentDecoder, Protocol};
use crate::error::{ResolverError, Result};

/// CoW Protocol API endpoints per chain.
fn api_url(chain: Chain) -> &'static str {
    match chain {
        Chain::Ethereum => "https://api.cow.fi/mainnet/api/v1/orders?limit=50",
        Chain::Arbitrum => "https://api.cow.fi/arbitrum/api/v1/orders?limit=50",
        Chain::Base => "https://api.cow.fi/base/api/v1/orders?limit=50",
        Chain::Optimism => "https://api.cow.fi/optimism/api/v1/orders?limit=50",
        Chain::Polygon => "https://api.cow.fi/polygon/api/v1/orders?limit=50",
    }
}

/// Raw CoW Protocol order from the API (GPv2 order shape).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct CowOrder {
    uid: String,
    sell_token: String,
    buy_token: String,
    sell_amount: String,
    buy_amount: String,
    fee_amount: String,
    valid_to: u64,
    receiver: String,
    owner: String,
    #[serde(default)]
    partially_fillable: bool,
    #[serde(default)]
    signature: String,
    #[serde(default)]
    app_data: String,
}

#[derive(Debug, Deserialize)]
struct CowApiResponse {
    #[serde(default)]
    orders: Vec<CowOrder>,
}

/// Decodes CoW Protocol GPv2 orders.
pub struct CowProtocolDecoder {
    chain: Chain,
    client: reqwest::Client,
}

impl CowProtocolDecoder {
    pub fn new(chain: Chain) -> Self {
        Self {
            chain,
            client: reqwest::Client::new(),
        }
    }

    fn parse_order(&self, order: &CowOrder) -> Result<Intent> {
        let token_in: Address = order
            .sell_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid sell token: {e}")))?;

        let token_out: Address = order
            .buy_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid buy token: {e}")))?;

        let amount_in: U256 = order
            .sell_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid sell amount: {e}")))?;

        let min_amount_out: U256 = order
            .buy_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid buy amount: {e}")))?;

        let receiver: Address = order
            .receiver
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid receiver: {e}")))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Intent {
            id: order.uid.clone(),
            protocol: Protocol::CowProtocol,
            source_chain: self.chain,
            dest_chain: self.chain,
            token_in,
            token_out,
            amount_in,
            min_amount_out,
            current_amount_out: min_amount_out,
            deadline: order.valid_to,
            recipient: receiver,
            raw_order: hex::decode(
                order
                    .signature
                    .strip_prefix("0x")
                    .unwrap_or(&order.signature),
            )
            .unwrap_or_default(),
            discovered_at: now,
        })
    }
}

#[async_trait]
impl IntentDecoder for CowProtocolDecoder {
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>> {
        let url = api_url(self.chain);

        let resp: CowApiResponse = self
            .client
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| ResolverError::Rpc(format!("CoW Protocol API error: {e}")))?
            .json()
            .await
            .map_err(|e| ResolverError::Rpc(format!("CoW Protocol parse error: {e}")))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let intents: Vec<Intent> = resp
            .orders
            .iter()
            .filter(|o| o.valid_to > now) // skip expired
            .filter_map(|o| self.parse_order(o).ok())
            .collect();

        tracing::info!(
            "Fetched {} open CoW Protocol intents on {:?}",
            intents.len(),
            self.chain
        );
        Ok(intents)
    }

    fn decode(&self, _raw: &[u8]) -> Result<Intent> {
        Err(ResolverError::Intent(
            "Raw order decoding not yet implemented".into(),
        ))
    }

    fn protocol(&self) -> &str {
        "CoWProtocol"
    }
}
