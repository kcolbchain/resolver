//! CoW Protocol auction decoder.
//!
//! CoW exposes the current solver batch via the orderbook API. This decoder
//! normalizes solvable auction orders into the shared [`Intent`] shape so the
//! solver can evaluate CoW orders next to UniswapX and Across intents.

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use serde::Deserialize;

use super::{Chain, Intent, IntentDecoder, Protocol};
use crate::error::{ResolverError, Result};

#[derive(Debug, Deserialize)]
struct CowAuction {
    #[serde(default)]
    orders: Vec<CowAuctionOrder>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CowAuctionOrder {
    uid: String,
    sell_token: String,
    buy_token: String,
    sell_amount: String,
    buy_amount: String,
    valid_to: u64,
    receiver: Option<String>,
    owner: String,
}

/// Decodes CoW Protocol GPv2 auction orders.
pub struct CowDecoder {
    chain: Chain,
    client: reqwest::Client,
}

impl CowDecoder {
    pub fn new(chain: Chain) -> Self {
        Self {
            chain,
            client: reqwest::Client::new(),
        }
    }

    fn api_url(chain: Chain) -> Result<&'static str> {
        match chain {
            Chain::Ethereum => Ok("https://api.cow.fi/mainnet/api/v1/auction"),
            Chain::Arbitrum => Ok("https://api.cow.fi/arbitrum_one/api/v1/auction"),
            Chain::Base => Ok("https://api.cow.fi/base/api/v1/auction"),
            Chain::Polygon => Ok("https://api.cow.fi/polygon/api/v1/auction"),
            Chain::Optimism => Err(ResolverError::Intent(
                "CoW Protocol is not supported on Optimism".into(),
            )),
        }
    }

    fn decode_auction_order(&self, order: &CowAuctionOrder) -> Result<Intent> {
        let token_in: Address = order
            .sell_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid CoW sell token: {e}")))?;
        let token_out: Address = order
            .buy_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid CoW buy token: {e}")))?;
        let amount_in: U256 = order
            .sell_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid CoW sell amount: {e}")))?;
        let min_amount_out: U256 = order
            .buy_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid CoW buy amount: {e}")))?;

        let recipient = order.receiver.as_ref().unwrap_or(&order.owner);
        let recipient: Address = recipient
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid CoW receiver: {e}")))?;

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
            recipient,
            raw_order: hex::decode(order.uid.strip_prefix("0x").unwrap_or(&order.uid))
                .unwrap_or_default(),
            discovered_at: now,
        })
    }

    fn decode_auction(&self, auction: &CowAuction) -> Vec<Intent> {
        auction
            .orders
            .iter()
            .filter_map(|order| self.decode_auction_order(order).ok())
            .collect()
    }
}

#[async_trait]
impl IntentDecoder for CowDecoder {
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>> {
        let url = Self::api_url(self.chain)?;
        let resp = match self
            .client
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!("CoW auction API unreachable ({url}): {e}");
                return Ok(Vec::new());
            }
        };

        if !resp.status().is_success() {
            tracing::warn!("CoW auction API returned {} for {url}", resp.status());
            return Ok(Vec::new());
        }

        let auction: CowAuction = match resp.json().await {
            Ok(auction) => auction,
            Err(e) => {
                tracing::warn!("CoW auction API shape drifted: {e}");
                return Ok(Vec::new());
            }
        };

        let intents = self.decode_auction(&auction);
        tracing::info!(
            "Fetched {} open CoW intents on {:?}",
            intents.len(),
            self.chain
        );
        Ok(intents)
    }

    fn decode(&self, _raw: &[u8]) -> Result<Intent> {
        Err(ResolverError::Intent(
            "raw CoW decoding not yet implemented".into(),
        ))
    }

    fn protocol(&self) -> &str {
        "CoW"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_auction() -> CowAuction {
        serde_json::from_str(include_str!("../../tests/fixtures/cow_auction.json")).unwrap()
    }

    #[test]
    fn decodes_fixture_orders_into_intents() {
        let decoder = CowDecoder::new(Chain::Ethereum);
        let intents = decoder.decode_auction(&fixture_auction());

        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].protocol, Protocol::CowProtocol);
        assert_eq!(intents[0].source_chain, Chain::Ethereum);
        assert_eq!(intents[0].dest_chain, Chain::Ethereum);
        assert_eq!(
            intents[0].amount_in,
            U256::from(1_000_000_000_000_000_000u128)
        );
        assert_eq!(intents[0].min_amount_out, U256::from(3_000_000_000u64));
        assert_eq!(intents[0].deadline, 1_900_000_000);
    }

    #[test]
    fn falls_back_to_owner_when_receiver_is_null() {
        let decoder = CowDecoder::new(Chain::Ethereum);
        let intents = decoder.decode_auction(&fixture_auction());

        let owner: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        assert_eq!(intents[1].recipient, owner);
    }

    #[test]
    fn unsupported_chain_returns_error() {
        let err = CowDecoder::api_url(Chain::Optimism).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not supported on Optimism"), "got: {msg}");
    }

    #[test]
    fn raw_decode_is_explicitly_unimplemented() {
        let decoder = CowDecoder::new(Chain::Ethereum);
        let err = decoder.decode(b"").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("raw CoW decoding not yet implemented"),
            "got: {msg}"
        );
    }
}
