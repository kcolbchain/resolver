use thiserror::Error;

pub type Result<T> = std::result::Result<T, ResolverError>;

#[derive(Error, Debug)]
pub enum ResolverError {
    #[error("Intent error: {0}")]
    Intent(String),

    #[error("Solver error: {0}")]
    Solver(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("No profitable route found for intent {intent_id}")]
    NoProfitableRoute { intent_id: String },

    #[error("Intent expired: {intent_id} at {deadline}")]
    IntentExpired { intent_id: String, deadline: u64 },

    #[error("Simulation failed: {0}")]
    Simulation(String),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}
