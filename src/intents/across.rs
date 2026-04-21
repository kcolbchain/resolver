//! Across Protocol V3 intent decoder.
//!
//! Across is a cross-chain intent/bridge protocol: a user deposits on the
//! origin chain, relayers (solvers) fill on the destination chain and are
//! reimbursed on origin after the optimistic oracle period.
//!
//! For the AI-agent / CR8-USD settlement angle: Across is the primary way
//! agents can move value between chains (e.g. Ethereum ↔ Arbitrum) while an
//! intent is in-flight. This decoder normalises Across V3 `FundsDeposited`
//! events into the same [`Intent`] shape the rest of the solver consumes, so
//! the solver can reason about cross-chain agent settlement the same way it
//! reasons about same-chain UniswapX fills.
//!
//! Two entry points:
//!
//! 1. [`AcrossDecoder::fetch_open_intents`] — hits the public `app.across.to`
//!    API to pull suggested-fee quotes for a preset route. Useful as a live
//!    probe; not deterministic, so not exercised by unit tests.
//!
//! 2. [`AcrossDecoder::decode_deposit_event`] — pure, offline parse of a
//!    V3FundsDeposited-shaped JSON payload into an [`Intent`]. This is what
//!    the engine and tests use when replaying fixture data or indexer
//!    output.

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use serde::Deserialize;

use super::{Chain, Intent, IntentDecoder, Protocol};
use crate::error::{ResolverError, Result};

/// Routing preferences a solver can apply when evaluating Across intents.
///
/// The agent-economy thesis: when an AI agent is settling in a stablecoin
/// (CR8-USD-style), it wants to land on a low-fee chain with deep liquidity.
/// Arbitrum is today's default for that profile, so we prefer it. This is
/// advisory — the decoder still emits every intent, but callers can use
/// [`RoutingPreferences::prefers`] to sort or filter.
#[derive(Debug, Clone)]
pub struct RoutingPreferences {
    /// Chain the solver would prefer intents to terminate on.
    pub preferred_dest: Chain,
}

impl Default for RoutingPreferences {
    fn default() -> Self {
        // Arbitrum is the default landing chain for agent settlement: cheap
        // gas, deep USDC/USDT liquidity, and it's where the CR8 agent
        // economy is building.
        Self {
            preferred_dest: Chain::Arbitrum,
        }
    }
}

impl RoutingPreferences {
    /// Returns true if the intent lands on the preferred chain.
    pub fn prefers(&self, intent: &Intent) -> bool {
        intent.dest_chain == self.preferred_dest
    }
}

/// Shape of an Across V3 `FundsDeposited` event, as emitted by indexers
/// (Subgraph, `cast logs --json`, etc.). Field names follow the on-chain
/// event ABI with camelCase rewriting applied by most indexers.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3DepositEvent {
    pub deposit_id: u64,
    pub origin_chain_id: u64,
    pub destination_chain_id: u64,
    pub input_token: String,
    pub output_token: String,
    pub input_amount: String,
    pub output_amount: String,
    pub recipient: String,
    /// Unix seconds after which the deposit expires if unfilled.
    pub fill_deadline: u64,
    #[serde(default)]
    pub depositor: String,
    #[serde(default)]
    pub message: String,
}

/// Across Protocol decoder.
pub struct AcrossDecoder {
    /// Origin chain this decoder pulls intents for (Across is multi-chain; one
    /// decoder instance per origin is idiomatic).
    origin: Chain,
    client: reqwest::Client,
    preferences: RoutingPreferences,
}

impl AcrossDecoder {
    /// Construct a decoder for the given origin chain with default
    /// (Arbitrum-preferring) routing preferences.
    pub fn new(origin: Chain) -> Self {
        Self {
            origin,
            client: reqwest::Client::new(),
            preferences: RoutingPreferences::default(),
        }
    }

    /// Override routing preferences (e.g. if the operator wants to prefer
    /// Base or Optimism instead of Arbitrum).
    pub fn with_preferences(mut self, prefs: RoutingPreferences) -> Self {
        self.preferences = prefs;
        self
    }

    /// Expose the active routing preferences (read-only).
    pub fn preferences(&self) -> &RoutingPreferences {
        &self.preferences
    }

    /// Pure parse: turn a decoded `FundsDeposited` event into our [`Intent`].
    ///
    /// This never touches the network. All unit tests go through this path.
    pub fn decode_deposit_event(&self, event: &V3DepositEvent) -> Result<Intent> {
        let source_chain = Chain::from_id(event.origin_chain_id).ok_or_else(|| {
            ResolverError::Intent(format!("Unknown origin chain: {}", event.origin_chain_id))
        })?;
        let dest_chain = Chain::from_id(event.destination_chain_id).ok_or_else(|| {
            ResolverError::Intent(format!(
                "Unknown destination chain: {}",
                event.destination_chain_id
            ))
        })?;

        let token_in: Address = event
            .input_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input token: {e}")))?;
        let token_out: Address = event
            .output_token
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid output token: {e}")))?;

        let amount_in: U256 = event
            .input_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid input amount: {e}")))?;
        let output_amount: U256 = event
            .output_amount
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid output amount: {e}")))?;

        let recipient: Address = event
            .recipient
            .parse()
            .map_err(|e| ResolverError::Intent(format!("Invalid recipient: {e}")))?;

        // Across V3 orders quote a fixed output amount (no Dutch decay on the
        // output side), so `min_amount_out` and `current_amount_out` are the
        // same value. Solver surplus on Across comes from filling cheaper
        // than the relayer fee the user paid, not from decay.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Deterministic ID: origin chain + depositId is globally unique in
        // Across V3.
        let id = format!("across-{}-{}", event.origin_chain_id, event.deposit_id);

        Ok(Intent {
            id,
            protocol: Protocol::Across,
            source_chain,
            dest_chain,
            token_in,
            token_out,
            amount_in,
            min_amount_out: output_amount,
            current_amount_out: output_amount,
            deadline: event.fill_deadline,
            recipient,
            raw_order: Vec::new(),
            discovered_at: now,
        })
    }
}

#[async_trait]
impl IntentDecoder for AcrossDecoder {
    async fn fetch_open_intents(&self) -> Result<Vec<Intent>> {
        // Across doesn't expose a public "open intents" firehose; solvers
        // subscribe to chain events via Subgraph or direct RPC. For the
        // MVP we hit the `deposits` status endpoint, which returns recent
        // deposits for an origin chain. If the endpoint is unreachable or
        // changes shape we return an empty list rather than erroring — this
        // keeps the engine's cycle loop healthy.
        let url = format!(
            "https://app.across.to/api/deposits?originChainId={}&status=pending&limit=50",
            self.origin.chain_id()
        );

        let resp = match self
            .client
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Across API unreachable ({url}): {e}");
                return Ok(Vec::new());
            }
        };

        #[derive(Deserialize)]
        struct AcrossDepositsResponse {
            #[serde(default)]
            deposits: Vec<V3DepositEvent>,
        }

        let body: AcrossDepositsResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("Across API shape drifted: {e}");
                return Ok(Vec::new());
            }
        };

        let intents: Vec<Intent> = body
            .deposits
            .iter()
            .filter_map(|d| self.decode_deposit_event(d).ok())
            .collect();

        tracing::info!(
            "Fetched {} open Across intents from {:?} (preferred dest: {:?})",
            intents.len(),
            self.origin,
            self.preferences.preferred_dest,
        );
        Ok(intents)
    }

    fn decode(&self, raw: &[u8]) -> Result<Intent> {
        // Raw bytes for Across V3 are ABI-encoded `FundsDeposited` event
        // data. Full ABI decoding lives in a follow-up; today we accept a
        // JSON-encoded `V3DepositEvent` so indexers and tests can exercise
        // the path end-to-end.
        let event: V3DepositEvent = serde_json::from_slice(raw).map_err(|e| {
            ResolverError::Intent(format!("Across decode: not a V3DepositEvent JSON: {e}"))
        })?;
        self.decode_deposit_event(&event)
    }

    fn protocol(&self) -> &str {
        "Across"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_event() -> V3DepositEvent {
        V3DepositEvent {
            deposit_id: 987654,
            origin_chain_id: 1,          // Ethereum
            destination_chain_id: 42161, // Arbitrum — the agent-economy default
            input_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(), // USDC (Ethereum)
            output_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".into(), // USDC (Arbitrum)
            input_amount: "1000000000".into(), // 1 000 USDC
            output_amount: "999500000".into(), // user pays 0.5 USDC relayer fee
            recipient: "0x000000000000000000000000000000000000dEaD".into(),
            fill_deadline: 9_999_999_999, // far future
            depositor: "0x1111111111111111111111111111111111111111".into(),
            message: String::new(),
        }
    }

    #[test]
    fn decodes_eth_to_arbitrum_deposit() {
        let decoder = AcrossDecoder::new(Chain::Ethereum);
        let intent = decoder
            .decode_deposit_event(&fixture_event())
            .expect("fixture should decode");

        assert_eq!(intent.protocol, Protocol::Across);
        assert_eq!(intent.source_chain, Chain::Ethereum);
        assert_eq!(intent.dest_chain, Chain::Arbitrum);
        assert!(intent.is_cross_chain());
        assert_eq!(intent.amount_in, U256::from(1_000_000_000u64));
        assert_eq!(intent.min_amount_out, U256::from(999_500_000u64));
        assert_eq!(intent.id, "across-1-987654");
    }

    #[test]
    fn prefers_arbitrum_by_default() {
        let decoder = AcrossDecoder::new(Chain::Ethereum);
        let intent = decoder.decode_deposit_event(&fixture_event()).unwrap();
        assert!(
            decoder.preferences().prefers(&intent),
            "default preferences should prefer Arbitrum-landing intents"
        );
    }

    #[test]
    fn custom_preferences_override_default() {
        let decoder = AcrossDecoder::new(Chain::Ethereum).with_preferences(RoutingPreferences {
            preferred_dest: Chain::Base,
        });
        let intent = decoder.decode_deposit_event(&fixture_event()).unwrap();
        assert!(
            !decoder.preferences().prefers(&intent),
            "Base-preferring decoder should not prefer an Arbitrum-landing intent"
        );
    }

    #[test]
    fn rejects_unknown_chain() {
        let mut event = fixture_event();
        event.destination_chain_id = 99999;
        let decoder = AcrossDecoder::new(Chain::Ethereum);
        let err = decoder.decode_deposit_event(&event).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Unknown destination chain"), "got: {msg}");
    }

    #[test]
    fn decode_from_json_bytes_works() {
        let json = serde_json::to_vec(&serde_json::json!({
            "depositId": 42,
            "originChainId": 1,
            "destinationChainId": 42161,
            "inputToken": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "outputToken": "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            "inputAmount": "500000000",
            "outputAmount": "499750000",
            "recipient": "0x000000000000000000000000000000000000dEaD",
            "fillDeadline": 9999999999u64,
            "depositor": "0x1111111111111111111111111111111111111111",
            "message": ""
        }))
        .unwrap();

        let decoder = AcrossDecoder::new(Chain::Ethereum);
        let intent = decoder.decode(&json).expect("JSON bytes should decode");
        assert_eq!(intent.id, "across-1-42");
        assert_eq!(intent.dest_chain, Chain::Arbitrum);
    }
}
