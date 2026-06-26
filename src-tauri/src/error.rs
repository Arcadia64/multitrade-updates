use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("No broker connected")]
    NoBrokerConnected,

    #[error("Broker not found: {0}")]
    BrokerNotFound(String),

    #[error("Broker not logged in")]
    BrokerNotLoggedIn,

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("MFA required: {0}")]
    MfaRequired(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Credential store error: {0}")]
    CredentialError(String),

    #[error("Unknown broker type: {0}")]
    UnknownBrokerType(String),

    #[error("{0}")]
    Other(String),
}

// Tauri commands require the error type to be serializable.
// We serialize as a string representation of the error.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::NetworkError(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::ApiError(format!("JSON error: {}", e))
    }
}
