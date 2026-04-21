//! # resolver
//!
//! High-performance intent solver for DeFi. Parses, simulates, and fills
//! intents across UniswapX, Across, and CoW Protocol.

pub mod execution;
pub mod intents;
pub mod monitor;
pub mod solver;

mod error;
pub use error::{ResolverError, Result};
