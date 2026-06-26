use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use chrono::{Datelike, FixedOffset, Timelike, Utc};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::Client;
use serde::Deserialize;
use sha1::Sha1;
use uuid::Uuid;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WEBULL_API_BASE_URL: &str = "https://api.webull.com";
const WEBULL_API_VERSION: &str = "v1";
const SIGN_VERSION: &str = "1.0";
const SIGN_ALGORITHM: &str = "HMAC-SHA1";
const WEBULL_USER_AGENT: &str = "multitrade/1.0";

/// RFC 3986 percent-encoding set.
/// Everything that is NOT unreserved (ALPHA / DIGIT / '-' / '.' / '_' / '~')
/// gets percent-encoded.  We start from `NON_ALPHANUMERIC` and then *remove*
/// the four unreserved non-alphanumeric characters so they pass through.
const RFC3986_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

// ---------------------------------------------------------------------------
// Internal types for JSON deserialization
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SubscriptionEntry {
    account_id: String,
    account_type: String,
    currency: String,
    status: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SubscriptionsResponse {
    subscriptions: Vec<SubscriptionEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PositionEntry {
    instrument_id: String,
    symbol: String,
    qty: String,
    market_value: String,
    cost_price: String,
    last_price: String,
    unrealized_pnl: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PositionsResponse {
    has_next: bool,
    positions: Vec<PositionEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BalanceResponse {
    currency: String,
    total_asset: String,
    cash_balance: String,
    buying_power: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PlaceOrderResponse {
    order_id: String,
}

// ---------------------------------------------------------------------------
// Internal account type stored in the broker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct WebullAccount {
    account_id: String,
    account_type: String,
    currency: String,
    status: String,
}

// ---------------------------------------------------------------------------
// WebullBroker
// ---------------------------------------------------------------------------

pub struct WebullBroker {
    id: String,
    app_key: String,
    app_secret: String,
    email: String,
    is_logged_in: bool,
    login_time: Option<chrono::DateTime<Utc>>,
    event_emitter: Option<EventEmitter>,
    http_client: Client,
    accounts: Vec<WebullAccount>,
}

impl WebullBroker {
    pub fn new(id: String) -> Self {
        let id = if id.is_empty() {
            format!("webull-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0))
        } else {
            id
        };

        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            id,
            app_key: String::new(),
            app_secret: String::new(),
            email: String::new(),
            is_logged_in: false,
            login_time: None,
            event_emitter: None,
            http_client,
            accounts: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Event helpers
    // ------------------------------------------------------------------

    fn emit_event(&self, event_name: &str, data: serde_json::Value) {
        if let Some(ref emitter) = self.event_emitter {
            if event_name == "log" {
                let message = match data {
                    serde_json::Value::String(msg) => msg,
                    other => other.to_string(),
                };
                emitter(
                    event_name,
                    serde_json::json!({
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
        self.emit_event("log", serde_json::Value::String(msg.to_string()));
    }

    // ------------------------------------------------------------------
    // Crypto / signing helpers
    // ------------------------------------------------------------------

    /// Generate a UUID v5 nonce identical to the Go / Python SDK logic.
    fn generate_nonce(&self) -> String {
        let hostname = "multitrade";
        let random_part = Uuid::new_v4().to_string();
        let name = format!("{}{}", hostname, random_part);
        Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
    }

    /// ISO 8601 timestamp in UTC (no fractional seconds).
    fn get_timestamp(&self) -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// MD5 hex digest, upper-case.
    fn md5_hex(&self, data: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(data.as_bytes());
        let result = hasher.finalize();
        hex::encode_upper(result)
    }

    /// HMAC-SHA1 signature, base64-encoded.  The key is `secret + "&"`.
    fn hmac_sha1_sign(&self, data: &str, secret: &str) -> String {
        let key = format!("{}&", secret);
        let mut mac =
            Hmac::<Sha1>::new_from_slice(key.as_bytes()).expect("HMAC can take key of any size");
        mac.update(data.as_bytes());
        let result = mac.finalize().into_bytes();
        base64::engine::general_purpose::STANDARD.encode(result)
    }

    /// RFC 3986 percent-encoding.
    fn percent_encode(&self, s: &str) -> String {
        utf8_percent_encode(s, RFC3986_ENCODE_SET).to_string()
    }

    // ------------------------------------------------------------------
    // Request signing & execution
    // ------------------------------------------------------------------

    /// Build the signed header map (everything except `signature` first,
    /// compute the signature, then insert it).
    fn build_signed_headers(
        &self,
        path: &str,
        query_params: Option<&BTreeMap<String, String>>,
        body_params: Option<&serde_json::Value>,
    ) -> BTreeMap<String, String> {
        let timestamp = self.get_timestamp();
        let nonce = self.generate_nonce();

        // The base header pairs used for signing.  We use a BTreeMap so
        // iteration order is already sorted by key.
        let mut headers: BTreeMap<String, String> = BTreeMap::new();
        headers.insert("app_key".to_string(), self.app_key.clone());
        headers.insert("timestamp".to_string(), timestamp);
        headers.insert("sign_version".to_string(), SIGN_VERSION.to_string());
        headers.insert("sign_algorithm".to_string(), SIGN_ALGORITHM.to_string());
        headers.insert("nonce".to_string(), nonce);
        headers.insert("host".to_string(), "api.webull.com".to_string());

        // 1. Sorted header key=value pairs (percent-encoded)
        let header_parts: Vec<String> = headers
            .iter()
            .map(|(k, v)| {
                format!("{}={}", self.percent_encode(k), self.percent_encode(v))
            })
            .collect();

        // 2. Sorted query params (percent-encoded)
        let query_parts: Option<Vec<String>> = query_params.map(|qp| {
            qp.iter()
                .map(|(k, v)| {
                    format!("{}={}", self.percent_encode(k), self.percent_encode(v))
                })
                .collect()
        });

        // 3. Body digest (MD5 of compact JSON, upper-case hex)
        let body_digest: Option<String> = body_params.and_then(|bp| {
            if let serde_json::Value::Object(map) = bp {
                if map.is_empty() {
                    return None;
                }
            }
            let json_str = serde_json::to_string(bp).unwrap_or_default();
            if json_str.is_empty() || json_str == "{}" {
                None
            } else {
                Some(self.md5_hex(&json_str))
            }
        });

        // Combine: path & header_parts & [query_parts] & [body_digest]
        let mut sign_parts: Vec<String> = Vec::new();
        sign_parts.push(path.to_string());
        sign_parts.push(header_parts.join("&"));
        if let Some(ref qp) = query_parts {
            if !qp.is_empty() {
                sign_parts.push(qp.join("&"));
            }
        }
        if let Some(ref bd) = body_digest {
            sign_parts.push(bd.clone());
        }

        let string_to_sign = sign_parts.join("&");
        let signature = self.hmac_sha1_sign(&string_to_sign, &self.app_secret);

        headers.insert("signature".to_string(), signature);
        headers
    }

    /// Execute a signed HTTP request against the Webull API.
    async fn make_signed_request(
        &self,
        method: &str,
        path: &str,
        query_params: Option<&BTreeMap<String, String>>,
        body_params: Option<&serde_json::Value>,
    ) -> Result<Vec<u8>, AppError> {
        // Full URL
        let mut full_url = format!("{}/{}{}", WEBULL_API_BASE_URL, WEBULL_API_VERSION, path);
        if let Some(qp) = query_params {
            if !qp.is_empty() {
                let qs: Vec<String> = qp
                    .iter()
                    .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                    .collect();
                full_url = format!("{}?{}", full_url, qs.join("&"));
            }
        }

        // Signed headers - note the path for signing includes the version prefix
        let sign_path = format!("/{}{}", WEBULL_API_VERSION, path);
        let signed_headers =
            self.build_signed_headers(&sign_path, query_params, body_params);

        // Build the reqwest request
        let mut builder = match method {
            "POST" => self.http_client.post(&full_url),
            "PUT" => self.http_client.put(&full_url),
            "DELETE" => self.http_client.delete(&full_url),
            "PATCH" => self.http_client.patch(&full_url),
            _ => self.http_client.get(&full_url),
        };

        // Apply signed headers
        for (k, v) in &signed_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        // Standard headers
        builder = builder
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", WEBULL_USER_AGENT)
            .header("Accept-Encoding", "gzip");

        // Body
        if let Some(bp) = body_params {
            builder = builder.json(bp);
        }

        let resp = builder.send().await.map_err(|e| {
            AppError::NetworkError(format!("request failed: {}", e))
        })?;

        let status = resp.status();
        let resp_bytes = resp.bytes().await.map_err(|e| {
            AppError::NetworkError(format!("failed to read response: {}", e))
        })?.to_vec();

        if !status.is_success() {
            // Try to extract a human-readable error from the JSON body
            if let Ok(error_obj) = serde_json::from_slice::<serde_json::Value>(&resp_bytes) {
                if let Some(msg) = error_obj.get("msg").and_then(|v| v.as_str()) {
                    return Err(AppError::ApiError(format!(
                        "API error (status {}): {}",
                        status.as_u16(),
                        msg
                    )));
                }
                if let Some(msg) = error_obj.get("message").and_then(|v| v.as_str()) {
                    return Err(AppError::ApiError(format!(
                        "API error (status {}): {}",
                        status.as_u16(),
                        msg
                    )));
                }
            }
            return Err(AppError::ApiError(format!(
                "request failed (status {}): {}",
                status.as_u16(),
                String::from_utf8_lossy(&resp_bytes)
            )));
        }

        Ok(resp_bytes)
    }

    // ------------------------------------------------------------------
    // Account helpers
    // ------------------------------------------------------------------

    /// Fetch subscriptions from the API and populate `self.accounts`.
    async fn load_accounts(&mut self) -> Result<(), AppError> {
        let resp_bytes =
            self.make_signed_request("GET", "/app/subscriptions/list", None, None)
                .await?;

        let result: SubscriptionsResponse =
            serde_json::from_slice(&resp_bytes).map_err(|e| {
                AppError::ApiError(format!("failed to parse subscriptions response: {}", e))
            })?;

        self.accounts.clear();
        for sub in &result.subscriptions {
            self.accounts.push(WebullAccount {
                account_id: sub.account_id.clone(),
                account_type: sub.account_type.clone(),
                currency: sub.currency.clone(),
                status: sub.status.clone(),
            });
            self.emit_log(&format!(
                "WEBULL: Found account - ID: {}, Type: {}, Currency: {}, Status: {}",
                sub.account_id, sub.account_type, sub.currency, sub.status
            ));
        }

        self.emit_log(&format!(
            "WEBULL: Loaded {} accounts total",
            self.accounts.len()
        ));

        Ok(())
    }

    /// Resolve a display-name or raw ID to a concrete account ID.
    fn find_account_id(&self, name_or_id: &str) -> Option<String> {
        for acc in &self.accounts {
            // Direct account ID match (highest priority)
            if acc.account_id == name_or_id {
                return Some(acc.account_id.clone());
            }

            let base_name = if acc.account_type.is_empty() {
                "Account"
            } else {
                &acc.account_type
            };

            // Disambiguated format: "TypeName (AccountID)"
            let disambiguated = format!("{} ({})", base_name, acc.account_id);
            if name_or_id == disambiguated {
                return Some(acc.account_id.clone());
            }

            // Exact account type match
            if !acc.account_type.is_empty() && acc.account_type == name_or_id {
                return Some(acc.account_id.clone());
            }

            // Legacy format: "TypeName Account"
            let legacy_display = format!("{} Account", acc.account_type);
            if legacy_display == name_or_id {
                return Some(acc.account_id.clone());
            }

            // Legacy disambiguated: "TypeName Account (AccountID)"
            let legacy_disambiguated =
                format!("{} Account ({})", acc.account_type, acc.account_id);
            if legacy_disambiguated == name_or_id {
                return Some(acc.account_id.clone());
            }
        }
        None
    }

    /// Place an order on a single account and return a status string.
    async fn place_order_on_account(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account_id: &str,
    ) -> String {
        let order_side = if side.eq_ignore_ascii_case("sell") {
            "SELL"
        } else {
            "BUY"
        };

        // mt- prefixed client order ID (first 20 chars of a UUID v4)
        let uuid_str = Uuid::new_v4().to_string();
        let truncated = &uuid_str[..20.min(uuid_str.len())];
        let client_order_id = format!("mt-{}", truncated);

        self.emit_log(&format!(
            "WEBULL: Placing {} order for {} shares of {} on account {}",
            order_side, shares, ticker, account_id
        ));

        let body = serde_json::json!({
            "account_id": account_id,
            "client_order_id": client_order_id,
            "stock_order": {
                "symbol": ticker.to_uppercase(),
                "qty": format!("{}", shares),
                "side": order_side,
                "order_type": "MARKET",
                "tif": "DAY",
                "extended_hours_trading": false,
            }
        });

        let resp = self
            .make_signed_request("POST", "/trade/order/place", None, Some(&body))
            .await;

        match resp {
            Ok(resp_bytes) => {
                let parsed: Result<PlaceOrderResponse, _> =
                    serde_json::from_slice(&resp_bytes);
                match parsed {
                    Ok(result) => {
                        self.emit_log(&format!(
                            "WEBULL: Order submitted - {} {} shares of {} (Order ID: {})",
                            order_side, shares, ticker, result.order_id
                        ));
                        format!(
                            "Order submitted: {} {} shares of {}",
                            order_side, shares, ticker
                        )
                    }
                    Err(e) => {
                        format!("Order submitted but failed to parse response: {}", e)
                    }
                }
            }
            Err(e) => {
                self.emit_log(&format!(
                    "WEBULL: Order failed on account {}: {}",
                    account_id, e
                ));
                format!("Error: {}", e)
            }
        }
    }

    /// Place an order on every known account.
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

        let mut results: Vec<String> = Vec::new();
        for acc in &self.accounts {
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
                            self.emit_log(&format!(
                                "Webull {}: No shares of {} in this account, skipping.",
                                acc.account_id, ticker
                            ));
                            continue;
                        }
                        if sell_max {
                            self.emit_log(&format!(
                                "Webull {}: Selling all {} shares held.",
                                acc.account_id, owned
                            ));
                            owned
                        } else {
                            let capped = shares.min(owned);
                            if (capped - shares).abs() > 0.001 {
                                self.emit_log(&format!(
                                    "Webull {}: Requested {} shares but account holds {}, selling {}.",
                                    acc.account_id, shares, owned, capped
                                ));
                            }
                            capped
                        }
                    }
                    Err(e) => {
                        if sell_max {
                            self.emit_log(&format!(
                                "Webull {}: Could not look up holdings: {}. Skipping account.",
                                acc.account_id, e
                            ));
                            continue;
                        }
                        self.emit_log(&format!(
                            "Webull {}: Could not look up holdings: {}. Using requested shares.",
                            acc.account_id, e
                        ));
                        shares
                    }
                }
            } else {
                shares
            };
            let result =
                self.place_order_on_account(ticker, side, order_shares, &acc.account_id)
                    .await;
            results.push(format!("{}: {}", acc.account_type, result));
        }

        results.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Broker trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Broker for WebullBroker {
    // ------------------------------------------------------------------
    // Identity
    // ------------------------------------------------------------------

    fn get_type(&self) -> BrokerType {
        BrokerType::Webull
    }

    fn get_name(&self) -> &str {
        "Webull"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    // ------------------------------------------------------------------
    // Authentication
    // ------------------------------------------------------------------

    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.emit_log(
            "Webull uses API keys for authentication. \
             Please enter your App Key, then your App Secret (separated by a comma).",
        );
        self.emit_log("Format: app_key,app_secret");
        Ok(())
    }

    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();

        let parts: Vec<&str> = code.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err(AppError::AuthFailed(
                "invalid credentials format. Expected: app_key,app_secret".to_string(),
            ));
        }

        self.app_key = parts[0].trim().to_string();
        self.app_secret = parts[1].trim().to_string();

        if self.app_key.is_empty() || self.app_secret.is_empty() {
            return Err(AppError::AuthFailed(
                "both app_key and app_secret are required".to_string(),
            ));
        }

        // Verify credentials by loading accounts
        self.load_accounts().await.map_err(|e| {
            AppError::AuthFailed(format!("failed to authenticate with Webull: {}", e))
        })?;

        self.is_logged_in = true;
        self.login_time = Some(Utc::now());
        self.emit_log("Successfully authenticated with Webull");

        Ok(())
    }

    async fn login_with_stored_credentials(
        &mut self,
        creds: &StoredCredentials,
    ) -> Result<(), AppError> {
        self.email = creds.email.clone();
        self.app_key = creds.access_token.clone(); // app_key stored here
        self.app_secret = creds.refresh_token.clone(); // app_secret stored here

        if self.app_key.is_empty() || self.app_secret.is_empty() {
            return Err(AppError::AuthFailed(
                "missing app credentials".to_string(),
            ));
        }

        // Validate by fetching accounts
        self.load_accounts().await.map_err(|e| {
            AppError::AuthFailed(format!("failed to validate credentials: {}", e))
        })?;

        self.is_logged_in = true;
        self.login_time = Some(Utc::now());

        Ok(())
    }

    async fn refresh_token(&mut self) -> Result<(), AppError> {
        // Webull API keys are long-lived; no refresh needed.
        Ok(())
    }

    fn logout(&mut self) {
        self.app_key.clear();
        self.app_secret.clear();
        self.is_logged_in = false;
        self.accounts.clear();
        self.emit_log("Logged out from Webull");
    }

    fn is_logged_in(&self) -> bool {
        self.is_logged_in
    }

    fn get_current_email(&self) -> &str {
        &self.email
    }

    fn get_login_time(&self) -> String {
        match self.login_time {
            Some(t) => t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            None => String::new(),
        }
    }

    async fn check_token_validity(&mut self) -> bool {
        if !self.is_logged_in || self.app_key.is_empty() || self.app_secret.is_empty() {
            return false;
        }

        self.make_signed_request("GET", "/app/subscriptions/list", None, None)
            .await
            .is_ok()
    }

    // ------------------------------------------------------------------
    // Credential export
    // ------------------------------------------------------------------

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        if !self.is_logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }

        Ok(StoredCredentials {
            broker_type: BrokerType::Webull.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: self.app_key.clone(),
            token_type: String::new(),
            refresh_token: self.app_secret.clone(),
            device_token: String::new(),
        })
    }

    // ------------------------------------------------------------------
    // Account operations
    // ------------------------------------------------------------------

    async fn get_accounts(&self) -> Result<Vec<serde_json::Value>, AppError> {
        // If accounts are empty, fetch them.  Because &self is immutable here
        // we make a fresh API call rather than mutating.
        let accounts = if self.accounts.is_empty() {
            let resp_bytes =
                self.make_signed_request("GET", "/app/subscriptions/list", None, None)
                    .await?;
            let result: SubscriptionsResponse =
                serde_json::from_slice(&resp_bytes).map_err(|e| {
                    AppError::ApiError(format!(
                        "failed to parse subscriptions response: {}",
                        e
                    ))
                })?;
            result
                .subscriptions
                .into_iter()
                .map(|s| WebullAccount {
                    account_id: s.account_id,
                    account_type: s.account_type,
                    currency: s.currency,
                    status: s.status,
                })
                .collect::<Vec<_>>()
        } else {
            self.accounts.clone()
        };

        // First pass: count how many accounts share each base name
        let mut base_name_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for acc in &accounts {
            let base = if acc.account_type.is_empty() {
                "Account".to_string()
            } else {
                acc.account_type.clone()
            };
            *base_name_counts.entry(base).or_insert(0) += 1;
        }

        // Second pass: build the response with unique display names
        let mut result: Vec<serde_json::Value> = Vec::with_capacity(accounts.len());
        for (idx, acc) in accounts.iter().enumerate() {
            let base_name = if acc.account_type.is_empty() {
                "Account".to_string()
            } else {
                acc.account_type.clone()
            };

            let name = if base_name_counts.get(&base_name).copied().unwrap_or(0) > 1 {
                format!("{} ({})", base_name, acc.account_id)
            } else {
                base_name
            };

            result.push(serde_json::json!({
                "id": acc.account_id,
                "name": name,
                "status": acc.status,
                "isPrimary": idx == 0,
                "accountType": acc.account_type,
                "currency": acc.currency,
            }));
        }

        Ok(result)
    }

    async fn get_account_details(
        &self,
        account_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        let mut qp = BTreeMap::new();
        qp.insert("account_id".to_string(), account_id.to_string());

        let resp_bytes =
            self.make_signed_request("GET", "/account/profile", Some(&qp), None)
                .await?;

        let result: serde_json::Value =
            serde_json::from_slice(&resp_bytes).map_err(|e| {
                AppError::ApiError(format!(
                    "failed to parse account profile response: {}",
                    e
                ))
            })?;

        Ok(result)
    }

    async fn get_account_holdings(
        &self,
        account_id: &str,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let mut all_positions: Vec<serde_json::Value> = Vec::new();
        let mut last_instrument_id: Option<String> = None;

        loop {
            let mut qp = BTreeMap::new();
            qp.insert("account_id".to_string(), account_id.to_string());
            qp.insert("page_size".to_string(), "100".to_string());
            if let Some(ref lid) = last_instrument_id {
                qp.insert("last_instrument_id".to_string(), lid.clone());
            }

            let resp_bytes =
                self.make_signed_request("GET", "/account/positions", Some(&qp), None)
                    .await?;

            let result: PositionsResponse =
                serde_json::from_slice(&resp_bytes).map_err(|e| {
                    AppError::ApiError(format!(
                        "failed to parse positions response: {}",
                        e
                    ))
                })?;

            for pos in &result.positions {
                all_positions.push(serde_json::json!({
                    "ticker": pos.symbol,
                    "name": pos.symbol,
                    "shares": pos.qty,
                    "price": pos.last_price,
                    "marketValue": pos.market_value,
                    "costBasis": pos.cost_price,
                    "avgPrice": pos.cost_price,
                    "unrealizedPnL": pos.unrealized_pnl,
                }));
                last_instrument_id = Some(pos.instrument_id.clone());
            }

            if !result.has_next {
                break;
            }
        }

        Ok(all_positions)
    }

    async fn get_account_cash(
        &self,
        account_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        let mut qp = BTreeMap::new();
        qp.insert("account_id".to_string(), account_id.to_string());
        qp.insert(
            "total_asset_currency".to_string(),
            "USD".to_string(),
        );

        let resp_bytes =
            self.make_signed_request("GET", "/account/balance", Some(&qp), None)
                .await?;

        let result: BalanceResponse =
            serde_json::from_slice(&resp_bytes).map_err(|e| {
                AppError::ApiError(format!("failed to parse balance response: {}", e))
            })?;

        Ok(serde_json::json!({
            "currency": result.currency,
            "balance": {
                "canTrade": result.buying_power,
                "canWithdraw": result.cash_balance,
                "buyingPower": result.buying_power,
                "totalAsset": result.total_asset,
            }
        }))
    }

    // ------------------------------------------------------------------
    // Trading
    // ------------------------------------------------------------------

    async fn place_order(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account: &str,
        sell_max: bool,
    ) -> String {
        if !self.is_logged_in {
            return "Error: Not logged in".to_string();
        }

        // All accounts
        if account == "All accounts" {
            return self.place_order_all_accounts(ticker, side, shares, sell_max).await;
        }

        // Multiple comma-separated account names
        let requested_names: Vec<&str> = account.split(',').collect();
        if requested_names.len() > 1 {
            let mut results: Vec<String> = Vec::new();
            for req_name in &requested_names {
                let req_name = req_name.trim();
                if req_name.is_empty() {
                    continue;
                }
                let account_id = self
                    .find_account_id(req_name)
                    .unwrap_or_else(|| req_name.to_string());
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
                                self.emit_log(&format!(
                                    "Webull {}: No shares of {} in this account, skipping.",
                                    account_id, ticker
                                ));
                                continue;
                            }
                            if sell_max {
                                self.emit_log(&format!(
                                    "Webull {}: Selling all {} shares held.",
                                    account_id, owned
                                ));
                                owned
                            } else {
                                let capped = shares.min(owned);
                                if (capped - shares).abs() > 0.001 {
                                    self.emit_log(&format!(
                                        "Webull {}: Requested {} shares but account holds {}, selling {}.",
                                        account_id, shares, owned, capped
                                    ));
                                }
                                capped
                            }
                        }
                        Err(e) => {
                            if sell_max {
                                self.emit_log(&format!(
                                    "Webull {}: Could not look up holdings: {}. Skipping account.",
                                    account_id, e
                                ));
                                continue;
                            }
                            shares
                        }
                    }
                } else {
                    shares
                };
                let result =
                    self.place_order_on_account(ticker, side, order_shares, &account_id)
                        .await;
                results.push(result);
            }
            return results.join("; ");
        }

        // Single account
        let req_name = requested_names[0].trim();
        let account_id = self
            .find_account_id(req_name)
            .unwrap_or_else(|| req_name.to_string());
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
                        self.emit_log(&format!(
                            "Webull {}: Selling all {} shares held.",
                            account_id, owned
                        ));
                        owned
                    } else {
                        let capped = shares.min(owned);
                        if (capped - shares).abs() > 0.001 {
                            self.emit_log(&format!(
                                "Webull {}: Requested {} shares but account holds {}, selling {}.",
                                account_id, shares, owned, capped
                            ));
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

    // ------------------------------------------------------------------
    // Market info
    // ------------------------------------------------------------------

    async fn is_market_open(&self) -> Result<bool, AppError> {
        // EST = UTC-5
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        let now = Utc::now().with_timezone(&est);
        let hour = now.hour();
        let minute = now.minute();
        let weekday = now.weekday();

        // Closed on weekends
        if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
            return Ok(false);
        }

        // Regular hours: 9:30 AM - 4:00 PM ET
        if hour < 9 || hour >= 16 {
            return Ok(false);
        }
        if hour == 9 && minute < 30 {
            return Ok(false);
        }

        Ok(true)
    }

    async fn get_stock_quote(
        &self,
        ticker: &str,
    ) -> Result<serde_json::Value, AppError> {
        let mut qp = BTreeMap::new();
        qp.insert("symbol".to_string(), ticker.to_uppercase());

        let resp_bytes =
            self.make_signed_request("GET", "/market/quote", Some(&qp), None)
                .await?;

        let result: serde_json::Value =
            serde_json::from_slice(&resp_bytes).map_err(|e| {
                AppError::ApiError(format!("failed to parse quote response: {}", e))
            })?;

        Ok(result)
    }

    // ------------------------------------------------------------------
    // Event emission
    // ------------------------------------------------------------------

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}

/// URL-encode helper module used inside `make_signed_request` for building
/// the query string (standard form encoding, not the RFC 3986 signing
/// variant).
mod urlencoding {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

    /// Percent-encode a value for use in a URL query string.
    /// This is a simple wrapper that encodes everything except unreserved
    /// characters, similar to Go's `url.QueryEscape` but with `%20` for
    /// spaces (since we join with `&` ourselves).
    pub fn encode(s: &str) -> String {
        // The NON_ALPHANUMERIC set encodes everything except ASCII
        // alphanumerics.  We manually un-encode the unreserved chars
        // that should pass through: - . _ ~
        let raw = utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();
        raw.replace("%2D", "-")
            .replace("%2E", ".")
            .replace("%5F", "_")
            .replace("%7E", "~")
    }
}
