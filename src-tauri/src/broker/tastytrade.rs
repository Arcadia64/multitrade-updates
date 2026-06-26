use async_trait::async_trait;
use chrono::{Datelike, DateTime, Timelike, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TASTYTRADE_PROD_API_URL: &str = "https://api.tastyworks.com";
const TASTYTRADE_SANDBOX_API_URL: &str = "https://api.cert.tastyworks.com";
const TASTYTRADE_USER_AGENT: &str = "multitrade/1.0";

// ---------------------------------------------------------------------------
// Internal account model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct TastytradeAccount {
    #[serde(rename = "account-number")]
    account_number: String,
    #[serde(rename = "external-id")]
    external_id: String,
    #[serde(rename = "opened-at")]
    opened_at: String,
    #[serde(rename = "nickname")]
    nickname: String,
    #[serde(rename = "account-type-name")]
    account_type_name: String,
    #[serde(rename = "is-closed")]
    is_closed: bool,
    #[serde(rename = "day-trader-status")]
    day_trader_status: bool,
    #[serde(rename = "is-firm-error")]
    is_firm_error: bool,
    #[serde(rename = "is-firm-proprietary")]
    is_firm_proprietary: bool,
    #[serde(rename = "is-futures-approved")]
    is_futures_approved: bool,
    #[serde(rename = "is-test-drive")]
    is_test_drive: bool,
    #[serde(rename = "margin-or-cash")]
    margin_or_cash: String,
    #[serde(rename = "is-foreign")]
    is_foreign: bool,
    #[serde(rename = "authority-level")]
    authority_level: String,
}

// ---------------------------------------------------------------------------
// Helper response types used only for JSON deserialization
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
struct OAuthTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: String,
    scope: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AccountItem {
    account: TastytradeAccount,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AccountItemsData {
    items: Vec<AccountItem>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AccountsResponse {
    data: AccountItemsData,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct GenericDataResponse {
    data: Value,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ItemsDataResponse {
    data: ItemsWrapper,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ItemsWrapper {
    items: Vec<Value>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct OrderInfo {
    id: i64,
    status: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct OrderData {
    order: OrderInfo,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct OrderResponse {
    data: OrderData,
}

// ---------------------------------------------------------------------------
// The broker struct
// ---------------------------------------------------------------------------

/// Token fields use RwLock for interior mutability so that &self methods
/// can auto-refresh tokens on 401.
pub struct TastytradeBroker {
    id: String,
    access_token: RwLock<String>,
    refresh_token: RwLock<String>,
    client_secret: String,
    token_expiry: RwLock<Option<DateTime<Utc>>>,
    email: String,
    username: String,
    is_logged_in: RwLock<bool>,
    login_time: Option<DateTime<Utc>>,
    event_emitter: Option<EventEmitter>,
    http_client: Client,
    accounts: Vec<TastytradeAccount>,
    use_sandbox: bool,
}

impl TastytradeBroker {
    /// Create a new Tastytrade broker instance.
    pub fn new(id: Option<String>) -> Self {
        let id = id
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            id,
            access_token: RwLock::new(String::new()),
            refresh_token: RwLock::new(String::new()),
            client_secret: String::new(),
            token_expiry: RwLock::new(None),
            email: String::new(),
            username: String::new(),
            is_logged_in: RwLock::new(false),
            login_time: None,
            event_emitter: None,
            http_client,
            accounts: Vec::new(),
            use_sandbox: false,
        }
    }

    // -- helpers ----------------------------------------------------------

    fn base_url(&self) -> &str {
        if self.use_sandbox {
            TASTYTRADE_SANDBOX_API_URL
        } else {
            TASTYTRADE_PROD_API_URL
        }
    }

    fn emit_event(&self, event_name: &str, data: Value) {
        if let Some(ref emitter) = self.event_emitter {
            if event_name == "log" {
                let message = match data {
                    Value::String(msg) => msg,
                    other => other.to_string(),
                };
                emitter(
                    event_name,
                    json!({
                        "message": message,
                        "brokerType": self.get_type().to_string(),
                        "brokerId": self.id.clone(),
                    }),
                );
                return;
            }
            emitter(event_name, data);
        }
    }

    fn emit_log(&self, msg: &str) {
        self.emit_event("log", json!(msg));
    }

    // -- OAuth token management -------------------------------------------

    /// Exchange a refresh token for a new access token via OAuth2.
    async fn exchange_oauth_token(&self) -> Result<OAuthTokenResponse, AppError> {
        let url = format!("{}/oauth/token", self.base_url());
        let refresh_tok = self.refresh_token.read().await.clone();

        let body = json!({
            "grant_type": "refresh_token",
            "client_secret": self.client_secret,
            "refresh_token": refresh_tok
        });

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", TASTYTRADE_USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("failed to send OAuth request: {}", e)))?;

        let status = resp.status();
        let resp_bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(format!("failed to read response: {}", e)))?;

        if !status.is_success() {
            if let Ok(error_resp) = serde_json::from_slice::<Value>(&resp_bytes) {
                if let Some(msg) = error_resp
                    .get("error_description")
                    .and_then(|m| m.as_str())
                {
                    return Err(AppError::AuthFailed(format!(
                        "OAuth token exchange failed: {}",
                        msg
                    )));
                }
                if let Some(msg) = error_resp
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                {
                    return Err(AppError::AuthFailed(format!(
                        "OAuth token exchange failed: {}",
                        msg
                    )));
                }
            }
            return Err(AppError::AuthFailed(format!(
                "OAuth token exchange failed (status {}): {}",
                status.as_u16(),
                String::from_utf8_lossy(&resp_bytes)
            )));
        }

        let result: OAuthTokenResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse OAuth response: {}", e)))?;

        if result.access_token.is_empty() {
            return Err(AppError::AuthFailed(
                "no access token in OAuth response".to_string(),
            ));
        }

        Ok(result)
    }

    /// Store tokens from an OAuth response and compute expiry.
    async fn apply_oauth_tokens(&self, resp: &OAuthTokenResponse) {
        *self.access_token.write().await = resp.access_token.clone();
        if !resp.refresh_token.is_empty() {
            *self.refresh_token.write().await = resp.refresh_token.clone();
        }
        // Set expiry with 60-second buffer
        let expiry = if resp.expires_in > 60 {
            Utc::now() + chrono::Duration::seconds((resp.expires_in - 60) as i64)
        } else {
            Utc::now()
        };
        *self.token_expiry.write().await = Some(expiry);
    }

    /// Check if the current access token has expired or is about to.
    async fn is_token_expired(&self) -> bool {
        match *self.token_expiry.read().await {
            Some(expiry) => Utc::now() >= expiry,
            None => true, // No expiry info means we should refresh
        }
    }

    async fn load_accounts(&mut self) -> Result<(), AppError> {
        let url = format!("{}/customers/me/accounts", self.base_url());
        let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;

        let result: AccountsResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse accounts response: {}", e)))?;

        self.accounts.clear();
        for item in &result.data.items {
            self.accounts.push(item.account.clone());
        }

        Ok(())
    }

    /// Low-level authenticated request helper.
    /// On a 401 response it will attempt a single token refresh and retry.
    async fn make_authenticated_request(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
    ) -> Result<Vec<u8>, AppError> {
        self.do_authenticated_request(method, url, body.clone(), true)
            .await
    }

    async fn do_authenticated_request(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
        allow_retry: bool,
    ) -> Result<Vec<u8>, AppError> {
        // Proactive token refresh if expired
        if self.is_token_expired().await {
            log::info!("Tastytrade: Access token expired, proactively refreshing...");
            if let Err(e) = self.do_refresh_token_inner().await {
                log::warn!("Tastytrade: Proactive refresh failed: {}", e);
            }
        }

        let token = self.access_token.read().await.clone();

        let builder = match method {
            "GET" => self.http_client.get(url),
            "POST" => self.http_client.post(url),
            "PUT" => self.http_client.put(url),
            "DELETE" => self.http_client.delete(url),
            "PATCH" => self.http_client.patch(url),
            _ => self.http_client.get(url),
        };

        let mut builder = builder
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", TASTYTRADE_USER_AGENT);

        if let Some(ref json_body) = body {
            builder = builder.json(json_body);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("request failed: {}", e)))?;

        let status = resp.status();
        let resp_bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(format!("failed to read response: {}", e)))?
            .to_vec();

        if status == reqwest::StatusCode::UNAUTHORIZED && allow_retry {
            // Token expired — attempt refresh via RwLock interior mutability
            log::info!("Tastytrade: Received 401 Unauthorized, attempting to refresh token...");

            if let Err(refresh_err) = self.do_refresh_token_inner().await {
                self.emit_event(
                    "tokenExpired",
                    json!({
                        "brokerId": self.id,
                        "message": "Your Tastytrade session has expired. Please log in again."
                    }),
                );
                return Err(AppError::AuthFailed(format!(
                    "unauthorized and token refresh failed: {}",
                    refresh_err
                )));
            }

            log::info!("Tastytrade: Token refreshed, retrying request...");

            // Retry with new token
            let new_token = self.access_token.read().await.clone();
            let retry_builder = match method {
                "GET" => self.http_client.get(url),
                "POST" => self.http_client.post(url),
                "PUT" => self.http_client.put(url),
                "DELETE" => self.http_client.delete(url),
                "PATCH" => self.http_client.patch(url),
                _ => self.http_client.get(url),
            };

            let mut retry_builder = retry_builder
                .header("Authorization", format!("Bearer {}", new_token))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .header("User-Agent", TASTYTRADE_USER_AGENT);

            if let Some(ref json_body) = body {
                retry_builder = retry_builder.json(json_body);
            }

            let retry_resp = retry_builder
                .send()
                .await
                .map_err(|e| AppError::NetworkError(format!("retry request failed: {}", e)))?;

            let retry_status = retry_resp.status();
            let retry_bytes = retry_resp
                .bytes()
                .await
                .map_err(|e| AppError::NetworkError(format!("failed to read retry response: {}", e)))?
                .to_vec();

            if retry_status == reqwest::StatusCode::UNAUTHORIZED {
                self.emit_event(
                    "tokenExpired",
                    json!({
                        "brokerId": self.id,
                        "message": "Your Tastytrade session has expired. Please log in again."
                    }),
                );
                return Err(AppError::AuthFailed(
                    "unauthorized after token refresh".to_string(),
                ));
            }

            if !retry_status.is_success() {
                return Err(AppError::ApiError(format!(
                    "request failed (status {}): {}",
                    retry_status.as_u16(),
                    String::from_utf8_lossy(&retry_bytes)
                )));
            }

            return Ok(retry_bytes);
        }

        if !status.is_success() {
            return Err(AppError::ApiError(format!(
                "request failed (status {}): {}",
                status.as_u16(),
                String::from_utf8_lossy(&resp_bytes)
            )));
        }

        Ok(resp_bytes)
    }

    /// Refreshes the access token using the OAuth refresh token.
    /// Uses RwLock interior mutability so it works with &self.
    async fn do_refresh_token_inner(&self) -> Result<(), AppError> {
        let refresh_tok = self.refresh_token.read().await.clone();
        if refresh_tok.is_empty() {
            return Err(AppError::AuthFailed(
                "no refresh token available".to_string(),
            ));
        }
        if self.client_secret.is_empty() {
            return Err(AppError::AuthFailed(
                "no client secret available for OAuth refresh".to_string(),
            ));
        }

        match self.exchange_oauth_token().await {
            Ok(oauth_resp) => {
                self.apply_oauth_tokens(&oauth_resp).await;
                self.emit_log("Tastytrade OAuth token refreshed");
                Ok(())
            }
            Err(e) => {
                self.access_token.write().await.clear();
                *self.is_logged_in.write().await = false;
                Err(AppError::AuthFailed(format!(
                    "OAuth token refresh failed: {}",
                    e
                )))
            }
        }
    }

    // -- order helpers ----------------------------------------------------

    fn find_account_number(&self, name_or_id: &str) -> Option<String> {
        for acc in &self.accounts {
            // Direct account number match (highest priority)
            if acc.account_number == name_or_id {
                return Some(acc.account_number.clone());
            }

            // Disambiguated format: "TypeName (AccountNumber)"
            let disambiguated_nickname =
                format!("{} ({})", acc.nickname, acc.account_number);
            let disambiguated_type =
                format!("{} ({})", acc.account_type_name, acc.account_number);
            let disambiguated_account = format!("Account ({})", acc.account_number);

            if name_or_id == disambiguated_nickname
                || name_or_id == disambiguated_type
                || name_or_id == disambiguated_account
            {
                return Some(acc.account_number.clone());
            }

            // Exact nickname match
            if !acc.nickname.is_empty() && acc.nickname == name_or_id {
                return Some(acc.account_number.clone());
            }

            // Exact account type name match
            if !acc.account_type_name.is_empty() && acc.account_type_name == name_or_id {
                return Some(acc.account_number.clone());
            }

            // Legacy display name format
            let display_name = format!("Account {}", acc.account_number);
            if display_name == name_or_id {
                return Some(acc.account_number.clone());
            }
        }
        None
    }

    async fn place_order_on_account(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account_number: &str,
    ) -> String {
        let url = format!("{}/accounts/{}/orders", self.base_url(), account_number);

        // Determine order action
        let order_action = if side.eq_ignore_ascii_case("sell") {
            "Sell to Close"
        } else {
            "Buy to Open"
        };

        self.emit_log(&format!(
            "TASTYTRADE: Placing {} order for {} shares of {} on account {} (action: {})",
            side, shares, ticker, account_number, order_action
        ));

        let request_body = json!({
            "time-in-force": "Day",
            "order-type": "Market",
            "legs": [
                {
                    "instrument-type": "Equity",
                    "symbol": ticker.to_uppercase(),
                    "quantity": shares,
                    "action": order_action
                }
            ]
        });

        let resp_bytes = match self
            .make_authenticated_request("POST", &url, Some(request_body))
            .await
        {
            Ok(b) => b,
            Err(e) => {
                self.emit_log(&format!(
                    "TASTYTRADE: Order failed on account {}: {}",
                    account_number, e
                ));
                return format!("Error: {}", e);
            }
        };

        let action_str = if side.eq_ignore_ascii_case("sell") {
            "SELL"
        } else {
            "BUY"
        };

        match serde_json::from_slice::<OrderResponse>(&resp_bytes) {
            Ok(result) => {
                self.emit_log(&format!(
                    "Order submitted: {} {} shares of {} (Order ID: {}, Status: {})",
                    action_str, shares, ticker, result.data.order.id, result.data.order.status
                ));
                format!(
                    "Order submitted: {} {} shares of {}",
                    action_str, shares, ticker
                )
            }
            Err(e) => {
                format!("Order submitted but failed to parse response: {}", e)
            }
        }
    }

    async fn place_order_all_accounts(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        sell_max: bool,
    ) -> String {
        if self.accounts.is_empty() {
            return "Error: No accounts available".to_string();
        }

        let mut results = Vec::new();
        for acc in &self.accounts {
            if acc.is_closed {
                continue;
            }
            // Only place orders on accounts with trading authority
            if acc.authority_level == "read-only" {
                continue;
            }
            let order_shares = if side.eq_ignore_ascii_case("sell") {
                match self.get_account_holdings(&acc.account_number).await {
                    Ok(holdings) => {
                        let owned = holdings.iter().find_map(|h| {
                            let h_ticker = h.get("ticker").and_then(|t| t.as_str()).unwrap_or("");
                            if h_ticker.eq_ignore_ascii_case(ticker) {
                                h.get("shares").and_then(|v| v.as_f64())
                                    .or_else(|| h.get("shares").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()))
                            } else {
                                None
                            }
                        }).unwrap_or(0.0);
                        if owned <= 0.0 {
                            self.emit_event("log", json!(format!(
                                "Tastytrade {}: No shares of {} in this account, skipping.",
                                acc.account_number, ticker
                            )));
                            continue;
                        }
                        if sell_max {
                            self.emit_event("log", json!(format!(
                                "Tastytrade {}: Selling all {} shares held.",
                                acc.account_number, owned
                            )));
                            owned
                        } else {
                            let capped = shares.min(owned);
                            if (capped - shares).abs() > 0.001 {
                                self.emit_event("log", json!(format!(
                                    "Tastytrade {}: Requested {} shares but account holds {}, selling {}.",
                                    acc.account_number, shares, owned, capped
                                )));
                            }
                            capped
                        }
                    }
                    Err(e) => {
                        if sell_max {
                            self.emit_event("log", json!(format!(
                                "Tastytrade {}: Could not look up holdings: {}. Skipping account.",
                                acc.account_number, e
                            )));
                            continue;
                        }
                        self.emit_event("log", json!(format!(
                            "Tastytrade {}: Could not look up holdings: {}. Using requested shares.",
                            acc.account_number, e
                        )));
                        shares
                    }
                }
            } else {
                shares
            };
            let result = self
                .place_order_on_account(ticker, side, order_shares, &acc.account_number)
                .await;
            let name = if !acc.nickname.is_empty() {
                &acc.nickname
            } else {
                &acc.account_type_name
            };
            results.push(format!("{}: {}", name, result));
        }

        results.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Flexible value extraction (mirrors Go extractStringValue)
// ---------------------------------------------------------------------------

fn extract_string_value(data: &Value, key: &str) -> String {
    match data.get(key) {
        None => "0.00".to_string(),
        Some(val) => match val {
            Value::String(s) if s.is_empty() => "0.00".to_string(),
            Value::String(s) => s.clone(),
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    format!("{:.2}", f)
                } else if let Some(i) = n.as_i64() {
                    format!("{}.00", i)
                } else if let Some(u) = n.as_u64() {
                    format!("{}.00", u)
                } else {
                    "0.00".to_string()
                }
            }
            Value::Null => "0.00".to_string(),
            other => {
                let s = other.to_string();
                if s.is_empty() || s == "null" {
                    "0.00".to_string()
                } else {
                    s
                }
            }
        },
    }
}


/// Flexibly extract a price string from a serde_json::Value (2 decimal places).
fn flexible_price(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                format!("{:.2}", f)
            } else {
                n.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Like flexible_string but formatted with no decimals for integer quantities.
fn flexible_quantity(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                format!("{:.0}", f)
            } else {
                n.to_string()
            }
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Broker trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Broker for TastytradeBroker {
    // -- Identity ---------------------------------------------------------

    fn get_type(&self) -> BrokerType {
        BrokerType::Tastytrade
    }

    fn get_name(&self) -> &str {
        "Tastytrade"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    // -- Authentication ---------------------------------------------------

    /// Tastytrade uses OAuth2 with a client secret and refresh token.
    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.username = email.to_string();
        self.emit_log("Tastytrade uses OAuth. Provide your Client Secret and Refresh Token from https://my.tastytrade.com/app.html#/manage/api-access/open-api/");
        Ok(())
    }

    /// Login using OAuth credentials.
    /// `code` is a composite string: "client_secret,refresh_token"
    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.username = email.to_string();

        // Parse composite "client_secret,refresh_token" from code
        let parts: Vec<&str> = code.splitn(2, ',').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(AppError::AuthFailed(
                "Invalid OAuth credentials format. Expected 'client_secret,refresh_token'.".to_string(),
            ));
        }

        self.client_secret = parts[0].to_string();
        *self.refresh_token.write().await = parts[1].to_string();

        // Exchange refresh token for access token
        let oauth_resp = self.exchange_oauth_token().await
            .map_err(|e| AppError::AuthFailed(format!("failed to authenticate with Tastytrade: {}", e)))?;
        self.apply_oauth_tokens(&oauth_resp).await;

        // Load accounts to verify login
        self.load_accounts()
            .await
            .map_err(|e| AppError::ApiError(format!("failed to load accounts: {}", e)))?;

        *self.is_logged_in.write().await = true;
        self.login_time = Some(Utc::now());
        self.emit_log("Successfully logged in to Tastytrade via OAuth");

        Ok(())
    }

    /// Restore a session from stored credentials (OAuth).
    async fn login_with_stored_credentials(
        &mut self,
        creds: &StoredCredentials,
    ) -> Result<(), AppError> {
        self.email = creds.email.clone();

        // client_secret is stored in device_token field
        self.client_secret = creds.device_token.clone();
        if self.client_secret.is_empty() {
            return Err(AppError::AuthFailed(
                "Tastytrade OAuth credentials missing. Please re-link your account with your Client Secret and Refresh Token.".to_string(),
            ));
        }

        *self.refresh_token.write().await = creds.refresh_token.clone();
        if self.refresh_token.read().await.is_empty() {
            return Err(AppError::AuthFailed(
                "Tastytrade refresh token missing. Please re-link your account.".to_string(),
            ));
        }

        // OAuth access tokens are short-lived (15 min), always refresh on restore
        self.emit_log("Refreshing Tastytrade OAuth token...");
        self.do_refresh_token_inner().await.map_err(|e| {
            AppError::AuthFailed(format!("OAuth token refresh failed: {}", e))
        })?;

        // Load accounts to verify
        self.load_accounts().await.map_err(|e| {
            AppError::AuthFailed(format!(
                "failed to validate refreshed session: {}",
                e
            ))
        })?;

        *self.is_logged_in.write().await = true;
        self.login_time = Some(Utc::now());

        Ok(())
    }

    /// Refresh the access token using the OAuth refresh token.
    async fn refresh_token(&mut self) -> Result<(), AppError> {
        self.do_refresh_token_inner().await
    }

    fn logout(&mut self) {
        // OAuth has no server session to delete — just clear local state
        if let Ok(mut t) = self.access_token.try_write() { t.clear(); }
        if let Ok(mut t) = self.refresh_token.try_write() { t.clear(); }
        if let Ok(mut t) = self.token_expiry.try_write() { *t = None; }
        if let Ok(mut f) = self.is_logged_in.try_write() { *f = false; }
        self.client_secret.clear();
        self.accounts.clear();
        self.emit_log("Logged out from Tastytrade");
    }

    fn is_logged_in(&self) -> bool {
        self.is_logged_in.try_read().map(|v| *v).unwrap_or(false)
    }

    fn get_current_email(&self) -> &str {
        &self.email
    }

    fn get_login_time(&self) -> String {
        match self.login_time {
            Some(t) => t.to_rfc3339(),
            None => String::new(),
        }
    }

    /// Check if the access token is still valid; try refreshing if expired.
    async fn check_token_validity(&mut self) -> bool {
        if !*self.is_logged_in.read().await {
            return false;
        }

        // Proactively refresh if expired
        if self.is_token_expired().await {
            if let Err(_) = self.do_refresh_token_inner().await {
                return false;
            }
        }

        let url = format!("{}/customers/me/accounts", self.base_url());
        let token = self.access_token.read().await.clone();

        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .header("User-Agent", TASTYTRADE_USER_AGENT)
            .send()
            .await;

        match resp {
            Ok(r) => r.status() == reqwest::StatusCode::OK,
            Err(_) => false,
        }
    }

    // -- Credential Export ------------------------------------------------

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        let logged_in = self.is_logged_in.try_read().map(|v| *v).unwrap_or(false);
        if !logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }

        Ok(StoredCredentials {
            broker_type: BrokerType::Tastytrade.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: self.access_token.try_read().map(|v| v.clone()).unwrap_or_default(),
            token_type: "Bearer".to_string(),
            refresh_token: self.refresh_token.try_read().map(|v| v.clone()).unwrap_or_default(),
            device_token: self.client_secret.clone(),
        })
    }

    // -- Account Operations -----------------------------------------------

    async fn get_accounts(&self) -> Result<Vec<Value>, AppError> {
        let accounts = if self.accounts.is_empty() {
            // We need fresh data; fetch inline.  Because the trait method takes
            // &self we cannot mutate.  Instead we do a one-off request.
            let url = format!("{}/customers/me/accounts", self.base_url());
            let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;
            let result: AccountsResponse = serde_json::from_slice(&resp_bytes)
                .map_err(|e| AppError::ApiError(format!("failed to parse accounts response: {}", e)))?;
            result
                .data
                .items
                .into_iter()
                .map(|item| item.account)
                .collect::<Vec<_>>()
        } else {
            self.accounts.clone()
        };

        // First pass: count how many accounts share each base name (excluding closed)
        let mut base_name_counts: HashMap<String, usize> = HashMap::new();
        for acc in &accounts {
            if acc.is_closed {
                continue;
            }
            let base_name = if !acc.nickname.is_empty() {
                acc.nickname.clone()
            } else if !acc.account_type_name.is_empty() {
                acc.account_type_name.clone()
            } else {
                "Account".to_string()
            };
            *base_name_counts.entry(base_name).or_insert(0) += 1;
        }

        // Second pass: build the output list with unique names
        let mut output: Vec<Value> = Vec::with_capacity(accounts.len());
        for acc in &accounts {
            if acc.is_closed {
                continue;
            }

            let base_name = if !acc.nickname.is_empty() {
                acc.nickname.clone()
            } else if !acc.account_type_name.is_empty() {
                acc.account_type_name.clone()
            } else {
                "Account".to_string()
            };

            let name = if base_name_counts.get(&base_name).copied().unwrap_or(0) > 1 {
                format!("{} ({})", base_name, acc.account_number)
            } else {
                base_name
            };

            output.push(json!({
                "id": acc.account_number,
                "name": name,
                "status": "APPROVED",
                "isPrimary": output.is_empty(),
                "accountType": acc.account_type_name,
                "marginOrCash": acc.margin_or_cash,
                "authorityLevel": acc.authority_level,
                "futuresApproved": acc.is_futures_approved,
                "dayTraderStatus": acc.day_trader_status,
            }));
        }

        Ok(output)
    }

    async fn get_account_details(&self, account_id: &str) -> Result<Value, AppError> {
        let url = format!("{}/accounts/{}", self.base_url(), account_id);
        let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;

        let result: GenericDataResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse account details response: {}", e)))?;

        Ok(result.data)
    }

    async fn get_account_holdings(&self, account_id: &str) -> Result<Vec<Value>, AppError> {
        let url = format!("{}/accounts/{}/positions", self.base_url(), account_id);
        let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;

        let result: ItemsDataResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse holdings response: {}", e)))?;

        let mut holdings: Vec<Value> = Vec::new();

        for pos in &result.data.items {
            // Only include equity positions
            let instrument_type = pos
                .get("instrument-type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if instrument_type != "Equity" {
                continue;
            }

            let symbol = pos
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Flexible parsing: quantity may be string or number
            let quantity = pos
                .get("quantity")
                .map(|v| flexible_quantity(v))
                .unwrap_or_default();

            let average_open_price = pos
                .get("average-open-price")
                .map(|v| flexible_price(v))
                .unwrap_or_default();

            let close_price = pos
                .get("close-price")
                .map(|v| flexible_price(v))
                .unwrap_or_default();

            // Calculate market value
            let market_value = {
                let qty: f64 = quantity.parse().unwrap_or(0.0);
                let price: f64 = close_price.parse().unwrap_or(0.0);
                format!("{:.2}", qty * price)
            };

            // Calculate cost basis
            let cost_basis = {
                let qty: f64 = quantity.parse().unwrap_or(0.0);
                let avg_price: f64 = average_open_price.parse().unwrap_or(0.0);
                format!("{:.2}", qty * avg_price)
            };

            holdings.push(json!({
                "ticker": symbol,
                "name": symbol,
                "shares": quantity,
                "price": close_price,
                "marketValue": market_value,
                "costBasis": cost_basis,
                "avgPrice": average_open_price,
            }));
        }

        Ok(holdings)
    }

    async fn get_account_cash(&self, account_id: &str) -> Result<Value, AppError> {
        let url = format!("{}/accounts/{}/balances", self.base_url(), account_id);

        let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;

        let result: GenericDataResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse balances response: {}", e)))?;

        let cash_balance = extract_string_value(&result.data, "cash-balance");
        let buying_power = extract_string_value(&result.data, "derivative-buying-power");
        let equity_buying_power = extract_string_value(&result.data, "equity-buying-power");

        Ok(json!({
            "currency": "USD",
            "balance": {
                "canTrade": equity_buying_power,
                "canWithdraw": cash_balance,
                "buyingPower": buying_power,
            }
        }))
    }

    // -- Trading ----------------------------------------------------------

    async fn place_order(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account: &str,
        sell_max: bool,
    ) -> String {
        if !*self.is_logged_in.read().await {
            return "Error: Not logged in".to_string();
        }

        // If account is "All accounts", place order on all accounts
        if account == "All accounts" {
            return self.place_order_all_accounts(ticker, side, shares, sell_max).await;
        }

        // Parse comma-separated account names
        let requested_names: Vec<&str> = account.split(',').collect();
        if requested_names.len() > 1 {
            let mut results = Vec::new();
            for req_name in &requested_names {
                let req_name = req_name.trim();
                if req_name.is_empty() {
                    continue;
                }

                let account_number = self
                    .find_account_number(req_name)
                    .unwrap_or_else(|| req_name.to_string());

                let order_shares = if side.eq_ignore_ascii_case("sell") {
                    match self.get_account_holdings(&account_number).await {
                        Ok(holdings) => {
                            let owned = holdings.iter().find_map(|h| {
                                let h_ticker = h.get("ticker").and_then(|t| t.as_str()).unwrap_or("");
                                if h_ticker.eq_ignore_ascii_case(ticker) {
                                    h.get("shares").and_then(|v| v.as_f64())
                                        .or_else(|| h.get("shares").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()))
                                } else {
                                    None
                                }
                            }).unwrap_or(0.0);
                            if owned <= 0.0 {
                                self.emit_event("log", json!(format!(
                                    "Tastytrade {}: No shares of {} in this account, skipping.",
                                    account_number, ticker
                                )));
                                continue;
                            }
                            if sell_max {
                                self.emit_event("log", json!(format!(
                                    "Tastytrade {}: Selling all {} shares held.",
                                    account_number, owned
                                )));
                                owned
                            } else {
                                let capped = shares.min(owned);
                                if (capped - shares).abs() > 0.001 {
                                    self.emit_event("log", json!(format!(
                                        "Tastytrade {}: Requested {} shares but account holds {}, selling {}.",
                                        account_number, shares, owned, capped
                                    )));
                                }
                                capped
                            }
                        }
                        Err(e) => {
                            if sell_max {
                                self.emit_event("log", json!(format!(
                                    "Tastytrade {}: Could not look up holdings: {}. Skipping account.",
                                    account_number, e
                                )));
                                continue;
                            }
                            shares
                        }
                    }
                } else {
                    shares
                };
                let result = self
                    .place_order_on_account(ticker, side, order_shares, &account_number)
                    .await;
                results.push(result);
            }
            return results.join("; ");
        }

        // Single account
        let req_name = requested_names[0].trim();
        let account_number = self
            .find_account_number(req_name)
            .unwrap_or_else(|| req_name.to_string());

        let order_shares = if side.eq_ignore_ascii_case("sell") {
            match self.get_account_holdings(&account_number).await {
                Ok(holdings) => {
                    let owned = holdings.iter().find_map(|h| {
                        let h_ticker = h.get("ticker").and_then(|t| t.as_str()).unwrap_or("");
                        if h_ticker.eq_ignore_ascii_case(ticker) {
                            h.get("shares").and_then(|v| v.as_f64())
                                .or_else(|| h.get("shares").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()))
                        } else {
                            None
                        }
                    }).unwrap_or(0.0);
                    if owned <= 0.0 {
                        return format!("No shares of {} in this account", ticker);
                    }
                    if sell_max {
                        self.emit_event("log", json!(format!(
                            "Tastytrade {}: Selling all {} shares held.",
                            account_number, owned
                        )));
                        owned
                    } else {
                        let capped = shares.min(owned);
                        if (capped - shares).abs() > 0.001 {
                            self.emit_event("log", json!(format!(
                                "Tastytrade {}: Requested {} shares but account holds {}, selling {}.",
                                account_number, shares, owned, capped
                            )));
                        }
                        capped
                    }
                }
                Err(e) => {
                    if sell_max {
                        return format!("Error: Could not look up holdings for {}: {}", account_number, e);
                    }
                    shares
                }
            }
        } else {
            shares
        };
        self.place_order_on_account(ticker, side, order_shares, &account_number)
            .await
    }

    // -- Market Info ------------------------------------------------------

    async fn is_market_open(&self) -> Result<bool, AppError> {
        // Simple time-based check for US market hours (EST/EDT approximation).
        // Uses a fixed UTC-5 offset like the Go implementation.
        let est_offset = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
        let now = Utc::now().with_timezone(&est_offset);
        let hour = now.hour();
        let minute = now.minute();
        let weekday = now.weekday();

        // Market is open Monday-Friday, 9:30 AM - 4:00 PM ET
        if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
            return Ok(false);
        }

        if hour < 9 || hour >= 16 {
            return Ok(false);
        }

        if hour == 9 && minute < 30 {
            return Ok(false);
        }

        Ok(true)
    }

    async fn get_stock_quote(&self, ticker: &str) -> Result<Value, AppError> {
        let url = format!(
            "{}/market-data/equity-quotes/{}",
            self.base_url(),
            ticker.to_uppercase()
        );

        let resp_bytes = self.make_authenticated_request("GET", &url, None).await?;

        let result: GenericDataResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| AppError::ApiError(format!("failed to parse quote response: {}", e)))?;

        Ok(result.data)
    }

    // -- Event emission ---------------------------------------------------

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}
