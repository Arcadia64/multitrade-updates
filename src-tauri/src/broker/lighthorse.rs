use async_trait::async_trait;
use chrono::{DateTime, Utc, Datelike, Timelike};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use md5::{Digest, Md5};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::Engine;
use uuid::Uuid;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize, Debug)]
struct LighthorseCustomField {
    id: String,
    value: Value,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct LighthorseState {
    cash: f64,
    equity: f64,
    mkt: f64,
    #[serde(rename = "customFields")]
    custom_fields: Option<Vec<LighthorseCustomField>>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct LighthorseStateResponse {
    s: String,
    d: LighthorseState,
}

#[derive(Deserialize, Debug)]
#[serde(default)]
struct LighthorsePosition {
    instrument: String,
    name: String,
    side: String,
    qty: f64,
    last: f64,
    #[serde(rename = "avgPrice")]
    avg_price: f64,
    mkt: f64,
}

impl Default for LighthorsePosition {
    fn default() -> Self {
        Self {
            instrument: String::new(),
            name: String::new(),
            side: String::new(),
            qty: 0.0,
            last: 0.0,
            avg_price: 0.0,
            mkt: 0.0,
        }
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct LighthorsePositionsResponse {
    s: String,
    d: Vec<LighthorsePosition>,
}

#[derive(Deserialize, Debug)]
struct PreviewData {
    #[serde(rename = "confirmId")]
    confirm_id: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PreviewResponse {
    s: String,
    d: PreviewData,
}

#[derive(Deserialize, Debug)]
struct PlaceData {
    #[serde(rename = "orderId")]
    order_id: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PlaceResponse {
    s: String,
    d: PlaceData,
}

pub struct LighthorseBroker {
    id: String,
    api_key: String,
    api_secret: String,
    account_id: String,
    email: String,
    is_logged_in: bool,
    login_time: Option<DateTime<Utc>>,
    event_emitter: Option<EventEmitter>,
    http_client: Client,
}

impl LighthorseBroker {
    pub fn new(id: Option<String>) -> Self {
        let id = id
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            id,
            api_key: String::new(),
            api_secret: String::new(),
            account_id: String::new(),
            email: String::new(),
            is_logged_in: false,
            login_time: None,
            event_emitter: None,
            http_client,
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

    async fn make_signed_request(
        &self,
        method: &str,
        rel_path: &str,
        query: &str,
        body: Option<Value>,
    ) -> Result<Vec<u8>, AppError> {
        if !self.is_logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }
        Self::make_signed_request_raw(
            &self.http_client,
            &self.api_key,
            &self.api_secret,
            method,
            rel_path,
            query,
            body,
        )
        .await
    }

    async fn make_signed_request_raw(
        client: &Client,
        api_key: &str,
        api_secret: &str,
        method: &str,
        rel_path: &str,
        query: &str,
        body: Option<Value>,
    ) -> Result<Vec<u8>, AppError> {
        let base_url = "https://interface.lighthorse.io";
        let path_prefix = "/open-api/lighthorse/v1";
        let request_path = format!("{}{}", path_prefix, rel_path);
        let url = if query.is_empty() {
            format!("{}{}", base_url, request_path)
        } else {
            format!("{}{}?{}", base_url, request_path, query)
        };

        let nonce = Uuid::new_v4().to_string();
        let timestamp = Utc::now().timestamp().to_string();

        let body_str = match body {
            Some(ref b) if !b.is_null() => serde_json::to_string(b).unwrap_or_else(|_| "{}".to_string()),
            _ => "{}".to_string(),
        };

        let mut md5_hasher = Md5::new();
        md5_hasher.update(body_str.as_bytes());
        let body_hash = hex::encode(md5_hasher.finalize());

        let string_to_sign = format!(
            "{}\n{}\n{}\nx-trade-apikey:{}\nx-trade-timestamp:{}\nx-trade-nonce:{}\n{}",
            method.to_uppercase(),
            request_path,
            query,
            api_key,
            timestamp,
            nonce,
            body_hash
        );

        let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
            .map_err(|e| AppError::AuthFailed(format!("Invalid API secret: {}", e)))?;
        mac.update(string_to_sign.as_bytes());
        let hmac_bytes = mac.finalize().into_bytes();
        let hex_signature = hex::encode(hmac_bytes);

        let base64_signature = base64::engine::general_purpose::STANDARD.encode(hex_signature.as_bytes());

        let mut builder = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "DELETE" => client.delete(&url),
            _ => client.get(&url),
        };

        builder = builder
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("x-trade-apikey", api_key)
            .header("x-trade-algorithm", "HMAC-SHA256")
            .header("x-trade-nonce", &nonce)
            .header("x-trade-timestamp", &timestamp)
            .header("x-trade-signature", &base64_signature);

        if method.to_uppercase() == "POST" {
            builder = builder.body(body_str);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("Request failed: {}", e)))?;

        let status = resp.status();
        let resp_bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to read response: {}", e)))?
            .to_vec();

        if !status.is_success() {
            if let Ok(err_val) = serde_json::from_slice::<Value>(&resp_bytes) {
                if let Some(errmsg) = err_val.get("errmsg").and_then(|m| m.as_str()) {
                    return Err(AppError::ApiError(errmsg.to_string()));
                }
                if let Some(s) = err_val.get("s").and_then(|code| code.as_str()) {
                    return Err(AppError::ApiError(format!("API error code: {}", s)));
                }
            }
            return Err(AppError::ApiError(format!(
                "HTTP {} error: {}",
                status.as_u16(),
                String::from_utf8_lossy(&resp_bytes)
            )));
        }

        Ok(resp_bytes)
    }
}

fn get_custom_field_f64(fields: &[LighthorseCustomField], target_id: &str) -> Option<f64> {
    for field in fields {
        if field.id == target_id {
            return match &field.value {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.parse::<f64>().ok(),
                _ => None,
            };
        }
    }
    None
}

#[async_trait]
impl Broker for LighthorseBroker {
    // -- Identity ---------------------------------------------------------

    fn get_type(&self) -> BrokerType {
        BrokerType::Lighthorse
    }

    fn get_name(&self) -> &str {
        "Light Horse"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    // -- Authentication ---------------------------------------------------

    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();
        self.emit_log("Provide your API Key, API Secret, and Account ID.");
        Ok(())
    }

    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();

        let parts: Vec<&str> = code.split(',').collect();
        if parts.len() != 3 || parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
            return Err(AppError::AuthFailed(
                "Invalid credentials format. Expected 'api_key,api_secret,account_id'.".to_string(),
            ));
        }

        let api_key = parts[0].to_string();
        let api_secret = parts[1].to_string();
        let account_id = parts[2].to_string();

        let rel_path = format!("/accounts/{}/state", account_id);
        let _resp_bytes = Self::make_signed_request_raw(
            &self.http_client,
            &api_key,
            &api_secret,
            "GET",
            &rel_path,
            "",
            None,
        )
        .await
        .map_err(|e| AppError::AuthFailed(format!("Authentication validation failed: {}", e)))?;

        self.api_key = api_key;
        self.api_secret = api_secret;
        self.account_id = account_id;
        self.is_logged_in = true;
        self.login_time = Some(Utc::now());
        self.emit_log("Successfully connected to Light Horse.");

        Ok(())
    }

    async fn login_with_stored_credentials(&mut self, creds: &StoredCredentials) -> Result<(), AppError> {
        self.email = creds.email.clone();
        self.api_key = creds.access_token.clone();
        self.api_secret = creds.refresh_token.clone();
        self.account_id = creds.device_token.clone();

        if self.api_key.is_empty() || self.api_secret.is_empty() || self.account_id.is_empty() {
            return Err(AppError::AuthFailed(
                "Stored Light Horse credentials are incomplete.".to_string(),
            ));
        }

        let rel_path = format!("/accounts/{}/state", self.account_id);
        let _resp_bytes = Self::make_signed_request_raw(
            &self.http_client,
            &self.api_key,
            &self.api_secret,
            "GET",
            &rel_path,
            "",
            None,
        )
        .await
        .map_err(|e| AppError::AuthFailed(format!("Failed to validate stored session: {}", e)))?;

        self.is_logged_in = true;
        self.login_time = Some(Utc::now());
        self.emit_log("Successfully restored Light Horse session from stored credentials.");

        Ok(())
    }

    async fn refresh_token(&mut self) -> Result<(), AppError> {
        Ok(())
    }

    fn logout(&mut self) {
        self.api_key.clear();
        self.api_secret.clear();
        self.account_id.clear();
        self.is_logged_in = false;
        self.login_time = None;
        self.emit_log("Logged out from Light Horse.");
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
        if !self.is_logged_in {
            return false;
        }
        let rel_path = format!("/accounts/{}/state", self.account_id);
        let resp = Self::make_signed_request_raw(
            &self.http_client,
            &self.api_key,
            &self.api_secret,
            "GET",
            &rel_path,
            "",
            None,
        )
        .await;
        resp.is_ok()
    }

    // -- Credential Export ------------------------------------------------

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        if !self.is_logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }
        Ok(StoredCredentials {
            broker_type: BrokerType::Lighthorse.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: self.api_key.clone(),
            token_type: "".to_string(),
            refresh_token: self.api_secret.clone(),
            device_token: self.account_id.clone(),
        })
    }

    // -- Account Operations -----------------------------------------------

    async fn get_accounts(&self) -> Result<Vec<Value>, AppError> {
        let rel_path = format!("/accounts/{}/state", self.account_id);
        let _resp_bytes = self.make_signed_request("GET", &rel_path, "", None).await?;

        let name = format!("Light Horse Account ({})", self.account_id);
        Ok(vec![serde_json::json!({
            "id": self.account_id,
            "name": name,
            "status": "APPROVED",
            "isPrimary": true,
        })])
    }

    async fn get_account_details(&self, account_id: &str) -> Result<Value, AppError> {
        let rel_path = format!("/accounts/{}/state", account_id);
        let resp_bytes = self.make_signed_request("GET", &rel_path, "", None).await?;
        let val: Value = serde_json::from_slice(&resp_bytes)?;
        Ok(val)
    }

    async fn get_account_holdings(&self, account_id: &str) -> Result<Vec<Value>, AppError> {
        let rel_path = format!("/accounts/{}/positions", account_id);
        let resp_bytes = self.make_signed_request("GET", &rel_path, "", None).await?;
        let res: LighthorsePositionsResponse = serde_json::from_slice(&resp_bytes)?;

        let mut holdings = Vec::new();
        for pos in res.d {
            holdings.push(serde_json::json!({
                "ticker": pos.instrument,
                "name": pos.name,
                "shares": pos.qty.to_string(),
                "price": format!("{:.2}", pos.last),
                "marketValue": pos.mkt,
                "costBasis": pos.qty * pos.avg_price,
                "avgPrice": pos.avg_price,
            }));
        }
        Ok(holdings)
    }

    async fn get_account_cash(&self, account_id: &str) -> Result<Value, AppError> {
        let rel_path = format!("/accounts/{}/state", account_id);
        let resp_bytes = self.make_signed_request("GET", &rel_path, "", None).await?;
        let res: LighthorseStateResponse = serde_json::from_slice(&resp_bytes)?;

        let cash = res.d.cash;
        let equity = res.d.equity;

        let fields = res.d.custom_fields.unwrap_or_default();
        let day_bp = get_custom_field_f64(&fields, "dayBP").unwrap_or(cash);

        Ok(serde_json::json!({
            "currency": "USD",
            "balance": {
                "canTrade": day_bp,
                "canWithdraw": cash,
                "buyingPower": day_bp,
                "totalAsset": equity,
            }
        }))
    }

    // -- Trading ----------------------------------------------------------

    async fn place_order(&self, ticker: &str, side: &str, shares: f64, account: &str, sell_max: bool) -> String {
        let target_account = if account == "All accounts" || account == self.account_id {
            self.account_id.clone()
        } else {
            self.account_id.clone()
        };

        let mut order_shares = shares;
        let action_str = side.to_uppercase();

        if sell_max && side.eq_ignore_ascii_case("sell") {
            match self.get_account_holdings(&target_account).await {
                Ok(holdings) => {
                    let owned = holdings.iter().find_map(|h| {
                        let h_ticker = h.get("ticker").and_then(|t| t.as_str()).unwrap_or("");
                        if h_ticker.eq_ignore_ascii_case(ticker) {
                            h.get("shares").and_then(|v| {
                                match v {
                                    Value::Number(n) => n.as_f64(),
                                    Value::String(s) => s.parse::<f64>().ok(),
                                    _ => None,
                                }
                            })
                        } else {
                            None
                        }
                    }).unwrap_or(0.0);
                    if owned <= 0.0 {
                        self.emit_log(&format!("Light Horse {}: No shares of {} to sell, skipping.", target_account, ticker));
                        return format!("Error: No shares of {} to sell", ticker);
                    }
                    self.emit_log(&format!("Light Horse {}: Selling all {} shares held.", target_account, owned));
                    order_shares = owned;
                }
                Err(e) => {
                    self.emit_log(&format!("Light Horse {}: Could not look up holdings: {}. Using requested shares.", target_account, e));
                }
            }
        }

        self.emit_log(&format!(
            "LIGHT HORSE: Placing {} order for {} shares of {} on account {}",
            side, order_shares, ticker, target_account
        ));

        // 1. Preview order
        let preview_path = format!("/accounts/{}/previewOrder", target_account);
        let preview_body = serde_json::json!({
            "instrument": ticker.to_uppercase(),
            "side": side.to_lowercase(),
            "type": "market",
            "quantityType": "qty",
            "qty": order_shares.to_string(),
            "category": "stock",
            "durationType": "day",
            "extendHours": "reg"
        });

        let preview_bytes = match self.make_signed_request("POST", &preview_path, "", Some(preview_body.clone())).await {
            Ok(b) => b,
            Err(e) => {
                self.emit_log(&format!("LIGHT HORSE: Preview order failed: {}", e));
                return format!("Error: {}", e);
            }
        };

        let preview_res: PreviewResponse = match serde_json::from_slice(&preview_bytes) {
            Ok(res) => res,
            Err(e) => {
                self.emit_log(&format!("LIGHT HORSE: Failed to parse preview response: {}", e));
                return format!("Error: Failed to parse preview response: {}", e);
            }
        };

        let confirm_id = preview_res.d.confirm_id;
        if confirm_id.is_empty() {
            self.emit_log("LIGHT HORSE: No confirmId returned from preview order.");
            return "Error: No confirmId returned from preview order".to_string();
        }

        // 2. Place order
        let place_path = format!("/accounts/{}/orders", target_account);
        let mut place_body = preview_body;
        place_body.as_object_mut().unwrap().insert("confirmId".to_string(), serde_json::Value::String(confirm_id));

        let place_bytes = match self.make_signed_request("POST", &place_path, "", Some(place_body)).await {
            Ok(b) => b,
            Err(e) => {
                self.emit_log(&format!("LIGHT HORSE: Place order failed: {}", e));
                return format!("Error: {}", e);
            }
        };

        let place_res: PlaceResponse = match serde_json::from_slice(&place_bytes) {
            Ok(res) => res,
            Err(e) => {
                self.emit_log(&format!("LIGHT HORSE: Failed to parse place order response: {}", e));
                return format!("Error: Failed to parse place order response: {}", e);
            }
        };

        let success_msg = format!(
            "Order submitted: {} {} shares of {} (Order ID: {})",
            action_str, order_shares, ticker.to_uppercase(), place_res.d.order_id
        );
        self.emit_log(&success_msg);
        success_msg
    }

    // -- Market Info ------------------------------------------------------

    async fn is_market_open(&self) -> Result<bool, AppError> {
        let est_offset = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
        let now = Utc::now().with_timezone(&est_offset);
        let hour = now.hour();
        let minute = now.minute();
        let weekday = now.weekday();

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

    async fn get_stock_quote(&self, _ticker: &str) -> Result<Value, AppError> {
        Ok(serde_json::json!({}))
    }

    // -- Event emission ---------------------------------------------------

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}
