use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

// Auth0 client ID for Fennel
const FENNEL_CLIENT_ID: &str = "FXGlhcVdamwozAFp8BZ2MWl6coPl6agX";

// API endpoints
const FENNEL_AUTH_URL: &str = "https://accounts.fennel.com";
const FENNEL_API_URL: &str = "https://fennel-api.prod.fennel.com/graphql/";
const FENNEL_AUDIENCE: &str = "https://meta.api.fennel.com/graphql";
const FENNEL_USER_AGENT: &str = "Dart/3.3 (dart:io)";

/// FennelBroker implements the Broker trait for the Fennel brokerage.
/// Token fields use RwLock for interior mutability so that &self methods
/// can auto-refresh tokens on 401, matching Go's graphqlQuery behavior.
pub struct FennelBroker {
    id: String,
    auth_token: RwLock<String>,
    refresh_token_str: RwLock<String>,
    is_logged_in: RwLock<bool>,
    email: String,
    login_time: Option<DateTime<Utc>>,
    client: Client,
    event_emitter: Option<EventEmitter>,
}

impl FennelBroker {
    /// Creates a new Fennel broker instance.
    /// If `id` is empty, a new UUID-based ID is generated.
    pub fn new(id: String) -> Self {
        let broker_id = if id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            id
        };

        Self {
            id: broker_id,
            auth_token: RwLock::new(String::new()),
            refresh_token_str: RwLock::new(String::new()),
            is_logged_in: RwLock::new(false),
            email: String::new(),
            login_time: None,
            client: Client::new(),
            event_emitter: None,
        }
    }

    /// Emit an event to the frontend if an emitter is set.
    fn emit(&self, event_name: &str, data: Value) {
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

    /// Makes a GraphQL request to the Fennel API.
    /// Handles 401 responses by attempting a token refresh and retrying.
    /// Matches Go's graphqlQuery: serialize payload to bytes first, send raw body.
    async fn graphql_query(
        &self,
        url: &str,
        query: &str,
        variables: Option<Value>,
    ) -> Result<Value, AppError> {
        let payload = json!({
            "operationName": null,
            "query": query,
            "variables": variables.unwrap_or(json!({})),
        });

        // Serialize payload to bytes exactly like Go's json.Marshal(payload)
        let json_data = serde_json::to_vec(&payload)
            .map_err(|e| AppError::ApiError(format!("failed to serialize payload: {}", e)))?;

        let host = if url.contains("accounts.fennel.com") {
            "accounts.fennel.com"
        } else {
            "fennel-api.prod.fennel.com"
        };

        let token = self.auth_token.read().await.clone();

        // Build and send request exactly like Go: raw body bytes, manual headers only
        let build_request = |client: &Client, token: &str, body: Vec<u8>| {
            let mut builder = client
                .post(url)
                .header("accept", "*/*")
                .header("content-type", "application/json")
                .header("user-agent", FENNEL_USER_AGENT)
                .header("host", host)
                .body(body);

            if !token.is_empty() {
                builder = builder.header("authorization", format!("Bearer {}", token));
            }

            builder
        };

        let resp = build_request(&self.client, &token, json_data.clone())
            .send()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            println!("Received 401 Unauthorized, attempting to refresh token...");

            self.refresh_token_inner().await?;

            println!("Token refreshed, retrying original request...");

            let new_token = self.auth_token.read().await.clone();
            let retry_resp = build_request(&self.client, &new_token, json_data.clone())
                .send()
                .await
                .map_err(|e| AppError::NetworkError(e.to_string()))?;

            if !retry_resp.status().is_success() {
                return Err(AppError::ApiError(format!(
                    "request failed after token refresh, status code: {}",
                    retry_resp.status().as_u16()
                )));
            }

            let result: Value = retry_resp
                .json()
                .await
                .map_err(|e| AppError::ApiError(format!("failed to decode response: {}", e)))?;

            return Ok(result);
        } else if !resp.status().is_success() {
            return Err(AppError::ApiError(format!(
                "graphql query failed, status code: {}",
                resp.status().as_u16()
            )));
        }

        let result: Value = resp
            .json()
            .await
            .map_err(|e| AppError::ApiError(format!("failed to decode response: {}", e)))?;

        Ok(result)
    }

    /// Internal helper to get all approved account IDs.
    async fn get_account_ids(&self) -> Result<Vec<String>, AppError> {
        let query = r#"
            query Check {
                user {
                    id
                    accounts {
                        ... on Account {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                        ... on RothIRA {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                        ... on TraditionalIRA {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                    }
                }
            }
        "#;

        let response = self.graphql_query(FENNEL_API_URL, query, None).await?;

        let data = response
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?;

        let user = data
            .get("user")
            .and_then(|u| u.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: user field missing or not an object".to_string(),
                )
            })?;

        let accounts = user
            .get("accounts")
            .and_then(|a| a.as_array())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: accounts field missing or not an array".to_string(),
                )
            })?;

        let mut account_ids = Vec::new();
        for account in accounts {
            let status = match account.get("status").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let id = match account.get("id").and_then(|i| i.as_str()) {
                Some(i) => i,
                None => continue,
            };

            if status == "APPROVED" {
                account_ids.push(id.to_string());
            }
        }

        if account_ids.is_empty() {
            return Err(AppError::ApiError("no approved accounts found".to_string()));
        }

        Ok(account_ids)
    }

    /// Internal: refresh the auth token using the stored refresh token.
    /// Uses RwLock interior mutability so it works with &self.
    async fn refresh_token_inner(&self) -> Result<(), AppError> {
        let refresh_tok = self.refresh_token_str.read().await.clone();
        if refresh_tok.is_empty() {
            self.emit(
                "tokenExpired",
                json!([self.id.clone(), "Your session has expired. Please log in again."]),
            );
            self.auth_token.write().await.clear();
            *self.is_logged_in.write().await = false;
            return Err(AppError::AuthFailed(
                "no refresh token available, please log in again".to_string(),
            ));
        }

        let url = format!("{}/oauth/token", FENNEL_AUTH_URL);
        let payload = json!({
            "grant_type": "refresh_token",
            "client_id": FENNEL_CLIENT_ID,
            "refresh_token": refresh_tok,
            "scope": "openid profile offline_access email",
        });

        let resp = self
            .client
            .post(&url)
            .header("accept", "*/*")
            .header("content-type", "application/json")
            .header("host", "accounts.fennel.com")
            .header("user-agent", FENNEL_USER_AGENT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            self.emit(
                "tokenExpired",
                json!([self.id.clone(), "Your session has expired. Please log in again."]),
            );
            self.auth_token.write().await.clear();
            self.refresh_token_str.write().await.clear();
            *self.is_logged_in.write().await = false;
            return Err(AppError::AuthFailed(format!(
                "token refresh failed, status code: {}",
                resp.status().as_u16()
            )));
        }

        let result: Value = resp
            .json()
            .await
            .map_err(|e| AppError::ApiError(format!("failed to decode response: {}", e)))?;

        let access_token = result
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                AppError::ApiError("access token not found in refresh response".to_string())
            })?;

        *self.auth_token.write().await = access_token.to_string();

        if let Some(new_refresh) = result.get("refresh_token").and_then(|t| t.as_str()) {
            *self.refresh_token_str.write().await = new_refresh.to_string();
        }

        println!("Token refreshed successfully");
        Ok(())
    }

    /// Internal: get portfolio data for an account.
    async fn get_portfolio(&self, account_id: &str) -> Result<Value, AppError> {
        let query = r#"
            query GetPortfolioSummary($accountId: String!) {
                account(accountId: $accountId) {
                    ... on Account {
                        id
                        portfolio {
                            cash {
                                balance {
                                    canTrade
                                    canWithdraw
                                    reservedBalance
                                    settledBalance
                                    tradeBalance
                                    tradeDecrease
                                    tradeIncrease
                                }
                                currency
                            }
                            totalEquityValue
                        }
                    }
                    ... on RothIRA {
                        id
                        portfolio {
                            cash {
                                balance {
                                    canTrade
                                    canWithdraw
                                    reservedBalance
                                    settledBalance
                                    tradeBalance
                                    tradeDecrease
                                    tradeIncrease
                                }
                                currency
                            }
                            totalEquityValue
                        }
                    }
                    ... on TraditionalIRA {
                        id
                        portfolio {
                            cash {
                                balance {
                                    canTrade
                                    canWithdraw
                                    reservedBalance
                                    settledBalance
                                    tradeBalance
                                    tradeDecrease
                                    tradeIncrease
                                }
                                currency
                            }
                            totalEquityValue
                        }
                    }
                }
            }
        "#;

        let variables = json!({ "accountId": account_id });

        let response = self
            .graphql_query(FENNEL_API_URL, query, Some(variables))
            .await
            .map_err(|e| AppError::ApiError(format!("failed to get portfolio data: {}", e)))?;

        let data = response
            .get("data")
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?
            .clone();

        Ok(data)
    }

    /// Internal: get stock holdings for an account.
    async fn get_stock_holdings(&self, account_id: &str) -> Result<Value, AppError> {
        let query = r#"
            query MinimumPortfolioData($accountId: String!) {
                account(accountId: $accountId) {
                    ... on Account {
                        id
                        portfolio {
                            totalEquityValue
                            bulbs {
                                isin
                                investment {
                                    marketValue
                                    ownedShares
                                }
                                security {
                                    currentStockPrice
                                    ticker
                                    securityName
                                    securityType
                                }
                                tradeable {
                                    canSell
                                    restrictionReason
                                }
                            }
                        }
                    }
                    ... on RothIRA {
                        id
                        portfolio {
                            totalEquityValue
                            bulbs {
                                isin
                                investment {
                                    marketValue
                                    ownedShares
                                }
                                security {
                                    currentStockPrice
                                    ticker
                                    securityName
                                    securityType
                                }
                                tradeable {
                                    canSell
                                    restrictionReason
                                }
                            }
                        }
                    }
                    ... on TraditionalIRA {
                        id
                        portfolio {
                            totalEquityValue
                            bulbs {
                                isin
                                investment {
                                    marketValue
                                    ownedShares
                                }
                                security {
                                    currentStockPrice
                                    ticker
                                    securityName
                                    securityType
                                }
                                tradeable {
                                    canSell
                                    restrictionReason
                                }
                            }
                        }
                    }
                }
            }
        "#;

        let variables = json!({ "accountId": account_id });

        let response = self
            .graphql_query(FENNEL_API_URL, query, Some(variables))
            .await
            .map_err(|e| AppError::ApiError(format!("failed to get stock holdings: {}", e)))?;

        let data = response
            .get("data")
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?
            .clone();

        Ok(data)
    }

    /// Internal: check if a stock (by ISIN) is tradable in a given account.
    async fn is_tradable(&self, isin: &str, account_id: &str) -> Result<bool, AppError> {
        let query = r#"
            query GetTradeable($isin: String!, $accountId: String) {
                bulbBulb(isin: $isin) {
                    tradeable(accountId: $accountId) {
                        canBuy
                        canSell
                        restrictionReason
                    }
                }
            }
        "#;

        let variables = json!({
            "isin": isin,
            "accountId": account_id,
        });

        let response = self
            .graphql_query(FENNEL_API_URL, query, Some(variables))
            .await
            .map_err(|e| {
                AppError::ApiError(format!("failed to check if stock is tradable: {}", e))
            })?;

        let data = response
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?;

        let bulb_bulb = data
            .get("bulbBulb")
            .and_then(|b| b.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: bulbBulb field missing or not an object".to_string(),
                )
            })?;

        let tradeable = bulb_bulb
            .get("tradeable")
            .and_then(|t| t.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: tradeable field missing or not an object".to_string(),
                )
            })?;

        let can_buy = tradeable
            .get("canBuy")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: canBuy field missing or not a boolean".to_string(),
                )
            })?;

        let can_sell = tradeable
            .get("canSell")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: canSell field missing or not a boolean".to_string(),
                )
            })?;

        Ok(can_buy || can_sell)
    }

    /// Internal: place an order for a specific account, with retry logic.
    async fn place_order_for_account(
        &self,
        account_id: &str,
        ticker: &str,
        quantity: f64,
        side: &str,
        isin: &str,
    ) -> Result<Value, AppError> {
        if !*self.is_logged_in.read().await || self.auth_token.read().await.is_empty() {
            return Err(AppError::BrokerNotLoggedIn);
        }

        let query = r#"
            mutation CreateOrder(
                $order_details: OrderDetailsInput__!
                $accountId: String!
            ) {
                createOrder(
                    accountId: $accountId
                    order: $order_details
                )
            }
        "#;

        // Fennel's GraphQL schema expects quantity as an Int, so we must send
        // whole numbers as JSON integers (e.g. 1 not 1.0). Rust's serde_json
        // serializes f64 values like 1.0 as "1.0" which GraphQL rejects for Int fields.
        // Go's json.Marshal formats float64(1.0) as "1", which is why the Go version works.
        let qty_value = if quantity.fract() == 0.0 {
            json!(quantity as i64)
        } else {
            json!(quantity)
        };

        let variables = json!({
            "order_details": {
                "quantity": qty_value,
                "symbol": ticker,
                "isin": isin,
                "side": side.to_lowercase(),
                "priceRule": "market",
                "timeInForce": "day",
                "routingOption": "exchange_ats_sdp",
            },
            "accountId": account_id,
        });

        let max_retries = 3;
        let retry_delay = tokio::time::Duration::from_secs(1);
        let mut last_err: Option<AppError> = None;

        for attempt in 1..=max_retries {
            self.emit("log", json!(format!(
                "placeOrderForAccount (Attempt {}/{}): Calling API for account {}",
                attempt, max_retries, account_id
            )));

            match self
                .graphql_query(FENNEL_API_URL, query, Some(variables.clone()))
                .await
            {
                Ok(response) => {
                    // Check for GraphQL errors in the response
                    if let Some(errors) = response.get("errors").and_then(|e| e.as_array()) {
                        if !errors.is_empty() {
                            let err_msg = if let Some(first_err) =
                                errors[0].as_object().and_then(|e| e.get("message"))
                            {
                                if let Some(msg) = first_err.as_str() {
                                    format!("GraphQL error: {}", msg)
                                } else {
                                    format!(
                                        "GraphQL query returned unspecified errors: {:?}",
                                        errors
                                    )
                                }
                            } else {
                                format!("GraphQL query returned errors: {:?}", errors)
                            };

                            let log_msg = format!(
                                "placeOrderForAccount (Attempt {}/{}): GraphQL error in response: {}",
                                attempt, max_retries, err_msg
                            );
                            self.emit("log", json!(log_msg));
                            last_err = Some(AppError::ApiError(err_msg));
                            // GraphQL errors are not retried -- break immediately
                            break;
                        }
                    }

                    // Validate response structure
                    match response.get("data").and_then(|d| d.as_object()) {
                        Some(data) => {
                            if data.contains_key("createOrder") {
                                let create_order_val = serde_json::to_string(
                                    &data.get("createOrder").unwrap_or(&json!(null))
                                ).unwrap_or_default();
                                self.emit("log", json!(format!(
                                    "placeOrderForAccount: Success. createOrder = {}",
                                    create_order_val
                                )));
                                return Ok(json!(data));
                            } else {
                                let err_msg = "failed to place order (API response structure unexpected, 'createOrder' field missing)".to_string();
                                let log_msg = format!(
                                    "placeOrderForAccount (Attempt {}/{}): {}. Response: {:?}",
                                    attempt, max_retries, err_msg, response
                                );
                                self.emit("log", json!(log_msg));
                                last_err = Some(AppError::ApiError(err_msg));
                            }
                        }
                        None => {
                            let err_msg =
                                "invalid response format: data field missing or not an object"
                                    .to_string();
                            let log_msg = format!(
                                "placeOrderForAccount (Attempt {}/{}): {}",
                                attempt, max_retries, err_msg
                            );
                            self.emit("log", json!(log_msg));
                            last_err = Some(AppError::ApiError(err_msg));
                        }
                    }
                }
                Err(e) => {
                    let log_msg = format!(
                        "placeOrderForAccount (Attempt {}/{}): graphqlQuery call failed: {}",
                        attempt, max_retries, e
                    );
                    self.emit("log", json!(log_msg));
                    last_err = Some(e);
                }
            }

            // Wait before retrying (unless this was the last attempt)
            if last_err.is_some() && attempt < max_retries {
                let log_msg = format!(
                    "placeOrderForAccount (Attempt {}/{}): Waiting {:.1} seconds before retrying...",
                    attempt,
                    max_retries,
                    retry_delay.as_secs_f64()
                );
                self.emit("log", json!(log_msg));
                tokio::time::sleep(retry_delay).await;
            }
        }

        Err(last_err.unwrap_or_else(|| {
            AppError::ApiError(format!("failed after {} attempts", max_retries))
        }))
    }
}

/// Extract a numeric suffix from account names like "Account 3".
fn extract_account_number(name: &str) -> Option<i64> {
    if let Some(num_str) = name.strip_prefix("Account ") {
        num_str.trim().parse::<i64>().ok()
    } else {
        None
    }
}

#[async_trait]
impl Broker for FennelBroker {
    fn get_type(&self) -> BrokerType {
        BrokerType::Fennel
    }

    fn get_name(&self) -> &str {
        "Fennel"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        self.email = email.to_string();

        let url = format!("{}/passwordless/start", FENNEL_AUTH_URL);
        let payload = json!({
            "email": email,
            "client_id": FENNEL_CLIENT_ID,
            "connection": "email",
            "send": "code",
        });

        let resp = self
            .client
            .post(&url)
            .header("accept", "*/*")
            .header("content-type", "application/json")
            .header("host", "accounts.fennel.com")
            .header("user-agent", FENNEL_USER_AGENT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        println!("2FA initiation response status: {}", resp.status());

        if !resp.status().is_success() {
            return Err(AppError::AuthFailed(format!(
                "failed to start 2FA, status code: {}",
                resp.status().as_u16()
            )));
        }

        let result: Value = resp
            .json()
            .await
            .map_err(|e| AppError::ApiError(format!("failed to decode response: {}", e)))?;

        println!("2FA initiation response: {:?}", result);
        println!("2FA initiation request sent for email: {}", email);
        Ok(())
    }

    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        println!("login: Function started for email: {}", email);

        let url = format!("{}/oauth/token", FENNEL_AUTH_URL);
        let payload = json!({
            "grant_type": "http://auth0.com/oauth/grant-type/passwordless/otp",
            "client_id": FENNEL_CLIENT_ID,
            "otp": code,
            "username": email,
            "scope": "openid profile offline_access email",
            "audience": FENNEL_AUDIENCE,
            "realm": "email",
        });

        println!("login: Creating HTTP request to {}", url);

        let resp = self
            .client
            .post(&url)
            .header("accept", "*/*")
            .header("content-type", "application/json")
            .header("host", "accounts.fennel.com")
            .header("user-agent", FENNEL_USER_AGENT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                println!("login: Error sending request: {}", e);
                AppError::NetworkError(e.to_string())
            })?;

        println!("login: Received response status: {}", resp.status());

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            println!("login: Login failed, status code: {}", status);
            println!("login: Error response body: {}", body);
            return Err(AppError::AuthFailed(format!(
                "login failed, status code: {}",
                status
            )));
        }

        println!("login: Decoding JSON response...");

        let result: Value = resp.json().await.map_err(|e| {
            println!("login: Failed to decode response: {}", e);
            AppError::ApiError(format!("failed to decode response: {}", e))
        })?;

        println!("login: Decoded login response: {:?}", result);

        let access_token = result
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                println!("login: Access token not found in response.");
                AppError::AuthFailed("access token not found in response".to_string())
            })?;

        if let Some(refresh_token) = result.get("refresh_token").and_then(|t| t.as_str()) {
            *self.refresh_token_str.write().await = refresh_token.to_string();
            println!("login: Stored refresh token.");
        }

        *self.auth_token.write().await = access_token.to_string();
        self.email = email.to_string();
        self.login_time = Some(Utc::now());
        *self.is_logged_in.write().await = true;
        println!("login: Updated broker state (token, email, loginTime, isLoggedIn=true).");

        let token_preview = if access_token.len() > 10 {
            format!("{}...", &access_token[..10])
        } else {
            access_token.to_string()
        };
        println!("login: Login successful, token received: {}", token_preview);
        println!("login: Function finished successfully.");

        Ok(())
    }

    async fn login_with_stored_credentials(
        &mut self,
        creds: &StoredCredentials,
    ) -> Result<(), AppError> {
        println!(
            "LoginWithStoredCredentials: Attempting to restore session for {}",
            creds.email
        );

        // Set the stored credentials
        *self.auth_token.write().await = creds.access_token.clone();
        *self.refresh_token_str.write().await = creds.refresh_token.clone();
        self.email = creds.email.clone();
        *self.is_logged_in.write().await = true;
        self.login_time = Some(Utc::now());

        // Try to validate the token by making an API call
        match self.get_account_ids().await {
            Ok(_) => {}
            Err(_) => {
                println!(
                    "LoginWithStoredCredentials: Token validation failed, attempting refresh..."
                );

                // Token is invalid, try to refresh
                if let Err(refresh_err) = self.refresh_token_inner().await {
                    // Refresh failed, clear state
                    self.auth_token.write().await.clear();
                    self.refresh_token_str.write().await.clear();
                    *self.is_logged_in.write().await = false;
                    return Err(AppError::AuthFailed(format!(
                        "failed to restore session: {}",
                        refresh_err
                    )));
                }

                // Verify the refreshed token works
                if let Err(err) = self.get_account_ids().await {
                    self.auth_token.write().await.clear();
                    self.refresh_token_str.write().await.clear();
                    *self.is_logged_in.write().await = false;
                    return Err(AppError::AuthFailed(format!(
                        "token refresh succeeded but validation still failed: {}",
                        err
                    )));
                }
            }
        }

        println!(
            "LoginWithStoredCredentials: Session restored successfully for {}",
            self.email
        );
        Ok(())
    }

    async fn refresh_token(&mut self) -> Result<(), AppError> {
        self.refresh_token_inner().await
    }

    fn logout(&mut self) {
        if let Ok(mut t) = self.auth_token.try_write() { t.clear(); }
        if let Ok(mut t) = self.refresh_token_str.try_write() { t.clear(); }
        self.email.clear();
        if let Ok(mut t) = self.is_logged_in.try_write() { *t = false; }
    }

    fn is_logged_in(&self) -> bool {
        let logged_in = self.is_logged_in.try_read().map(|v| *v).unwrap_or(false);
        let has_token = self.auth_token.try_read().map(|v| !v.is_empty()).unwrap_or(false);
        logged_in && has_token
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
        let logged_in = *self.is_logged_in.read().await;
        let token_empty = self.auth_token.read().await.is_empty();
        if !logged_in || token_empty {
            return false;
        }

        match self.get_account_ids().await {
            Ok(_) => true,
            Err(e) => {
                println!("Token validation failed: {}", e);
                self.auth_token.write().await.clear();
                self.refresh_token_str.write().await.clear();
                self.email.clear();
                *self.is_logged_in.write().await = false;
                false
            }
        }
    }

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        let logged_in = self.is_logged_in.try_read().map(|v| *v).unwrap_or(false);
        if !logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }

        let auth_token = self.auth_token.try_read().map(|v| v.clone()).unwrap_or_default();
        let refresh_token = self.refresh_token_str.try_read().map(|v| v.clone()).unwrap_or_default();

        Ok(StoredCredentials {
            broker_type: BrokerType::Fennel.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: auth_token,
            refresh_token,
            token_type: String::new(),
            device_token: String::new(),
        })
    }

    async fn get_accounts(&self) -> Result<Vec<Value>, AppError> {
        if self.auth_token.read().await.is_empty() {
            return Err(AppError::BrokerNotLoggedIn);
        }

        let query = r#"
            query Check {
                user {
                    id
                    accounts {
                        ... on Account {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                        ... on RothIRA {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                        ... on TraditionalIRA {
                            name
                            id
                            created
                            isPrimary
                            status
                        }
                    }
                }
            }
        "#;

        let response = self
            .graphql_query(FENNEL_API_URL, query, None)
            .await
            .map_err(|e| AppError::ApiError(format!("failed to get accounts: {}", e)))?;

        let data = response
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?;

        let user = data
            .get("user")
            .and_then(|u| u.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: user field missing or not an object".to_string(),
                )
            })?;

        let accounts = user
            .get("accounts")
            .and_then(|a| a.as_array())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: accounts field missing or not an array".to_string(),
                )
            })?;

        // Filter to only return APPROVED accounts
        let result: Vec<Value> = accounts
            .iter()
            .filter(|acc| {
                acc.get("status")
                    .and_then(|s| s.as_str())
                    .map(|s| s == "APPROVED")
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        Ok(result)
    }

    async fn get_account_details(&self, account_id: &str) -> Result<Value, AppError> {
        if self.auth_token.read().await.is_empty() {
            return Err(AppError::BrokerNotLoggedIn);
        }

        if account_id.is_empty() {
            return Err(AppError::ApiError(
                "account ID cannot be empty".to_string(),
            ));
        }

        let portfolio = self
            .get_portfolio(account_id)
            .await
            .map_err(|e| AppError::ApiError(format!("failed to get portfolio data: {}", e)))?;

        let holdings = self
            .get_stock_holdings(account_id)
            .await
            .map_err(|e| AppError::ApiError(format!("failed to get stock holdings: {}", e)))?;

        Ok(json!({
            "portfolio": portfolio,
            "holdings": holdings,
        }))
    }

    async fn get_account_holdings(&self, account_id: &str) -> Result<Vec<Value>, AppError> {
        if !*self.is_logged_in.read().await {
            return Err(AppError::BrokerNotLoggedIn);
        }

        if account_id.is_empty() {
            return Err(AppError::ApiError(
                "account ID cannot be empty".to_string(),
            ));
        }

        let response = self.get_stock_holdings(account_id).await?;

        let account_data = response
            .get("account")
            .and_then(|a| a.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: account field missing or not an object".to_string(),
                )
            })?;

        let portfolio = account_data
            .get("portfolio")
            .and_then(|p| p.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: portfolio field missing or not an object".to_string(),
                )
            })?;

        let bulbs = match portfolio.get("bulbs").and_then(|b| b.as_array()) {
            Some(b) => b,
            None => return Ok(Vec::new()),
        };

        let mut result = Vec::with_capacity(bulbs.len());
        for bulb in bulbs {
            let security = match bulb.get("security").and_then(|s| s.as_object()) {
                Some(s) => s,
                None => continue,
            };

            let investment = match bulb.get("investment").and_then(|i| i.as_object()) {
                Some(i) => i,
                None => continue,
            };

            let isin = match bulb.get("isin").and_then(|i| i.as_str()) {
                Some(i) => i,
                None => continue,
            };

            let can_sell = bulb
                .get("tradeable")
                .and_then(|t| t.as_object())
                .and_then(|t| t.get("canSell"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let holding = json!({
                "ticker": security.get("ticker"),
                "name": security.get("securityName"),
                "price": security.get("currentStockPrice"),
                "shares": investment.get("ownedShares"),
                "marketValue": investment.get("marketValue"),
                "isin": isin,
                "type": security.get("securityType"),
                "canSell": can_sell,
            });

            result.push(holding);
        }

        Ok(result)
    }

    async fn get_account_cash(&self, account_id: &str) -> Result<Value, AppError> {
        if !*self.is_logged_in.read().await {
            return Err(AppError::BrokerNotLoggedIn);
        }

        if account_id.is_empty() {
            return Err(AppError::ApiError(
                "account ID cannot be empty".to_string(),
            ));
        }

        let response = self.get_portfolio(account_id).await?;

        let account = response
            .get("account")
            .and_then(|a| a.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: account field missing or not an object".to_string(),
                )
            })?;

        let portfolio = account
            .get("portfolio")
            .and_then(|p| p.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: portfolio field missing or not an object".to_string(),
                )
            })?;

        let cash = portfolio.get("cash").ok_or_else(|| {
            AppError::ApiError(
                "invalid response format: cash field missing or not an object".to_string(),
            )
        })?;

        Ok(cash.clone())
    }

    async fn place_order(&self, ticker: &str, side: &str, shares: f64, account: &str, sell_max: bool) -> String {
        if !*self.is_logged_in.read().await || self.auth_token.read().await.is_empty() {
            return "Error: Not logged in".to_string();
        }

        self.emit(
            "log",
            json!(format!(
                "Received request to place {} order for {} shares of {} in account: {}",
                side, shares, ticker, account
            )),
        );

        // Check if market is open
        let market_open = match self.is_market_open().await {
            Ok(open) => open,
            Err(e) => {
                return format!("Error checking market status: {}", e);
            }
        };
        if !market_open {
            return "Error: Market is closed. Cannot place order.".to_string();
        }

        // Get stock info and ISIN
        let stock_info = match self.get_stock_quote(ticker).await {
            Ok(info) => info,
            Err(e) => {
                return format!("Error getting stock quote for {}: {}", ticker, e);
            }
        };

        let isin = match stock_info.get("isin").and_then(|i| i.as_str()) {
            Some(i) if !i.is_empty() => i.to_string(),
            _ => {
                return format!("Error: Could not find ISIN for ticker {}", ticker);
            }
        };

        // Get all accounts to determine target(s)
        let all_accounts = match self.get_accounts().await {
            Ok(accs) => accs,
            Err(e) => {
                return format!("Error fetching accounts: {}", e);
            }
        };
        if all_accounts.is_empty() {
            return "Error: No accounts found.".to_string();
        }

        let mut target_accounts: Vec<Value> = if account == "All accounts" {
            all_accounts.clone()
        } else {
            // Parse comma-separated account names
            let requested_names: Vec<&str> = account.split(',').map(|s| s.trim()).collect();
            let mut matched = Vec::new();
            for req_name in &requested_names {
                if req_name.is_empty() {
                    continue;
                }
                for acc in &all_accounts {
                    if let Some(acc_name) = acc.get("name").and_then(|n| n.as_str()) {
                        if acc_name == *req_name {
                            matched.push(acc.clone());
                            break;
                        }
                    }
                }
            }
            if matched.is_empty() {
                return format!(
                    "Error: Could not find any accounts matching '{}'",
                    account
                );
            }
            matched
        };

        if target_accounts.is_empty() {
            return "Error: No matching accounts found to place order in.".to_string();
        }

        // Sort accounts: primary first, then APPROVED first, then by account number, then by name
        target_accounts.sort_by(|a, b| {
            let is_primary_a = a
                .get("isPrimary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_primary_b = b
                .get("isPrimary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_primary_a != is_primary_b {
                // true (primary) should come first
                return is_primary_b.cmp(&is_primary_a);
            }

            let status_a = a
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let status_b = b
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if status_a != status_b {
                if status_a == "APPROVED" {
                    return std::cmp::Ordering::Less;
                }
                if status_b == "APPROVED" {
                    return std::cmp::Ordering::Greater;
                }
                return status_a.cmp(status_b);
            }

            let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or_else(|| {
                // Fallback not possible with str reference, handled below
                ""
            });
            let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");

            let num_a = extract_account_number(name_a);
            let num_b = extract_account_number(name_b);

            match (num_a, num_b) {
                (Some(na), Some(nb)) => na.cmp(&nb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => name_a.cmp(name_b),
            }
        });

        let target_account_names: Vec<String> = target_accounts
            .iter()
            .enumerate()
            .map(|(i, acc)| {
                if let Some(name) = acc.get("name").and_then(|n| n.as_str()) {
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
                if let Some(id) = acc.get("id").and_then(|n| n.as_str()) {
                    if id.len() >= 6 {
                        return format!("Account {}...", &id[..6]);
                    }
                    return format!("Account {}", id);
                }
                format!("Account {}", i + 1)
            })
            .collect();

        self.emit(
            "log",
            json!(format!(
                "Targeting account(s) in order: {:?}",
                target_account_names
            )),
        );

        let mut success_count = 0;
        let mut failure_count = 0;
        let mut error_messages: Vec<String> = Vec::new();

        for (i, current_account) in target_accounts.iter().enumerate() {
            let account_id = match current_account.get("id").and_then(|id| id.as_str()) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => {
                    self.emit(
                        "log",
                        json!(format!(
                            "Skipping account at index {}: Missing or invalid ID",
                            i
                        )),
                    );
                    failure_count += 1;
                    continue;
                }
            };

            let account_log_prefix =
                if let Some(name) = current_account.get("name").and_then(|n| n.as_str()) {
                    if !name.is_empty() {
                        name.to_string()
                    } else {
                        format!("Account {}", account_id)
                    }
                } else {
                    format!("Account {}", account_id)
                };

            // Check if the stock is tradable in this account
            match self.is_tradable(&isin, &account_id).await {
                Ok(tradable) => {
                    if !tradable {
                        let info_msg = format!(
                            "{}: Stock {} is not tradable in this account. Order SKIPPED.",
                            account_log_prefix, ticker
                        );
                        self.emit("log", json!(info_msg));
                        continue;
                    }
                }
                Err(e) => {
                    let err_msg = format!(
                        "{}: Failed to check if {} is tradable: {}",
                        account_log_prefix, ticker, e
                    );
                    self.emit("log", json!(err_msg));
                    error_messages.push(err_msg);
                    failure_count += 1;
                    continue;
                }
            }

            // For sell orders, look up actual holdings in this account and cap quantity
            let order_shares = if side.eq_ignore_ascii_case("sell") {
                match self.get_account_holdings(&account_id).await {
                    Ok(holdings) => {
                        let owned = holdings.iter().find_map(|h| {
                            let h_ticker = h.get("ticker").and_then(|t| t.as_str()).unwrap_or("");
                            if h_ticker.eq_ignore_ascii_case(ticker) {
                                // shares can be a number or string from the API
                                if let Some(n) = h.get("shares").and_then(|v| v.as_f64()) {
                                    Some(n)
                                } else if let Some(s) = h.get("shares").and_then(|v| v.as_str()) {
                                    s.parse::<f64>().ok()
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }).unwrap_or(0.0);

                        if owned <= 0.0 {
                            self.emit("log", json!(format!(
                                "{}: No shares of {} in this account, skipping.",
                                account_log_prefix, ticker
                            )));
                            continue;
                        }

                        if sell_max {
                            self.emit("log", json!(format!(
                                "{}: Selling all {} shares held.",
                                account_log_prefix, owned
                            )));
                            owned
                        } else {
                            let capped = shares.min(owned);
                            if (capped - shares).abs() > 0.001 {
                                self.emit("log", json!(format!(
                                    "{}: Requested {} shares but account holds {}, selling {}.",
                                    account_log_prefix, shares, owned, capped
                                )));
                            }
                            capped
                        }
                    }
                    Err(e) => {
                        if sell_max {
                            self.emit("log", json!(format!(
                                "{}: Could not look up holdings: {}. Skipping account.",
                                account_log_prefix, e
                            )));
                            continue;
                        }
                        self.emit("log", json!(format!(
                            "{}: Could not look up holdings: {}. Using requested shares.",
                            account_log_prefix, e
                        )));
                        shares
                    }
                }
            } else {
                shares
            };

            // Add delay between accounts for multi-account orders
            if target_accounts.len() > 1 && i > 0 {
                let delay_seconds = rand::thread_rng().gen_range(15..=20);
                self.emit(
                    "log",
                    json!(format!(
                        "Waiting {} seconds before placing order for next account...",
                        delay_seconds
                    )),
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;
            }

            self.emit(
                "log",
                json!(format!(
                    "{}: Placing {} order for {} shares of {}...",
                    account_log_prefix, side, order_shares, ticker
                )),
            );

            match self
                .place_order_for_account(&account_id, ticker, order_shares, side, &isin)
                .await
            {
                Ok(_) => {
                    let success_msg =
                        format!("{}: Order placed successfully.", account_log_prefix);
                    self.emit("log", json!(success_msg));
                    println!("{}", success_msg);
                    success_count += 1;
                }
                Err(e) => {
                    let err_msg =
                        format!("{}: Failed to place order: {}", account_log_prefix, e);
                    self.emit("log", json!(err_msg));
                    println!("{}", err_msg);
                    error_messages.push(err_msg);
                    failure_count += 1;
                }
            }
        }

        // Build summary
        let mut summary = String::new();
        if success_count > 0 {
            summary.push_str(&format!(
                "Successfully placed orders in {} account(s). ",
                success_count
            ));
        }
        if failure_count > 0 {
            summary.push_str(&format!(
                "Failed to place orders in {} account(s). ",
                failure_count
            ));
        }
        if success_count == 0 && failure_count == 0 {
            summary.push_str(
                "No orders were placed (e.g., stock not tradable in selected accounts).",
            );
        } else if failure_count > 0 {
            summary.push_str("Check log for details.");
        }

        if failure_count > 0 {
            format!("Error: {}", summary)
        } else {
            summary
        }
    }

    async fn is_market_open(&self) -> Result<bool, AppError> {
        let query = r#"
            query MarketHours {
                securityMarketInfo {
                    isOpen
                }
            }
        "#;

        let response = self
            .graphql_query(FENNEL_API_URL, query, None)
            .await
            .map_err(|e| AppError::ApiError(format!("failed to check market hours: {}", e)))?;

        let data = response
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?;

        let security_market_info = data
            .get("securityMarketInfo")
            .and_then(|s| s.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: securityMarketInfo field missing or not an object"
                        .to_string(),
                )
            })?;

        let is_open = security_market_info
            .get("isOpen")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: isOpen field missing or not a boolean".to_string(),
                )
            })?;

        Ok(is_open)
    }

    async fn get_stock_quote(&self, ticker: &str) -> Result<Value, AppError> {
        if ticker.is_empty() {
            return Err(AppError::ApiError("ticker cannot be empty".to_string()));
        }

        let query = r#"
            query Search($query: String!, $count: Int) {
                searchSearch {
                    searchSecurities(query: $query, count: $count) {
                        isin
                        security {
                            currentStockPrice
                            ticker
                        }
                    }
                }
            }
        "#;

        let variables = json!({
            "query": ticker,
            "count": 5,
        });

        let response = self
            .graphql_query(FENNEL_API_URL, query, Some(variables))
            .await
            .map_err(|e| AppError::ApiError(format!("failed to query stock info: {}", e)))?;

        println!("Stock quote response: {:?}", response);

        let data = response
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: data field missing or not an object".to_string(),
                )
            })?;

        let search_search = data
            .get("searchSearch")
            .and_then(|s| s.as_object())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: searchSearch field missing or not an object"
                        .to_string(),
                )
            })?;

        let search_securities = search_search
            .get("searchSecurities")
            .and_then(|s| s.as_array())
            .ok_or_else(|| {
                AppError::ApiError(
                    "invalid response format: searchSecurities field missing or not an array"
                        .to_string(),
                )
            })?;

        println!(
            "Found {} securities for ticker {}",
            search_securities.len(),
            ticker
        );

        for sec in search_securities {
            let security_obj = match sec.get("security").and_then(|s| s.as_object()) {
                Some(s) => s,
                None => continue,
            };

            let security_ticker = match security_obj.get("ticker").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => continue,
            };

            println!(
                "Comparing ticker: {} with input: {}",
                security_ticker, ticker
            );

            if security_ticker.eq_ignore_ascii_case(ticker) {
                println!("Found matching security for ticker {}", ticker);
                return Ok(sec.clone());
            }
        }

        Err(AppError::ApiError(format!(
            "no exact match found for ticker {}",
            ticker
        )))
    }

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}
