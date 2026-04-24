use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
}
