pub mod manager;
pub mod fennel;
pub mod robinhood;
pub mod public_invest;
pub mod tastytrade;
pub mod webull;
pub mod lighthorse;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

use crate::credentials::StoredCredentials;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrokerType {
    Fennel,
    Public,
    Robinhood,
    Tastytrade,
    Webull,
    Lighthorse,
}

impl fmt::Display for BrokerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerType::Fennel => write!(f, "fennel"),
            BrokerType::Public => write!(f, "public"),
            BrokerType::Robinhood => write!(f, "robinhood"),
            BrokerType::Tastytrade => write!(f, "tastytrade"),
            BrokerType::Webull => write!(f, "webull"),
            BrokerType::Lighthorse => write!(f, "lighthorse"),
        }
    }
}

impl BrokerType {
    pub fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "fennel" => Ok(BrokerType::Fennel),
            "public" => Ok(BrokerType::Public),
            "robinhood" => Ok(BrokerType::Robinhood),
            "tastytrade" => Ok(BrokerType::Tastytrade),
            "webull" => Ok(BrokerType::Webull),
            "lighthorse" => Ok(BrokerType::Lighthorse),
            _ => Err(AppError::UnknownBrokerType(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub broker_type: BrokerType,
    pub name: String,
    pub email: String,
    pub status: String,
}

/// Event emitter function type - sends events to the Tauri frontend.
/// Takes an event name and a JSON payload.
pub type EventEmitter = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// The Broker trait maps from the Go Broker interface.
/// All I/O methods are async. Send + Sync bounds are required
/// because Tauri manages state across async command handlers.
#[async_trait]
pub trait Broker: Send + Sync {
    // Identity
    fn get_type(&self) -> BrokerType;
    fn get_name(&self) -> &str;
    fn get_id(&self) -> &str;

    // Authentication
    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError>;
    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError>;
    async fn login_with_stored_credentials(&mut self, creds: &StoredCredentials) -> Result<(), AppError>;
    #[allow(dead_code)]
    async fn refresh_token(&mut self) -> Result<(), AppError>;
    fn logout(&mut self);
    fn is_logged_in(&self) -> bool;
    fn get_current_email(&self) -> &str;
    fn get_login_time(&self) -> String;
    async fn check_token_validity(&mut self) -> bool;

    // Credential Export
    fn export_credentials(&self) -> Result<StoredCredentials, AppError>;

    // Account Operations
    async fn get_accounts(&self) -> Result<Vec<serde_json::Value>, AppError>;
    #[allow(dead_code)]
    async fn get_account_details(&self, account_id: &str) -> Result<serde_json::Value, AppError>;
    async fn get_account_holdings(&self, account_id: &str) -> Result<Vec<serde_json::Value>, AppError>;
    async fn get_account_cash(&self, account_id: &str) -> Result<serde_json::Value, AppError>;

    // Trading
    async fn place_order(&self, ticker: &str, side: &str, shares: f64, account: &str, sell_max: bool) -> String;

    // Market Info
    async fn is_market_open(&self) -> Result<bool, AppError>;
    async fn get_stock_quote(&self, ticker: &str) -> Result<serde_json::Value, AppError>;

    // Event emission
    fn set_event_emitter(&mut self, emitter: EventEmitter);
}
