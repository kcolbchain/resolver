//! Solver-share vault — Rust hooks for the SolverInventoryVault contract.
//!
//! Tracks vault NAV, fill count, drawdown, and attribution by venue.
//! These hooks run alongside the solver engine to update the vault state
//! after each profitable fill.

use alloy::primitives::U256;
use serde::{Deserialize, Serialize};

/// Per-chain vault state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSnapshot {
    pub chain_id: u64,
    pub total_assets: U256,
    pub total_shares: U256,
    pub fill_count: u64,
    pub total_rebate_distributed: U256,
    pub total_inventory_consumed: U256,
    pub adverse_streak: u64,
    pub killed: bool,
    pub timestamp: u64,
}

/// Attribution for a single fill — which venue, which vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillAttribution {
    pub fill_id: String,
    pub venue: String,
    pub chain_id: u64,
    pub inventory_consumed: U256,
    pub rebate_earned: U256,
    pub fill_profit_usd: f64,
}

/// Vault NAV tracker — maintains per-chain vault state.
pub struct VaultTracker {
    snapshots: Vec<VaultSnapshot>,
    attributions: Vec<FillAttribution>,
}

impl VaultTracker {
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            attributions: Vec::new(),
        }
    }

    /// Record a new vault snapshot.
    pub fn record_snapshot(&mut self, snapshot: VaultSnapshot) {
        self.snapshots.push(snapshot);
    }

    /// Record a fill attribution.
    pub fn record_fill(&mut self, attribution: FillAttribution) {
        self.attributions.push(attribution);
    }

    /// Get total fills tracked.
    pub fn total_fills(&self) -> usize {
        self.attributions.len()
    }

    /// Get total rebate distributed across all chains.
    pub fn total_rebate(&self) -> U256 {
        let mut total = U256::ZERO;
        for snap in &self.snapshots {
            total += snap.total_rebate_distributed;
        }
        total
    }

    /// Get attribution by venue.
    pub fn attribution_by_venue(&self) -> Vec<(String, Vec<&FillAttribution>)> {
        use std::collections::HashMap;
        let mut map: HashMap<String, Vec<&FillAttribution>> = HashMap::new();
        for a in &self.attributions {
            map.entry(a.venue.clone()).or_default().push(a);
        }
        map.into_iter().collect()
    }

    /// Check if any vault is in killed state.
    pub fn any_killed(&self) -> bool {
        self.snapshots.iter().any(|s| s.killed)
    }
}

impl Default for VaultTracker {
    fn default() -> Self {
        Self::new()
    }
}
