//! # resolver
//!
//! High-performance intent solver for DeFi. Parses, simulates, and fills
//! intents across UniswapX, Across, and CoW Protocol.

pub mod intents;
pub mod solver;
pub mod execution;
pub mod monitor;

mod error;
pub use error::{ResolverError, Result};
