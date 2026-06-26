use async_trait::async_trait;
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

const PUBLIC_API_BASE_URL: &str = "https://api.public.com";

/// Represents a Public.com brokerage account, used for caching account data
/// so that order placement can resolve display-names to account IDs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PublicAccount {
    account_id: String,
    account_type: String,
    options_level: String,
    brokerage_account_type: String,
    trade_permissions: String,
}

/// PublicBroker implements the Broker trait for Public.com.
///
/// Authentication flow:
///   1. User provides their API secret key (via the `code` parameter of `login`).
///   2. The secret key is POSTed to generate a short-lived access token (60 min).
///   3. The token is refreshed automatically 5 minutes before expiry, or on a 401.
pub struct PublicBroker {
    id: String,
    secret_key: String,
    access_token: String,
    token_expiry: DateTime<Utc>,
    email: String,
    is_logged_in: bool,
    login_time: Option<DateTime<Utc>>,
    event_emitter: Option<EventEmitter>,
    http_client: Client,
    /// Cached accounts from the last successful GetAccounts call.
    accounts: Vec<PublicAccount>,
    /// Guard to prevent concurrent token refreshes.
    #[allow(dead_code)]
    refresh_lock: Arc<Mutex<()>>,
}

impl PublicBroker {
    pub fn new(id: Option<String>) -> Self {
        let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        Self {
            id,
            secret_key: String::new(),
            access_token: String::new(),
            token_expiry: Utc::now(),
            email: String::new(),
            is_logged_in: false,
            login_time: None,
            event_emitter: None,
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            accounts: Vec::new(),
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    // ---------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------

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

    /// Generate a fresh access token from the stored secret key.
    async fn generate_access_token(&mut self) -> Result<(), AppError> {
        let url = format!("{}/userapiauthservice/personal/access-tokens", PUBLIC_API_BASE_URL);

        let body = json!({
            "validityInMinutes": 60,
            "secret": self.secret_key,
        });

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to send token request: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to read token response: {}", e)))?;

        if !status.is_success() {
            return Err(AppError::AuthFailed(format!(
                "Authentication failed (status {}): {}",
                status, resp_text
            )));
        }

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse token response: {}", e)))?;

        let access_token = parsed
            .get("accessToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::AuthFailed("No access token in response".to_string()))?;

        if access_token.is_empty() {
            return Err(AppError::AuthFailed("Empty access token in response".to_string()));
        }

        self.access_token = access_token.to_string();
        // Refresh 5 minutes before the 60-minute expiry (i.e. 55 minutes from now).
        self.token_expiry = Utc::now() + chrono::Duration::minutes(55);

        Ok(())
    }

    /// Ensure the current access token is still valid; refresh if needed.
    #[allow(dead_code)]
    async fn ensure_valid_token(&mut self) -> Result<(), AppError> {
        if Utc::now() >= self.token_expiry {
            self.do_refresh_token().await?;
        }
        Ok(())
    }

    /// Inner refresh that acquires the lock to prevent concurrent refreshes.
    #[allow(dead_code)]
    async fn do_refresh_token(&mut self) -> Result<(), AppError> {
        let _guard = self.refresh_lock.clone();
        let _lock = _guard.lock().await;
        if self.secret_key.is_empty() {
            return Err(AppError::AuthFailed("No secret key available for refresh".to_string()));
        }
        self.generate_access_token().await
    }

    /// Make an authenticated HTTP request to the Public API.
    ///
    /// Automatically refreshes the token on expiry and retries once on 401.
    async fn make_authenticated_request(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
    ) -> Result<String, AppError> {
        // We cannot call &mut self methods here because `get_accounts`, etc. take `&self`.
        // Token refresh before the call is handled at the call-site or via
        // check_and_refresh (see wrapper below).

        let mut request_builder = match method {
            "POST" => self.http_client.post(url),
            "PUT" => self.http_client.put(url),
            "DELETE" => self.http_client.delete(url),
            "PATCH" => self.http_client.patch(url),
            _ => self.http_client.get(url),
        };

        request_builder = request_builder
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json");

        if let Some(ref b) = body {
            request_builder = request_builder.json(b);
        }

        let resp = request_builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("Request failed: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to read response: {}", e)))?;

        if status.as_u16() == 401 {
            // Return a distinguishable error so callers can retry after refresh.
            return Err(AppError::AuthFailed(format!(
                "Unauthorized (401): {}",
                resp_text
            )));
        }

        if !status.is_success() {
            return Err(AppError::ApiError(format!(
                "Request failed (status {}): {}",
                status, resp_text
            )));
        }

        Ok(resp_text)
    }

    /// Wrapper that retries once on 401 after refreshing the token.
    /// Because the trait methods take `&self`, we keep a second copy of the
    /// secret key + client so we can issue a token-refresh POST without &mut self.
    /// In practice the token is refreshed by the manager calling `refresh_token`
    /// (which takes &mut self).  This retry path issues a standalone refresh.
    async fn authenticated_request_with_retry(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
    ) -> Result<String, AppError> {
        match self.make_authenticated_request(method, url, body.clone()).await {
            Ok(text) => Ok(text),
            Err(AppError::AuthFailed(msg)) if msg.starts_with("Unauthorized") => {
                // Attempt a standalone token refresh by generating a new access token
                // and retrying.  Because we only have &self we do the refresh inline.
                let new_token = self.refresh_access_token_inline().await?;
                // Retry the request with the new token.
                self.make_request_with_token(method, url, body, &new_token).await
            }
            Err(e) => Err(e),
        }
    }

    /// Issue a token refresh without requiring &mut self.
    /// Returns the new access token string.  The caller should use it for retry.
    /// NOTE: this does NOT update self.access_token (we cannot, with &self).
    /// The main refresh path (refresh_token with &mut self) will persist it.
    async fn refresh_access_token_inline(&self) -> Result<String, AppError> {
        if self.secret_key.is_empty() {
            return Err(AppError::AuthFailed("No secret key available for refresh".to_string()));
        }

        let url = format!("{}/userapiauthservice/personal/access-tokens", PUBLIC_API_BASE_URL);
        let body = json!({
            "validityInMinutes": 60,
            "secret": self.secret_key,
        });

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("Token refresh request failed: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to read refresh response: {}", e)))?;

        if !status.is_success() {
            return Err(AppError::AuthFailed(format!(
                "Token refresh failed (status {}): {}",
                status, resp_text
            )));
        }

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse refresh response: {}", e)))?;

        parsed
            .get("accessToken")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::AuthFailed("No access token in refresh response".to_string()))
    }

    /// Make a single authenticated request using a supplied token (for retry path).
    async fn make_request_with_token(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
        token: &str,
    ) -> Result<String, AppError> {
        let mut request_builder = match method {
            "POST" => self.http_client.post(url),
            "PUT" => self.http_client.put(url),
            "DELETE" => self.http_client.delete(url),
            "PATCH" => self.http_client.patch(url),
            _ => self.http_client.get(url),
        };

        request_builder = request_builder
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json");

        if let Some(ref b) = body {
            request_builder = request_builder.json(b);
        }

        let resp = request_builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("Retry request failed: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to read retry response: {}", e)))?;

        if !status.is_success() {
            return Err(AppError::ApiError(format!(
                "Request failed after retry (status {}): {}",
                status, resp_text
            )));
        }

        Ok(resp_text)
    }

    /// Resolve an account name/id to the actual account ID.
    fn resolve_account_id(&self, name_or_id: &str) -> String {
        for acc in &self.accounts {
            let display_name = format!("{} Account", acc.account_type);
            if acc.account_id == name_or_id || display_name == name_or_id {
                return acc.account_id.clone();
            }
        }
        // Fallback: use the input directly as an ID.
        name_or_id.to_string()
    }

    /// Place an order on a single account. Returns a human-readable result string.
    async fn place_order_on_account(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account_id: &str,
    ) -> String {
        let url = format!(
            "{}/userapigateway/trading/{}/order",
            PUBLIC_API_BASE_URL, account_id
        );

        let order_side = if side.eq_ignore_ascii_case("sell") {
            "SELL"
        } else {
            "BUY"
        };

        let order_id = Uuid::new_v4().to_string();

        let request_body = json!({
            "orderId": order_id,
            "instrument": {
                "symbol": ticker.to_uppercase(),
                "type": "EQUITY",
            },
            "orderSide": order_side,
            "orderType": "MARKET",
            "quantity": format_shares(shares),
            "expiration": {
                "timeInForce": "DAY",
            },
            "equityMarketSession": "CORE",
        });

        let resp = self
            .authenticated_request_with_retry("POST", &url, Some(request_body))
            .await;

        match resp {
            Ok(text) => {
                let returned_order_id = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("orderId").and_then(|o| o.as_str()).map(String::from))
                    .unwrap_or_default();

                self.emit_event(
                    "log",
                    json!(format!(
                        "Order submitted: {} {} shares of {} (Order ID: {})",
                        order_side, shares, ticker, returned_order_id
                    )),
                );

                format!(
                    "Order submitted: {} {} shares of {}",
                    order_side, shares, ticker
                )
            }
            Err(e) => {
                self.emit_event("log", json!(format!("Order failed: {}", e)));
                format!("Error: {}", e)
            }
        }
    }

    /// Place an order across all tradeable accounts.
    async fn place_order_all_accounts(&self, ticker: &str, side: &str, shares: f64, sell_max: bool) -> String {
        if self.accounts.is_empty() {
            return "Error: No accounts available".to_string();
        }

        let mut results = Vec::new();
        for acc in &self.accounts {
            if acc.trade_permissions == "RESTRICTED_NO_TRADING" {
                continue;
            }
            let order_shares = if side.eq_ignore_ascii_case("sell") {
                match self.get_account_holdings(&acc.account_id).await {
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
                                "Public {}: No shares of {} in this account, skipping.",
                                acc.account_id, ticker
                            )));
                            continue;
                        }
                        if sell_max {
                            self.emit_event("log", json!(format!(
                                "Public {}: Selling all {} shares held.",
                                acc.account_id, owned
                            )));
                            owned
                        } else {
                            let capped = shares.min(owned);
                            if (capped - shares).abs() > 0.001 {
                                self.emit_event("log", json!(format!(
                                    "Public {}: Requested {} shares but account holds {}, selling {}.",
                                    acc.account_id, shares, owned, capped
                                )));
                            }
                            capped
                        }
                    }
                    Err(e) => {
                        if sell_max {
                            self.emit_event("log", json!(format!(
                                "Public {}: Could not look up holdings: {}. Skipping account.",
                                acc.account_id, e
                            )));
                            continue;
                        }
                        self.emit_event("log", json!(format!(
                            "Public {}: Could not look up holdings: {}. Using requested shares.",
                            acc.account_id, e
                        )));
                        shares
                    }
                }
            } else {
                shares
            };
            let result = self
                .place_order_on_account(ticker, side, order_shares, &acc.account_id)
                .await;
            results.push(format!("{}: {}", acc.account_type, result));
        }

        results.join("; ")
    }
}

/// Format shares for the API: strip unnecessary trailing zeros.
/// Matches Go's %g format (shortest representation).
fn format_shares(shares: f64) -> String {
    if shares == shares.floor() {
        format!("{}", shares as i64)
    } else {
        format!("{}", shares)
    }
}

// ===================================================================
// Broker trait implementation
// ===================================================================

#[async_trait]
impl Broker for PublicBroker {
    fn get_type(&self) -> BrokerType {
        BrokerType::Public
    }

    fn get_name(&self) -> &str {
        "Public"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    /// Public.com uses API keys, not 2FA.  This simply stores the email and
    /// emits a log message telling the user to enter their secret key.
    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.emit_event(
            "log",
            json!("Public.com uses API keys instead of 2FA. Please enter your secret key."),
        );
        Ok(())
    }

    /// `code` is the user's secret API key.
    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.secret_key = code.to_string();

        // Generate access token using the secret key.
        self.generate_access_token().await.map_err(|e| {
            AppError::AuthFailed(format!("Failed to authenticate with Public: {}", e))
        })?;

        // Verify by fetching accounts.
        self.fetch_and_cache_accounts().await.map_err(|e| {
            AppError::AuthFailed(format!("Failed to verify authentication: {}", e))
        })?;

        self.is_logged_in = true;
        self.login_time = Some(Utc::now());
        self.emit_event("log", json!("Successfully logged in to Public.com"));

        Ok(())
    }

    async fn login_with_stored_credentials(&mut self, creds: &StoredCredentials) -> Result<(), AppError> {
        self.email = creds.email.clone();
        // The secret key is stored in the refresh_token field.
        self.secret_key = creds.refresh_token.clone();
        self.access_token = creds.access_token.clone();

        if self.secret_key.is_empty() {
            return Err(AppError::AuthFailed("No secret key stored".to_string()));
        }

        // Generate a fresh access token.
        self.generate_access_token().await.map_err(|e| {
            AppError::AuthFailed(format!("Failed to refresh authentication: {}", e))
        })?;

        // Verify by fetching accounts.
        self.fetch_and_cache_accounts().await.map_err(|e| {
            AppError::AuthFailed(format!("Failed to verify authentication: {}", e))
        })?;

        self.is_logged_in = true;
        self.login_time = Some(Utc::now());

        Ok(())
    }

    async fn refresh_token(&mut self) -> Result<(), AppError> {
        if self.secret_key.is_empty() {
            return Err(AppError::AuthFailed(
                "No secret key available for refresh".to_string(),
            ));
        }
        self.generate_access_token().await
    }

    fn logout(&mut self) {
        self.access_token.clear();
        self.secret_key.clear();
        self.is_logged_in = false;
        self.accounts.clear();
        self.emit_event("log", json!("Logged out from Public.com"));
    }

    fn is_logged_in(&self) -> bool {
        self.is_logged_in
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

    async fn check_token_validity(&mut self) -> bool {
        if !self.is_logged_in || self.access_token.is_empty() {
            return false;
        }

        // Try to fetch accounts to verify the token.
        self.get_accounts().await.is_ok()
    }

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        if !self.is_logged_in {
            return Err(AppError::AuthFailed("Not logged in".to_string()));
        }

        Ok(StoredCredentials {
            broker_type: BrokerType::Public.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: self.access_token.clone(),
            token_type: String::new(),
            refresh_token: self.secret_key.clone(), // Store secret key as refresh_token
            device_token: String::new(),
        })
    }

    async fn get_accounts(&self) -> Result<Vec<Value>, AppError> {
        let url = format!("{}/userapigateway/trading/account", PUBLIC_API_BASE_URL);

        let resp_text = self
            .authenticated_request_with_retry("GET", &url, None)
            .await?;

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse accounts response: {}", e)))?;

        let raw_accounts = parsed
            .get("accounts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut accounts: Vec<Value> = Vec::new();
        for acc_val in &raw_accounts {
            let trade_perms = acc_val
                .get("tradePermissions")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Skip accounts that cannot trade.
            if trade_perms == "RESTRICTED_NO_TRADING" {
                continue;
            }

            let account_id = acc_val
                .get("accountId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let account_type = acc_val
                .get("accountType")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let options_level = acc_val
                .get("optionsLevel")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let brokerage_account_type = acc_val
                .get("brokerageAccountType")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            accounts.push(json!({
                "id": account_id,
                "name": format!("{} Account", account_type),
                "status": "APPROVED",
                "isPrimary": account_type == "BROKERAGE",
                "accountType": account_type,
                "optionsLevel": options_level,
                "brokerageAccountType": brokerage_account_type,
                "tradePermissions": trade_perms,
            }));
        }

        Ok(accounts)
    }

    async fn get_account_details(&self, account_id: &str) -> Result<Value, AppError> {
        let url = format!(
            "{}/userapigateway/trading/{}/portfolio/v2",
            PUBLIC_API_BASE_URL, account_id
        );

        let resp_text = self
            .authenticated_request_with_retry("GET", &url, None)
            .await?;

        let result: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse portfolio response: {}", e)))?;

        Ok(result)
    }

    async fn get_account_holdings(&self, account_id: &str) -> Result<Vec<Value>, AppError> {
        let url = format!(
            "{}/userapigateway/trading/{}/portfolio/v2",
            PUBLIC_API_BASE_URL, account_id
        );

        let resp_text = self
            .authenticated_request_with_retry("GET", &url, None)
            .await?;

        let raw_result: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse holdings response: {}", e)))?;

        let positions = match raw_result.get("positions").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return Ok(Vec::new()),
        };

        let mut holdings: Vec<Value> = Vec::new();
        for pos in positions {
            let instrument = match pos.get("instrument") {
                Some(inst) => inst,
                None => continue,
            };

            let inst_type = instrument
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Only include equity positions.
            if inst_type != "EQUITY" {
                continue;
            }

            let symbol = instrument
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let name = instrument
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let quantity = pos
                .get("quantity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let current_value = pos
                .get("currentValue")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Get last price - handle different possible structures.
            let mut price_str = pos
                .get("lastPrice")
                .and_then(|lp| lp.get("price"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // If lastPrice.price is empty or "0", try to compute from currentValue / quantity.
            if price_str.is_empty() || price_str == "0" {
                if !current_value.is_empty() && !quantity.is_empty() {
                    if let (Ok(cv), Ok(qty)) = (
                        current_value.parse::<f64>(),
                        quantity.parse::<f64>(),
                    ) {
                        if qty > 0.0 {
                            price_str = format!("{:.2}", cv / qty);
                        }
                    }
                }

                // Still empty? Fall back to costBasis.unitCost.
                if price_str.is_empty() || price_str == "0" {
                    if let Some(unit_cost) = pos
                        .get("costBasis")
                        .and_then(|cb| cb.get("unitCost"))
                        .and_then(|v| v.as_str())
                    {
                        if !unit_cost.is_empty() {
                            price_str = unit_cost.to_string();
                        }
                    }
                }
            }

            // Get cost basis total.
            let cost_basis_str = pos
                .get("costBasis")
                .and_then(|cb| cb.get("totalCost"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            holdings.push(json!({
                "ticker": symbol,
                "name": name,
                "shares": quantity,
                "price": price_str,
                "marketValue": current_value,
                "costBasis": cost_basis_str,
            }));
        }

        Ok(holdings)
    }

    async fn get_account_cash(&self, account_id: &str) -> Result<Value, AppError> {
        let url = format!(
            "{}/userapigateway/trading/{}/portfolio/v2",
            PUBLIC_API_BASE_URL, account_id
        );

        let resp_text = self
            .authenticated_request_with_retry("GET", &url, None)
            .await?;

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse cash response: {}", e)))?;

        let buying_power = parsed.get("buyingPower");

        let can_trade = buying_power
            .and_then(|bp| bp.get("buyingPower"))
            .and_then(|v| v.as_str())
            .unwrap_or("0");

        let can_withdraw = buying_power
            .and_then(|bp| bp.get("cashOnlyBuyingPower"))
            .and_then(|v| v.as_str())
            .unwrap_or("0");

        Ok(json!({
            "currency": "USD",
            "balance": {
                "canTrade": can_trade,
                "canWithdraw": can_withdraw,
            }
        }))
    }

    async fn place_order(&self, ticker: &str, side: &str, shares: f64, account: &str, sell_max: bool) -> String {
        if !self.is_logged_in {
            return "Error: Not logged in".to_string();
        }

        // If "All accounts", place on every tradeable account.
        if account == "All accounts" {
            return self.place_order_all_accounts(ticker, side, shares, sell_max).await;
        }

        // Parse comma-separated account names.
        let requested_names: Vec<&str> = account.split(',').collect();
        if requested_names.len() > 1 {
            let mut results = Vec::new();
            for req_name in &requested_names {
                let req_name = req_name.trim();
                if req_name.is_empty() {
                    continue;
                }
                let account_id = self.resolve_account_id(req_name);
                let order_shares = if side.eq_ignore_ascii_case("sell") {
                    match self.get_account_holdings(&account_id).await {
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
                                    "Public {}: No shares of {} in this account, skipping.",
                                    account_id, ticker
                                )));
                                continue;
                            }
                            if sell_max {
                                self.emit_event("log", json!(format!(
                                    "Public {}: Selling all {} shares held.",
                                    account_id, owned
                                )));
                                owned
                            } else {
                                let capped = shares.min(owned);
                                if (capped - shares).abs() > 0.001 {
                                    self.emit_event("log", json!(format!(
                                        "Public {}: Requested {} shares but account holds {}, selling {}.",
                                        account_id, shares, owned, capped
                                    )));
                                }
                                capped
                            }
                        }
                        Err(e) => {
                            if sell_max {
                                self.emit_event("log", json!(format!(
                                    "Public {}: Could not look up holdings: {}. Skipping account.",
                                    account_id, e
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
                    .place_order_on_account(ticker, side, order_shares, &account_id)
                    .await;
                results.push(result);
            }
            return results.join("; ");
        }

        // Single account.
        let req_name = requested_names[0].trim();
        let account_id = self.resolve_account_id(req_name);
        let order_shares = if side.eq_ignore_ascii_case("sell") {
            match self.get_account_holdings(&account_id).await {
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
                            "Public {}: Selling all {} shares held.",
                            account_id, owned
                        )));
                        owned
                    } else {
                        let capped = shares.min(owned);
                        if (capped - shares).abs() > 0.001 {
                            self.emit_event("log", json!(format!(
                                "Public {}: Requested {} shares but account holds {}, selling {}.",
                                account_id, shares, owned, capped
                            )));
                        }
                        capped
                    }
                }
                Err(e) => {
                    if sell_max {
                        return format!("Error: Could not look up holdings for {}: {}", account_id, e);
                    }
                    shares
                }
            }
        } else {
            shares
        };
        self.place_order_on_account(ticker, side, order_shares, &account_id)
            .await
    }

    /// Simple time-based check for US equity market hours (Mon-Fri, 9:30-16:00 ET).
    async fn is_market_open(&self) -> Result<bool, AppError> {
        let eastern = FixedOffset::west_opt(5 * 3600).unwrap();
        let now = Utc::now().with_timezone(&eastern);

        let weekday = now.weekday();
        if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
            return Ok(false);
        }

        let hour = now.hour();
        let minute = now.minute();

        if hour < 9 || hour >= 16 {
            return Ok(false);
        }

        if hour == 9 && minute < 30 {
            return Ok(false);
        }

        Ok(true)
    }

    /// Fetch a stock quote via the POST-based market-data endpoint.
    async fn get_stock_quote(&self, ticker: &str) -> Result<Value, AppError> {
        let url = format!(
            "{}/userapigateway/trading/market-data/quote",
            PUBLIC_API_BASE_URL
        );

        let body = json!({
            "symbols": [ticker.to_uppercase()],
        });

        let resp_text = self
            .authenticated_request_with_retry("POST", &url, Some(body))
            .await?;

        let result: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse quote response: {}", e)))?;

        Ok(result)
    }

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}

// ===================================================================
// Helper method that requires &mut self -- not part of the trait but
// used internally during login flows where we already have &mut self.
// ===================================================================

impl PublicBroker {
    /// Fetch accounts and update the internal cache.
    /// Used during login / login_with_stored_credentials.
    async fn fetch_and_cache_accounts(&mut self) -> Result<Vec<Value>, AppError> {
        let url = format!("{}/userapigateway/trading/account", PUBLIC_API_BASE_URL);

        let resp_text = self
            .make_authenticated_request("GET", &url, None)
            .await?;

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::ApiError(format!("Failed to parse accounts response: {}", e)))?;

        // Cache the raw account structs for order placement.
        if let Some(accs) = parsed.get("accounts").and_then(|v| v.as_array()) {
            self.accounts = accs
                .iter()
                .filter_map(|a| serde_json::from_value::<PublicAccount>(a.clone()).ok())
                .collect();
        }

        let raw_accounts = parsed
            .get("accounts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut accounts: Vec<Value> = Vec::new();
        for acc_val in &raw_accounts {
            let trade_perms = acc_val
                .get("tradePermissions")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if trade_perms == "RESTRICTED_NO_TRADING" {
                continue;
            }

            let account_id = acc_val
                .get("accountId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let account_type = acc_val
                .get("accountType")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let options_level = acc_val
                .get("optionsLevel")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let brokerage_account_type = acc_val
                .get("brokerageAccountType")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            accounts.push(json!({
                "id": account_id,
                "name": format!("{} Account", account_type),
                "status": "APPROVED",
                "isPrimary": account_type == "BROKERAGE",
                "accountType": account_type,
                "optionsLevel": options_level,
                "brokerageAccountType": brokerage_account_type,
                "tradePermissions": trade_perms,
            }));
        }

        Ok(accounts)
    }
}
