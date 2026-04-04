//! UniswapX order decoder — parses Dutch auction orders from the UniswapX API.

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use serde::Deserialize;

use super::{Chain, Intent, IntentDecoder, Protocol};
use crate::error::{ResolverError, Result};

/// UniswapX API endpoints per chain.
fn api_url(chain: Chain) -> &'static str {
    match chain {
        Chain::Ethereum => "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=1",
        Chain::Arbitrum => "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=42161",
        Chain::Base => "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=8453",
        _ => "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=1",
    }
}

/// Raw UniswapX order from the API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UniswapXOrder {
    order_hash: String,
    chain_id: u64,
    #[serde(default)]
    encoded_order: String,
    order_status: String,
    input: OrderInput,
    outputs: Vec<OrderOutput>,
    deadline: u64,
    swapper: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderInput {
    token: String,
    start_amount: String,
    end_amount: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderOutput {
    token: String,
    start_amount: String,
    end_amount: String,
    recipient: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    orders: Vec<UniswapXOrder>,
}

/// Decodes UniswapX Dutch auction orders.
pub struct UniswapXDecoder {
    chain: Chain,
    client: reqwest::Client,
}

impl UniswapXDecoder {
    pub fn new(chain: Chain) -> Self {
        Self {
            chain,
            client: reqwest::Client::new(),
        }
    }

    fn parse_order(&self, order: &UniswapXOrder) -> Result<Intent> {
        let source_chain = Chain::from_id(order.chain_id)
            .ok_or_else(|| ResolverError::Intent(
                format!("Unknown chain ID: {}", order.chain_id)
            ))?;

        let token_in: Address = order.input.token.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input token: {e}")))?;

        let first_output = order.outputs.first()
            .ok_or_else(|| ResolverError::Intent("No outputs in order".into()))?;

        let token_out: Address = first_output.token.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid output token: {e}")))?;

        let amount_in: U256 = order.input.start_amount.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input amount: {e}")))?;

        let min_amount_out: U256 = first_output.end_amount.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid min output: {e}")))?;

        let current_amount_out: U256 = first_output.start_amount.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid current output: {e}")))?;

        let recipient: Address = first_output.recipient.parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid recipient: {e}")))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Intent {
            id: order.order_hash.clone(),
            protocol: Protocol::UniswapX,
            source_chain,
            dest_chain: source_chain, // UniswapX is same-chain
            token_in,
            token_out,
            amount_in,
            min_amount_out,
            current_amount_out,
            deadline: order.deadline,
            recipient,
            raw_order: hex::decode(
                order.encoded_order.strip_prefix("0x").unwrap_or(&order.encoded_order)
            ).unwrap_or_default(),
            discovered_at: now,
        })
    }
}

#[async_trait]
impl IntentDecoder for UniswapXDecoder {
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>> {
        let url = api_url(self.chain);

        let resp: ApiResponse = self.client
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| ResolverError::Rpc(format!("UniswapX API error: {e}")))?
            .json()
            .await
            .map_err(|e| ResolverError::Rpc(format!("UniswapX parse error: {e}")))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let intents: Vec<Intent> = resp.orders
            .iter()
            .filter(|o| o.deadline > now) // skip expired
            .filter_map(|o| self.parse_order(o).ok())
            .collect();

        tracing::info!("Fetched {} open UniswapX intents on {:?}", intents.len(), self.chain);
        Ok(intents)
    }

    fn decode(&self, raw: &[u8]) -> Result<Intent> {
        // For raw on-chain decoding — would parse the EIP-712 signed order
        Err(ResolverError::Intent("Raw order decoding not yet implemented".into()))
    }

    fn protocol(&self) -> &str {
        "UniswapX"
    }
}
