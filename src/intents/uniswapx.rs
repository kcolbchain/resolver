//! UniswapX order decoder — parses Dutch auction orders from the UniswapX API.

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use serde::Deserialize;

use super::{Chain, Intent, IntentDecoder, Protocol};
use crate::error::{ResolverError, Result};

/// UniswapX API endpoints per chain.
fn api_url(chain: Chain) -> Result<&'static str> {
    match chain {
        Chain::Ethereum => Ok("https://api.uniswap.org/v2/orders?orderStatus=open&chainId=1"),
        Chain::Arbitrum => Ok("https://api.uniswap.org/v2/orders?orderStatus=open&chainId=42161"),
        Chain::Base => Ok("https://api.uniswap.org/v2/orders?orderStatus=open&chainId=8453"),
        Chain::Optimism | Chain::Polygon | Chain::Unichain => Err(unsupported_chain_error(chain)),
    }
}

fn unsupported_chain_error(chain: Chain) -> ResolverError {
    ResolverError::Intent(format!("UniswapX not supported on {chain:?}"))
}

/// Raw UniswapX order from the API.
///
/// `order_status`, `swapper`, and `OrderInput::end_amount` are parsed from the
/// API for completeness and future use (audit logs, risk filters, Dutch-decay
/// curves on the input side) but not read on the hot path yet.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    pub fn new(chain: Chain) -> Result<Self> {
        api_url(chain)?;

        Ok(Self {
            chain,
            client: reqwest::Client::new(),
        })
    }

    fn parse_order(&self, order: &UniswapXOrder) -> Result<Intent> {
        let source_chain = Chain::from_id(order.chain_id).ok_or_else(|| {
            ResolverError::Intent(format!("Unknown chain ID: {}", order.chain_id))
        })?;

        let token_in: Address = order
            .input
            .token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input token: {e}")))?;

        let first_output = order
            .outputs
            .first()
            .ok_or_else(|| ResolverError::Intent("No outputs in order".into()))?;

        let token_out: Address = first_output
            .token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid output token: {e}")))?;

        let amount_in: U256 = order
            .input
            .start_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input amount: {e}")))?;

        let min_amount_out: U256 = first_output
            .end_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid min output: {e}")))?;

        let current_amount_out: U256 = first_output
            .start_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid current output: {e}")))?;

        let recipient: Address = first_output
            .recipient
            .parse()
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
                order
                    .encoded_order
                    .strip_prefix("0x")
                    .unwrap_or(&order.encoded_order),
            )
            .unwrap_or_default(),
            discovered_at: now,
        })
    }
}

#[async_trait]
impl IntentDecoder for UniswapXDecoder {
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>> {
        let url = api_url(self.chain)?;

        let resp: ApiResponse = self
            .client
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

        let intents: Vec<Intent> = resp
            .orders
            .iter()
            .filter(|o| o.deadline > now) // skip expired
            .filter_map(|o| self.parse_order(o).ok())
            .collect();

        tracing::info!(
            "Fetched {} open UniswapX intents on {:?}",
            intents.len(),
            self.chain
        );
        Ok(intents)
    }

    fn decode(&self, _raw: &[u8]) -> Result<Intent> {
        // For raw on-chain decoding — would parse the EIP-712 signed order.
        // Tracked as a TODO; stub returns an error rather than panicking.
        Err(ResolverError::Intent(
            "Raw order decoding not yet implemented".into(),
        ))
    }

    fn protocol(&self) -> &str {
        "UniswapX"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_error(chain: Chain) -> ResolverError {
        match UniswapXDecoder::new(chain) {
            Ok(_) => panic!("expected {chain:?} to be unsupported"),
            Err(err) => err,
        }
    }

    #[test]
    fn api_url_uses_explicit_supported_chains() {
        assert_eq!(
            api_url(Chain::Ethereum).unwrap(),
            "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=1"
        );
        assert_eq!(
            api_url(Chain::Arbitrum).unwrap(),
            "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=42161"
        );
        assert_eq!(
            api_url(Chain::Base).unwrap(),
            "https://api.uniswap.org/v2/orders?orderStatus=open&chainId=8453"
        );
    }

    #[test]
    fn decoder_rejects_unsupported_uniswapx_chains() {
        for chain in [Chain::Optimism, Chain::Polygon, Chain::Unichain] {
            let err = expect_error(chain);
            assert_eq!(
                err.to_string(),
                format!("Intent error: UniswapX not supported on {chain:?}")
            );
        }
    }
}
