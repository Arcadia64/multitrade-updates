use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::broker::{Broker, BrokerType, EventEmitter};
use crate::credentials::StoredCredentials;
use crate::error::AppError;

const ROBINHOOD_API_URL: &str = "https://api.robinhood.com";
const ROBINHOOD_CLIENT_ID: &str = "c82SH0WZOsabOXGP2sxqcj34FxkvfnWRZBKlBjFS";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct RobinhoodAccount {
    account_number: String,
    url: String,
    portfolio: String,
    positions: String,
    #[serde(rename = "type")]
    account_type: String,
    buying_power: String,
    cash: String,
    portfolio_cash: String,
}

/// Helper struct for resolved instrument details.
struct InstrumentInfo {
    symbol: String,
    name: String,
    price: String,
    tradable: bool,
}

/// Token fields use RwLock for interior mutability so that &self methods
/// can auto-refresh tokens on 401.
pub struct RobinhoodBroker {
    id: String,
    auth_token: RwLock<String>,
    token_type: RwLock<String>,
    refresh_token_value: RwLock<String>,
    is_logged_in_flag: RwLock<bool>,
    device_token: String,
    email: String,
    login_time: Option<DateTime<Utc>>,
    event_emitter: Option<EventEmitter>,
    accounts: Vec<RobinhoodAccount>,
    mfa_required: bool,
    mfa_type: String,
    challenge_id: String,
    verification_id: String,
    machine_id: String,
    sheriff_challenge_id: String,
    pending_username: String,
    pending_password: String,
    client: Client,
}

impl RobinhoodBroker {
    pub fn new(id: &str) -> Self {
        let actual_id = if id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            id.to_string()
        };

        Self {
            id: actual_id,
            auth_token: RwLock::new(String::new()),
            token_type: RwLock::new(String::new()),
            refresh_token_value: RwLock::new(String::new()),
            is_logged_in_flag: RwLock::new(false),
            device_token: uuid::Uuid::new_v4().to_string(),
            email: String::new(),
            login_time: None,
            event_emitter: None,
            accounts: Vec::new(),
            mfa_required: false,
            mfa_type: String::new(),
            challenge_id: String::new(),
            verification_id: String::new(),
            machine_id: String::new(),
            sheriff_challenge_id: String::new(),
            pending_username: String::new(),
            pending_password: String::new(),
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Sets the device token (used to preserve identity across reauth).
    pub fn set_device_token(&mut self, token: &str) {
        if !token.is_empty() {
            self.device_token = token.to_string();
        }
    }

    /// Emits an event through the event emitter if one is set.
    fn emit(&self, event_name: &str, data: serde_json::Value) {
        if let Some(ref emitter) = self.event_emitter {
            if event_name == "log" {
                let message = match data {
                    serde_json::Value::String(msg) => msg,
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

    /// Returns the Authorization header value using the stored token type.
    #[allow(dead_code)]
    fn auth_header_value_sync(&self) -> String {
        let tt = self.token_type.try_read().map(|v| v.clone()).unwrap_or_default();
        let token = self.auth_token.try_read().map(|v| v.clone()).unwrap_or_default();
        let token_type = if tt.is_empty() { "Bearer".to_string() } else { tt };
        format!("{} {}", token_type, token)
    }

    async fn auth_header_value(&self) -> String {
        let tt = self.token_type.read().await.clone();
        let token = self.auth_token.read().await.clone();
        let token_type = if tt.is_empty() { "Bearer".to_string() } else { tt };
        format!("{} {}", token_type, token)
    }

    /// Applies the standard Robinhood session headers matching robin_stocks.
    fn apply_standard_headers(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        builder
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=1")
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .header("X-Robinhood-API-Version", "1.431.4")
            .header("Connection", "keep-alive")
            .header("User-Agent", "*")
    }

    /// Attempts to authenticate with Robinhood using OAuth2.
    async fn attempt_login(
        &mut self,
        username: &str,
        password: &str,
        mfa_code: &str,
    ) -> Result<(), AppError> {
        let auth_url = format!("{}/oauth2/token/", ROBINHOOD_API_URL);

        let mut params: Vec<(&str, String)> = vec![
            ("client_id", ROBINHOOD_CLIENT_ID.to_string()),
            ("expires_in", "86400".to_string()),
            ("grant_type", "password".to_string()),
            ("password", password.to_string()),
            ("scope", "internal".to_string()),
            ("username", username.to_string()),
            ("device_token", self.device_token.clone()),
            ("try_passkeys", "false".to_string()),
            ("token_request_path", "/login".to_string()),
            ("create_read_only_secondary_token", "true".to_string()),
        ];

        if !mfa_code.is_empty() {
            params.push(("mfa_code", mfa_code.to_string()));
        }

        if !self.challenge_id.is_empty() && !mfa_code.is_empty() {
            params.push(("challenge_id", self.challenge_id.clone()));
        }

        if !self.verification_id.is_empty() && !mfa_code.is_empty() {
            params.push(("verification_id", self.verification_id.clone()));
        }

        let form_body = build_form_body(&params);

        let builder = self.client.post(&auth_url).body(form_body);
        let builder = self.apply_standard_headers(builder);

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(format!("login request failed: {}", e)))?;

        let status = resp.status().as_u16();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(format!("failed to read response: {}", e)))?;

        self.emit(
            "log",
            json!(format!(
                "Robinhood auth response status: {}, body length: {}",
                status,
                body_bytes.len()
            )),
        );

        // Log response keys for debugging
        if !body_bytes.is_empty() {
            if let Ok(peek) = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&body_bytes) {
                let keys: Vec<&String> = peek.keys().collect();
                self.emit(
                    "log",
                    json!(format!("Robinhood response keys: {:?}", keys)),
                );
            }
        }

        if body_bytes.is_empty() {
            return Err(AppError::ApiError(format!(
                "empty response from Robinhood API (status {})",
                status
            )));
        }

        let result: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&body_bytes).map_err(|e| {
                AppError::ApiError(format!(
                    "failed to parse response: {} (body: {})",
                    e,
                    String::from_utf8_lossy(&body_bytes)
                ))
            })?;

        // Check for verification_workflow FIRST (new Robinhood MFA via pathfinder).
        // This must be checked before the legacy fields because Robinhood may return
        // both mfa_required and verification_workflow in the same response, and only
        // the pathfinder flow actually triggers SMS/email delivery.
        if let Some(workflow) = result.get("verification_workflow").and_then(|v| v.as_object()) {
            let workflow_status = workflow
                .get("workflow_status")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Skip if workflow is already approved — just continue to access_token check
            if workflow_status != "workflow_status_approved" {
                if let Some(workflow_id) = workflow.get("id").and_then(|v| v.as_str()) {
                    if !workflow_id.is_empty() {
                        self.verification_id = workflow_id.to_string();
                        self.emit(
                            "log",
                            json!(format!(
                                "Verification workflow required: id={} status={}",
                                workflow_id, workflow_status
                            )),
                        );

                        let challenge_type = Box::pin(self
                            .initiate_verification_workflow(workflow_id))
                            .await
                            .map_err(|e| {
                                AppError::AuthFailed(format!("verification workflow failed: {}", e))
                            })?;

                        // If prompt was approved, login already completed via confirm_workflow_and_login
                        if challenge_type == "prompt_approved" {
                            return Ok(());
                        }

                        self.mfa_required = true;
                        self.mfa_type = challenge_type.clone();
                        return Err(AppError::MfaRequired(challenge_type));
                    }
                }
            } else {
                self.emit("log", json!("Verification workflow already approved, checking for access token"));
            }
        }

        // Legacy: Check for MFA challenge
        if let Some(challenge) = result.get("challenge").and_then(|v| v.as_object()) {
            let challenge_id = challenge
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let challenge_type = challenge
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            self.challenge_id = challenge_id;
            self.mfa_required = true;
            self.mfa_type = challenge_type.clone();
            self.emit(
                "log",
                json!(format!("MFA challenge required: type={}", challenge_type)),
            );
            return Err(AppError::MfaRequired(challenge_type));
        }

        // Legacy: Check if MFA is required
        if let Some(mfa_required) = result.get("mfa_required").and_then(|v| v.as_bool()) {
            if mfa_required {
                self.mfa_required = true;
                if let Some(mfa_type) = result.get("mfa_type").and_then(|v| v.as_str()) {
                    self.mfa_type = mfa_type.to_string();
                }
                self.emit(
                    "log",
                    json!(format!("MFA required: type={}", self.mfa_type)),
                );
                return Err(AppError::MfaRequired(self.mfa_type.clone()));
            }
        }

        // Check for access token (OAuth2 success)
        if let Some(access_token) = result.get("access_token").and_then(|v| v.as_str()) {
            if !access_token.is_empty() {
                *self.auth_token.write().await = access_token.to_string();
                if let Some(tt) = result.get("token_type").and_then(|v| v.as_str()) {
                    *self.token_type.write().await = tt.to_string();
                }
                if let Some(rt) = result.get("refresh_token").and_then(|v| v.as_str()) {
                    *self.refresh_token_value.write().await = rt.to_string();
                }
                *self.is_logged_in_flag.write().await = true;
                let at_len = self.auth_token.read().await.len();
                let rt_len = self.refresh_token_value.read().await.len();
                let tt_val = self.token_type.read().await.clone();
                self.emit(
                    "log",
                    json!(format!(
                        "Got access_token (len={}), refresh_token (len={}), token_type={}",
                        at_len,
                        rt_len,
                        tt_val
                    )),
                );
                self.login_time = Some(Utc::now());
                self.mfa_required = false;
                self.challenge_id.clear();
                self.verification_id.clear();
                self.machine_id.clear();
                self.sheriff_challenge_id.clear();
                self.pending_username.clear();
                self.pending_password.clear();

                // Load accounts (retry once after a brief delay if first attempt fails)
                if let Err(e) = self.load_accounts().await {
                    self.emit(
                        "log",
                        json!(format!("Warning: Failed to load accounts (attempt 1): {}", e)),
                    );
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                    if let Err(e2) = self.load_accounts().await {
                        self.emit(
                            "log",
                            json!(format!("Warning: Failed to load accounts (attempt 2): {}", e2)),
                        );
                    }
                }

                self.emit("log", json!("Robinhood login successful"));
                return Ok(());
            }
        }

        // Check for error
        if let Some(detail) = result.get("detail").and_then(|v| v.as_str()) {
            return Err(AppError::AuthFailed(format!("login failed: {}", detail)));
        }
        if let Some(error_msg) = result.get("error").and_then(|v| v.as_str()) {
            let error_desc = result
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(AppError::AuthFailed(format!(
                "login failed: {} - {}",
                error_msg, error_desc
            )));
        }

        Err(AppError::AuthFailed(format!(
            "login failed: unexpected response: {}",
            String::from_utf8_lossy(&body_bytes)
        )))
    }

    /// Submits the MFA code.
    async fn submit_mfa(&mut self, mfa_code: &str) -> Result<(), AppError> {
        if self.pending_username.is_empty() || self.pending_password.is_empty() {
            return Err(AppError::AuthFailed(
                "no pending login to complete MFA".to_string(),
            ));
        }

        // If we have a sheriff challenge (verification_workflow), use that flow
        if !self.sheriff_challenge_id.is_empty() {
            return self.submit_verification_mfa(mfa_code).await;
        }

        // Legacy MFA: pass mfa_code in the login retry
        let username = self.pending_username.clone();
        let password = self.pending_password.clone();
        self.attempt_login(&username, &password, mfa_code).await?;

        self.mfa_required = false;
        Ok(())
    }

    /// Makes an unauthenticated request used during the verification workflow.
    /// If `json_body` is true, sends as JSON; otherwise sends as form-encoded.
    async fn make_pathfinder_request(
        &self,
        method: &str,
        url: &str,
        payload: Option<PathfinderPayload>,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let builder = match method {
            "POST" => {
                let mut b = self.client.post(url);
                if let Some(ref p) = payload {
                    match p {
                        PathfinderPayload::Json(val) => {
                            b = b
                                .header("Accept", "*/*")
                                .header("Accept-Language", "en-US,en;q=1")
                                .header("Content-Type", "application/json")
                                .header("X-Robinhood-API-Version", "1.431.4")
                                .header("Connection", "keep-alive")
                                .header("User-Agent", "*")
                                .body(serde_json::to_string(val).unwrap_or_default());
                        }
                        PathfinderPayload::Form(params) => {
                            let form_body = build_form_body(params);
                            b = self.apply_standard_headers(b);
                            b = b.body(form_body);
                        }
                    }
                } else {
                    b = self.apply_standard_headers(b);
                }
                b
            }
            _ => {
                let b = self.client.get(url);
                self.apply_standard_headers(b)
            }
        };

        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        let status = resp.status().as_u16();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        if body_bytes.is_empty() {
            return Err(AppError::ApiError(format!(
                "empty response (status {})",
                status
            )));
        }

        let result: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&body_bytes).map_err(|e| {
                AppError::ApiError(format!(
                    "failed to parse response: {} (body: {})",
                    e,
                    String::from_utf8_lossy(&body_bytes)
                ))
            })?;

        Ok(result)
    }

    /// Initiates the pathfinder flow that sends the SMS/email.
    /// Returns the challenge type (e.g. "sms", "email", "prompt").
    async fn initiate_verification_workflow(
        &mut self,
        workflow_id: &str,
    ) -> Result<String, AppError> {
        // Step 1: POST /pathfinder/user_machine/ to create a machine entry
        let machine_url = format!("{}/pathfinder/user_machine/", ROBINHOOD_API_URL);
        let machine_payload = json!({
            "device_id": self.device_token,
            "flow": "suv",
            "input": {"workflow_id": workflow_id},
        });

        let machine_data = self
            .make_pathfinder_request(
                "POST",
                &machine_url,
                Some(PathfinderPayload::Json(machine_payload)),
            )
            .await
            .map_err(|e| {
                AppError::AuthFailed(format!("pathfinder user_machine failed: {}", e))
            })?;

        let machine_id = machine_data
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if machine_id.is_empty() {
            return Err(AppError::AuthFailed(
                "no machine ID returned from pathfinder".to_string(),
            ));
        }
        self.machine_id = machine_id.clone();
        self.emit(
            "log",
            json!(format!("Verification machine created: {}", machine_id)),
        );

        // Step 2: Poll /pathfinder/inquiries/{machine_id}/user_view/ until challenge appears
        let inquiries_url = format!(
            "{}/pathfinder/inquiries/{}/user_view/",
            ROBINHOOD_API_URL, machine_id
        );

        for _i in 0..12 {
            // Poll up to ~60 seconds
            tokio::time::sleep(Duration::from_secs(5)).await;

            let inquiries_resp = match self
                .make_pathfinder_request("GET", &inquiries_url, None)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    self.emit(
                        "log",
                        json!(format!("Inquiries poll error: {}, retrying...", e)),
                    );
                    continue;
                }
            };

            // Look for sheriff_challenge in context
            let ctx = match inquiries_resp.get("context").and_then(|v| v.as_object()) {
                Some(c) => c,
                None => continue,
            };
            let challenge = match ctx.get("sheriff_challenge").and_then(|v| v.as_object()) {
                Some(c) => c,
                None => continue,
            };

            let challenge_type = challenge
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let challenge_status = challenge
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let challenge_id_val = challenge
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            self.emit(
                "log",
                json!(format!(
                    "Sheriff challenge: type={} status={} id={}",
                    challenge_type, challenge_status, challenge_id_val
                )),
            );

            if challenge_type == "prompt" && challenge_status == "issued" {
                // App-based approval -- poll until user approves on their Robinhood app
                self.sheriff_challenge_id = challenge_id_val.clone();
                self.emit("log", json!("Waiting for approval on Robinhood app..."));
                self.emit(
                    "robinhood_prompt_approval",
                    json!("pending"),
                );

                let validated = self
                    .wait_for_prompt_approval(&challenge_id_val)
                    .await
                    .map_err(|e| {
                        AppError::AuthFailed(format!("prompt approval failed: {}", e))
                    })?;

                if validated {
                    // Prompt was approved -- confirm workflow and complete login
                    Box::pin(self.confirm_workflow_and_login()).await.map_err(|e| {
                        AppError::AuthFailed(format!("post-prompt login failed: {}", e))
                    })?;
                    return Ok("prompt_approved".to_string());
                }
                // If not validated, fall through to return prompt type for manual retry
                return Ok("prompt".to_string());
            }

            if (challenge_type == "sms" || challenge_type == "email")
                && challenge_status == "issued"
            {
                // SMS/email has been sent
                self.sheriff_challenge_id = challenge_id_val;
                return Ok(challenge_type);
            }

            if challenge_status == "validated" {
                // Already validated (e.g. from a previous attempt)
                self.sheriff_challenge_id = challenge_id_val;
                return Ok(challenge_type);
            }
        }

        Err(AppError::AuthFailed(
            "timed out waiting for verification challenge".to_string(),
        ))
    }

    /// Polls the push endpoint until the user approves on their Robinhood app.
    /// Matches robin_stocks' prompt handling in _validate_sherrif_id.
    async fn wait_for_prompt_approval(&self, challenge_id: &str) -> Result<bool, AppError> {
        let prompt_url = format!(
            "{}/push/{}/get_prompts_status/",
            ROBINHOOD_API_URL, challenge_id
        );

        for _i in 0..24 {
            // Poll up to ~2 minutes
            tokio::time::sleep(Duration::from_secs(5)).await;

            let resp = match self.make_pathfinder_request("GET", &prompt_url, None).await {
                Ok(r) => r,
                Err(e) => {
                    self.emit(
                        "log",
                        json!(format!("Prompt poll error: {}, retrying...", e)),
                    );
                    continue;
                }
            };

            let status = resp
                .get("challenge_status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            self.emit("log", json!(format!("Prompt status: {}", status)));

            if status == "validated" {
                self.emit("log", json!("Robinhood app approval received"));
                return Ok(true);
            }
        }

        Err(AppError::AuthFailed(
            "timed out waiting for Robinhood app approval".to_string(),
        ))
    }

    /// Confirms the verification workflow and re-attempts login.
    async fn confirm_workflow_and_login(&mut self) -> Result<(), AppError> {
        let inquiries_url = format!(
            "{}/pathfinder/inquiries/{}/user_view/",
            ROBINHOOD_API_URL, self.machine_id
        );
        let continue_payload = json!({
            "sequence": 0,
            "user_input": {"status": "continue"},
        });

        let mut workflow_confirmed = false;
        for _i in 0..5 {
            match self
                .make_pathfinder_request(
                    "POST",
                    &inquiries_url,
                    Some(PathfinderPayload::Json(continue_payload.clone())),
                )
                .await
            {
                Ok(confirm_resp) => {
                    self.emit(
                        "log",
                        json!(format!(
                            "Workflow confirm response keys: {:?}",
                            confirm_resp.keys().collect::<Vec<_>>()
                        )),
                    );
                    // Check for approval in type_context
                    if let Some(type_ctx) =
                        confirm_resp.get("type_context").and_then(|v| v.as_object())
                    {
                        if let Some(result_val) = type_ctx.get("result").and_then(|v| v.as_str()) {
                            if result_val == "workflow_status_approved" {
                                self.emit("log", json!("Verification workflow approved"));
                                workflow_confirmed = true;
                                break;
                            }
                        }
                    }
                    // Check for approval in verification_workflow
                    if let Some(vw) = confirm_resp
                        .get("verification_workflow")
                        .and_then(|v| v.as_object())
                    {
                        if let Some(ws) = vw.get("workflow_status").and_then(|v| v.as_str()) {
                            if ws == "workflow_status_approved" {
                                self.emit("log", json!("Verification workflow approved"));
                                workflow_confirmed = true;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    self.emit(
                        "log",
                        json!(format!("Workflow confirmation error: {}, retrying...", e)),
                    );
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if !workflow_confirmed {
            self.emit("log", json!("Warning: workflow confirmation not explicitly approved, proceeding anyway"));
        }

        // Clear verification state and re-attempt login
        self.mfa_required = false;
        self.sheriff_challenge_id.clear();
        self.machine_id.clear();
        self.verification_id.clear();

        // Brief delay to allow Robinhood to propagate the verification approval
        tokio::time::sleep(Duration::from_millis(1000)).await;

        let username = self.pending_username.clone();
        let password = self.pending_password.clone();
        self.emit("log", json!("Re-attempting login after workflow confirmation..."));
        Box::pin(self.attempt_login(&username, &password, "")).await
    }

    /// Handles MFA submission for the verification_workflow flow.
    async fn submit_verification_mfa(&mut self, mfa_code: &str) -> Result<(), AppError> {
        // Step 1: Submit the code to /challenge/{id}/respond/
        let challenge_url = format!(
            "{}/challenge/{}/respond/",
            ROBINHOOD_API_URL, self.sheriff_challenge_id
        );
        let challenge_params: Vec<(&str, String)> = vec![("response", mfa_code.to_string())];

        self.emit(
            "log",
            json!(format!(
                "Submitting verification code to challenge {}",
                self.sheriff_challenge_id
            )),
        );

        let challenge_resp = self
            .make_pathfinder_request(
                "POST",
                &challenge_url,
                Some(PathfinderPayload::Form(challenge_params)),
            )
            .await
            .map_err(|e| AppError::AuthFailed(format!("challenge response failed: {}", e)))?;

        let status = challenge_resp
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.emit(
            "log",
            json!(format!("Challenge response status: {}", status)),
        );

        if status != "validated" {
            return Err(AppError::AuthFailed(format!(
                "verification code rejected (status: {})",
                status
            )));
        }

        // Step 2: Confirm the workflow by posting continue to inquiries
        let inquiries_url = format!(
            "{}/pathfinder/inquiries/{}/user_view/",
            ROBINHOOD_API_URL, self.machine_id
        );
        let continue_payload = json!({
            "sequence": 0,
            "user_input": {"status": "continue"},
        });

        for _i in 0..5 {
            match self
                .make_pathfinder_request(
                    "POST",
                    &inquiries_url,
                    Some(PathfinderPayload::Json(continue_payload.clone())),
                )
                .await
            {
                Ok(confirm_resp) => {
                    // Check for approval in type_context
                    if let Some(type_ctx) =
                        confirm_resp.get("type_context").and_then(|v| v.as_object())
                    {
                        if let Some(result_val) = type_ctx.get("result").and_then(|v| v.as_str()) {
                            if result_val == "workflow_status_approved" {
                                self.emit("log", json!("Verification workflow approved"));
                                break;
                            }
                        }
                    }
                    // Check for approval in verification_workflow
                    if let Some(vw) = confirm_resp
                        .get("verification_workflow")
                        .and_then(|v| v.as_object())
                    {
                        if let Some(ws) = vw.get("workflow_status").and_then(|v| v.as_str()) {
                            if ws == "workflow_status_approved" {
                                self.emit("log", json!("Verification workflow approved"));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    self.emit(
                        "log",
                        json!(format!("Workflow confirmation error: {}, retrying...", e)),
                    );
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        // Step 3: Re-attempt login (the verification is now complete server-side)
        self.mfa_required = false;
        self.sheriff_challenge_id.clear();
        self.machine_id.clear();
        self.verification_id.clear();

        let username = self.pending_username.clone();
        let password = self.pending_password.clone();
        Box::pin(self.attempt_login(&username, &password, "")).await
    }

    /// Performs the actual token refresh call.
    /// Uses RwLock interior mutability so it works with &self.
    async fn do_refresh_token(&self) -> Result<(), AppError> {
        let refresh_tok = self.refresh_token_value.read().await.clone();
        if refresh_tok.is_empty() {
            self.emit(
                "tokenExpired",
                json!({
                    "brokerId": self.id,
                    "message": "Your Robinhood session has expired. Please log in again."
                }),
            );
            self.auth_token.write().await.clear();
            *self.is_logged_in_flag.write().await = false;
            return Err(AppError::AuthFailed(
                "no refresh token available, please log in again".to_string(),
            ));
        }

        let refresh_url = format!("{}/oauth2/token/", ROBINHOOD_API_URL);
        let params: Vec<(&str, String)> = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_tok),
            ("client_id", ROBINHOOD_CLIENT_ID.to_string()),
            ("scope", "internal".to_string()),
            ("device_token", self.device_token.clone()),
            ("expires_in", "86400".to_string()),
        ];

        let form_body = build_form_body(&params);

        let builder = self.client.post(&refresh_url).body(form_body);
        let builder = self.apply_standard_headers(builder);

        let resp = builder.send().await.map_err(|e| {
            AppError::NetworkError(format!("token refresh request failed: {}", e))
        })?;

        let status = resp.status();
        let body_bytes = resp.bytes().await.unwrap_or_default();

        if !status.is_success() {
            self.emit(
                "tokenExpired",
                json!({
                    "brokerId": self.id,
                    "message": "Your Robinhood session has expired. Please log in again."
                }),
            );
            self.auth_token.write().await.clear();
            self.refresh_token_value.write().await.clear();
            *self.is_logged_in_flag.write().await = false;
            return Err(AppError::AuthFailed(format!(
                "token refresh failed, status: {}, body: {}",
                status.as_u16(),
                String::from_utf8_lossy(&body_bytes)
            )));
        }

        let result: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&body_bytes).map_err(|e| AppError::ApiError(e.to_string()))?;

        if let Some(at) = result.get("access_token").and_then(|v| v.as_str()) {
            *self.auth_token.write().await = at.to_string();
        }
        if let Some(tt) = result.get("token_type").and_then(|v| v.as_str()) {
            *self.token_type.write().await = tt.to_string();
        }
        if let Some(rt) = result.get("refresh_token").and_then(|v| v.as_str()) {
            *self.refresh_token_value.write().await = rt.to_string();
        }

        log::info!("Robinhood: Token refreshed successfully");
        Ok(())
    }

    /// Makes an authenticated request to the Robinhood API.
    /// Handles 401 responses by attempting a token refresh and retrying.
    /// Uses RwLock interior mutability so this works with &self.
    async fn make_authenticated_request(
        &self,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Vec<u8>, AppError> {
        let do_request = |auth_header: String, body_clone: Option<serde_json::Value>| {
            let client = self.client.clone();
            let url = url.to_string();
            let method = method.to_string();
            async move {
                let mut builder = match method.as_str() {
                    "POST" => client.post(&url),
                    "PUT" => client.put(&url),
                    "DELETE" => client.delete(&url),
                    "PATCH" => client.patch(&url),
                    _ => client.get(&url),
                };

                builder = builder
                    .header("Accept", "*/*")
                    .header("Accept-Language", "en-US,en;q=1")
                    .header("X-Robinhood-API-Version", "1.431.4")
                    .header("Connection", "keep-alive")
                    .header("User-Agent", "*")
                    .header("Authorization", &auth_header);

                if let Some(ref b) = body_clone {
                    builder = builder
                        .header("Content-Type", "application/json")
                        .body(serde_json::to_string(b).unwrap_or_default());
                } else {
                    builder = builder.header(
                        "Content-Type",
                        "application/x-www-form-urlencoded; charset=utf-8",
                    );
                }

                builder.send().await
            }
        };

        let body_clone = body.cloned();
        let auth = self.auth_header_value().await;

        let resp = do_request(auth, body_clone)
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let resp_body = resp
            .bytes()
            .await
            .map_err(|e| AppError::NetworkError(e.to_string()))?
            .to_vec();

        if status.as_u16() == 401 {
            log::info!("Robinhood: Received 401 Unauthorized, attempting to refresh token...");

            if let Err(refresh_err) = self.do_refresh_token().await {
                self.emit(
                    "tokenExpired",
                    json!({
                        "brokerId": self.id,
                        "message": "Your Robinhood session has expired. Please log in again."
                    }),
                );
                return Err(AppError::AuthFailed(format!(
                    "unauthorized and token refresh failed: {}",
                    refresh_err
                )));
            }

            log::info!("Robinhood: Token refreshed, retrying request...");

            let retry_auth = self.auth_header_value().await;
            let retry_body = body.cloned();
            let retry_resp = do_request(retry_auth, retry_body)
                .await
                .map_err(|e| AppError::NetworkError(e.to_string()))?;

            let retry_status = retry_resp.status();
            let retry_body_bytes = retry_resp
                .bytes()
                .await
                .map_err(|e| AppError::NetworkError(e.to_string()))?
                .to_vec();

            if retry_status.as_u16() == 401 {
                self.emit(
                    "tokenExpired",
                    json!({
                        "brokerId": self.id,
                        "message": "Your Robinhood session has expired. Please log in again."
                    }),
                );
                return Err(AppError::AuthFailed(
                    "unauthorized after token refresh".to_string(),
                ));
            }

            if !retry_status.is_success() {
                return Err(AppError::ApiError(format!(
                    "request failed with status {}: {}",
                    retry_status.as_u16(),
                    String::from_utf8_lossy(&retry_body_bytes)
                )));
            }

            return Ok(retry_body_bytes);
        }

        if !status.is_success() {
            return Err(AppError::ApiError(format!(
                "request failed with status {}: {}",
                status.as_u16(),
                String::from_utf8_lossy(&resp_body)
            )));
        }

        Ok(resp_body)
    }

    /// Loads the user's Robinhood accounts.
    async fn load_accounts(&mut self) -> Result<(), AppError> {
        let url = format!(
            "{}/accounts/?default_to_all_accounts=true",
            ROBINHOOD_API_URL
        );

        self.emit("log", json!(format!("Loading accounts from {}", url)));
        let resp_body = self.make_authenticated_request("GET", &url, None).await?;
        self.emit(
            "log",
            json!(format!("Accounts response: {} bytes", resp_body.len())),
        );

        #[derive(Deserialize)]
        struct AccountsResponse {
            #[serde(default)]
            results: Vec<RobinhoodAccount>,
        }

        let parsed: AccountsResponse = serde_json::from_slice(&resp_body)
            .map_err(|e| {
                self.emit(
                    "log",
                    json!(format!(
                        "Failed to parse accounts response: {} (body: {})",
                        e,
                        String::from_utf8_lossy(&resp_body).chars().take(500).collect::<String>()
                    )),
                );
                AppError::ApiError(format!("failed to parse accounts: {}", e))
            })?;

        self.emit(
            "log",
            json!(format!("Loaded {} accounts", parsed.results.len())),
        );
        self.accounts = parsed.results;
        Ok(())
    }

    /// Fetches instrument details (symbol, name, price, tradability) from an instrument URL.
    async fn get_instrument_details(&self, instrument_url: &str) -> InstrumentInfo {
        let resp_body = match self
            .make_authenticated_request("GET", instrument_url, None)
            .await
        {
            Ok(b) => b,
            Err(_) => return InstrumentInfo::empty(),
        };

        let instrument: HashMap<String, serde_json::Value> =
            match serde_json::from_slice(&resp_body) {
                Ok(v) => v,
                Err(_) => return InstrumentInfo::empty(),
            };

        let mut info = InstrumentInfo {
            symbol: String::new(),
            name: String::new(),
            price: String::new(),
            tradable: true, // default tradable unless API says otherwise
        };

        info.symbol = instrument
            .get("symbol")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        info.name = instrument
            .get("simple_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if info.name.is_empty() {
            info.name = instrument
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        // Check tradability from instrument data
        if let Some(tradeable) = instrument.get("tradeable").and_then(|v| v.as_bool()) {
            info.tradable = tradeable;
        }
        if let Some(state) = instrument.get("state").and_then(|v| v.as_str()) {
            if state != "active" {
                info.tradable = false;
            }
        }
        if let Some(tradability) = instrument.get("tradability").and_then(|v| v.as_str()) {
            if tradability != "tradable" {
                info.tradable = false;
            }
        }

        // Get quote for current price
        if !info.symbol.is_empty() {
            if let Ok(quote) = self.get_stock_quote_internal(&info.symbol).await {
                if let Some(last_price) = quote.get("last_trade_price").and_then(|v| v.as_str()) {
                    info.price = last_price.to_string();
                }
            }
        }

        info
    }

    /// Gets the instrument URL for a ticker symbol.
    async fn get_instrument_url(&self, ticker: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/instruments/?symbol={}",
            ROBINHOOD_API_URL,
            ticker.to_uppercase()
        );

        let resp_body = self.make_authenticated_request("GET", &url, None).await?;

        #[derive(Deserialize)]
        struct InstrumentsResponse {
            #[serde(default)]
            results: Vec<HashMap<String, serde_json::Value>>,
        }

        let parsed: InstrumentsResponse = serde_json::from_slice(&resp_body)
            .map_err(|e| AppError::ApiError(format!("failed to parse instruments: {}", e)))?;

        if parsed.results.is_empty() {
            return Err(AppError::ApiError(format!(
                "instrument not found for ticker {}",
                ticker
            )));
        }

        parsed.results[0]
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::ApiError("no URL for instrument".to_string()))
    }

    /// Internal stock quote helper that works with &self.
    async fn get_stock_quote_internal(
        &self,
        ticker: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let url = format!("{}/quotes/{}/", ROBINHOOD_API_URL, ticker.to_uppercase());

        let resp_body = self.make_authenticated_request("GET", &url, None).await?;

        let result: HashMap<String, serde_json::Value> = serde_json::from_slice(&resp_body)
            .map_err(|e| AppError::ApiError(format!("failed to parse quote: {}", e)))?;

        Ok(result)
    }

    /// Checks whether an account matches any of the requested names.
    fn account_matches(acc: &RobinhoodAccount, names: &[&str]) -> bool {
        let base_name = format!("{} Account", acc.account_type);
        let disambiguated = format!("{} ({})", base_name, acc.account_number);
        for name in names {
            let name = name.trim();
            if acc.account_number == name || base_name == name || disambiguated == name {
                return true;
            }
        }
        false
    }

    /// Places an order on a specific account (matching robin_stocks order() payload).
    async fn place_order_on_account(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account_url: &str,
        instrument_url: &str,
    ) -> String {
        let order_url = format!("{}/orders/", ROBINHOOD_API_URL);

        let order_side = if side.eq_ignore_ascii_case("sell") {
            "sell"
        } else {
            "buy"
        };

        // Determine price type based on side (matching robin_stocks)
        let price_type = if order_side == "sell" {
            "bid_price"
        } else {
            "ask_price"
        };

        // Fetch current quote for price context
        let mut ask_price_f: f64 = 0.0;
        let mut bid_price_f: f64 = 0.0;
        let mut price_f: f64 = 0.0;

        if let Ok(quote) = self.get_stock_quote_internal(ticker).await {
            if let Some(ap) = quote.get("ask_price").and_then(|v| v.as_str()) {
                ask_price_f = ap.parse::<f64>().unwrap_or(0.0);
            }
            if let Some(bp) = quote.get("bid_price").and_then(|v| v.as_str()) {
                bid_price_f = bp.parse::<f64>().unwrap_or(0.0);
            }
            // Use the appropriate price based on side
            price_f = if price_type == "ask_price" {
                ask_price_f
            } else {
                bid_price_f
            };
            // Fallback to last_trade_price if the side-specific price is 0
            if price_f == 0.0 {
                if let Some(lp) = quote.get("last_trade_price").and_then(|v| v.as_str()) {
                    price_f = lp.parse::<f64>().unwrap_or(0.0);
                }
            }
        }

        // Build order payload (matching robin_stocks order() function)
        let bid_ask_timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string();

        let mut order = json!({
            "account": account_url,
            "instrument": instrument_url,
            "symbol": ticker.to_uppercase(),
            "price": round_price(price_f),
            "ask_price": round_price(ask_price_f),
            "bid_price": round_price(bid_price_f),
            "bid_ask_timestamp": bid_ask_timestamp,
            "quantity": shares,
            "ref_id": uuid::Uuid::new_v4().to_string(),
            "type": "market",
            "time_in_force": "gfd",
            "trigger": "immediate",
            "side": order_side,
            "market_hours": "regular_hours",
            "extended_hours": false,
            "order_form_version": 4,
        });

        // Apply regular_hours rules (matching robin_stocks)
        if order_side == "buy" {
            order["preset_percent_limit"] = json!("0.05");
            order["type"] = json!("limit");
        } else if order_side == "sell" {
            // For market sells during regular hours, robin_stocks deletes the price field
            if let Some(obj) = order.as_object_mut() {
                obj.remove("price");
            }
        }

        let resp_body = match self
            .make_authenticated_request("POST", &order_url, Some(&order))
            .await
        {
            Ok(b) => b,
            Err(e) => {
                self.emit("log", json!(format!("Order failed: {}", e)));
                return format!("Error: {}", e);
            }
        };

        let result: HashMap<String, serde_json::Value> = match serde_json::from_slice(&resp_body) {
            Ok(v) => v,
            Err(e) => {
                return format!("Order submitted but failed to parse response: {}", e);
            }
        };

        let order_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let state = result
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        self.emit(
            "log",
            json!(format!(
                "Order submitted: {} {} shares of {} (Order ID: {}, State: {})",
                order_side.to_uppercase(),
                shares,
                ticker,
                order_id,
                state
            )),
        );

        format!(
            "Order submitted: {} {} shares of {}",
            order_side.to_uppercase(),
            shares,
            ticker
        )
    }
}

impl InstrumentInfo {
    fn empty() -> Self {
        Self {
            symbol: String::new(),
            name: String::new(),
            price: String::new(),
            tradable: false,
        }
    }
}

/// Rounds a price to the appropriate decimal places (matching robin_stocks round_price).
fn round_price(price: f64) -> String {
    if price <= 1e-2 {
        format!("{:.6}", (price * 1e6).round() / 1e6)
    } else if price < 1e0 {
        format!("{:.4}", (price * 1e4).round() / 1e4)
    } else {
        format!("{:.2}", (price * 1e2).round() / 1e2)
    }
}

/// Builds a form-encoded body string from key-value pairs.
fn build_form_body(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                url::form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Payload variants for pathfinder requests.
enum PathfinderPayload {
    Json(serde_json::Value),
    Form(Vec<(&'static str, String)>),
}

// ─── Broker trait implementation ────────────────────────────────────────────

#[async_trait]
impl Broker for RobinhoodBroker {
    fn get_type(&self) -> BrokerType {
        BrokerType::Robinhood
    }

    fn get_name(&self) -> &str {
        "Robinhood"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    async fn start_2fa(&mut self, email: &str) -> Result<(), AppError> {
        // For Robinhood, we just store the email - actual auth happens in login
        self.email = email.to_string();
        self.emit(
            "log",
            json!(format!("Ready to login with email: {}", email)),
        );
        Ok(())
    }

    async fn login(&mut self, code: &str, email: &str) -> Result<(), AppError> {
        if self.mfa_required {
            // This is an MFA code submission
            return self.submit_mfa(code).await;
        }

        // First login attempt - code is the password
        self.email = email.to_string();
        self.pending_username = email.to_string();
        self.pending_password = code.to_string();

        self.attempt_login(email, code, "").await
    }

    async fn login_with_stored_credentials(
        &mut self,
        creds: &StoredCredentials,
    ) -> Result<(), AppError> {
        log::info!(
            "Robinhood: Attempting to restore session for {}",
            creds.email
        );
        self.emit(
            "log",
            json!(format!(
                "Restoring Robinhood session: access_token len={}, refresh_token len={}, device_token len={}, token_type={}",
                creds.access_token.len(),
                creds.refresh_token.len(),
                creds.device_token.len(),
                creds.token_type
            )),
        );

        *self.auth_token.write().await = creds.access_token.clone();
        *self.refresh_token_value.write().await = creds.refresh_token.clone();
        *self.token_type.write().await = creds.token_type.clone();
        self.email = creds.email.clone();
        *self.is_logged_in_flag.write().await = true;
        self.login_time = Some(Utc::now());

        // Restore device token if available, otherwise keep the one generated at initialization
        if !creds.device_token.is_empty() {
            self.device_token = creds.device_token.clone();
        }

        // Validate token by loading accounts
        match self.load_accounts().await {
            Ok(()) => {}
            Err(_e) => {
                log::info!(
                    "Robinhood: Token validation failed, attempting refresh: {}",
                    _e
                );

                // Try to refresh the token
                if let Err(refresh_err) = self.do_refresh_token().await {
                    log::info!("Robinhood: Token refresh failed: {}", refresh_err);
                    self.auth_token.write().await.clear();
                    self.refresh_token_value.write().await.clear();
                    *self.is_logged_in_flag.write().await = false;
                    return Err(AppError::AuthFailed(format!(
                        "failed to restore session: {}",
                        refresh_err
                    )));
                }

                // Retry loading accounts after refresh
                if let Err(e2) = self.load_accounts().await {
                    log::info!(
                        "Robinhood: Token refresh succeeded but validation still failed: {}",
                        e2
                    );
                    self.auth_token.write().await.clear();
                    self.refresh_token_value.write().await.clear();
                    *self.is_logged_in_flag.write().await = false;
                    return Err(AppError::AuthFailed(format!(
                        "token refresh succeeded but validation failed: {}",
                        e2
                    )));
                }
            }
        }

        log::info!("Robinhood: Session restored successfully for {}", self.email);
        Ok(())
    }

    async fn refresh_token(&mut self) -> Result<(), AppError> {
        self.do_refresh_token().await
    }

    fn logout(&mut self) {
        // Revoke the OAuth2 token (fire-and-forget)
        let token = self.auth_token.try_read().map(|v| v.clone()).unwrap_or_default();
        if !token.is_empty() {
            let client = self.client.clone();
            tokio::spawn(async move {
                let logout_url = format!("{}/oauth2/revoke_token/", ROBINHOOD_API_URL);
                let params = vec![
                    ("client_id", ROBINHOOD_CLIENT_ID.to_string()),
                    ("token", token),
                ];
                let form_body = build_form_body(
                    &params
                        .iter()
                        .map(|(k, v)| (*k, v.clone()))
                        .collect::<Vec<_>>(),
                );
                let _ = client
                    .post(&logout_url)
                    .header("Accept", "*/*")
                    .header("Accept-Language", "en-US,en;q=1")
                    .header(
                        "Content-Type",
                        "application/x-www-form-urlencoded; charset=utf-8",
                    )
                    .header("X-Robinhood-API-Version", "1.431.4")
                    .header("Connection", "keep-alive")
                    .header("User-Agent", "*")
                    .body(form_body)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await;
            });
        }

        if let Ok(mut t) = self.auth_token.try_write() { t.clear(); }
        if let Ok(mut t) = self.token_type.try_write() { t.clear(); }
        if let Ok(mut t) = self.refresh_token_value.try_write() { t.clear(); }
        self.email.clear();
        if let Ok(mut f) = self.is_logged_in_flag.try_write() { *f = false; }
        self.accounts.clear();
        self.mfa_required = false;
        self.challenge_id.clear();
        self.verification_id.clear();
        self.machine_id.clear();
        self.sheriff_challenge_id.clear();
    }

    fn is_logged_in(&self) -> bool {
        let logged_in = self.is_logged_in_flag.try_read().map(|v| *v).unwrap_or(false);
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
        if self.auth_token.read().await.is_empty() {
            return false;
        }

        let check_url = format!("{}/user/", ROBINHOOD_API_URL);
        let auth = self.auth_header_value().await;

        let builder = self
            .client
            .get(&check_url)
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=1")
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .header("X-Robinhood-API-Version", "1.431.4")
            .header("Connection", "keep-alive")
            .header("User-Agent", "*")
            .header("Authorization", auth)
            .timeout(Duration::from_secs(10));

        match builder.send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    fn export_credentials(&self) -> Result<StoredCredentials, AppError> {
        let logged_in = self.is_logged_in_flag.try_read().map(|v| *v).unwrap_or(false);
        if !logged_in {
            return Err(AppError::BrokerNotLoggedIn);
        }

        Ok(StoredCredentials {
            broker_type: BrokerType::Robinhood.to_string(),
            broker_id: self.id.clone(),
            email: self.email.clone(),
            access_token: self.auth_token.try_read().map(|v| v.clone()).unwrap_or_default(),
            token_type: self.token_type.try_read().map(|v| v.clone()).unwrap_or_default(),
            refresh_token: self.refresh_token_value.try_read().map(|v| v.clone()).unwrap_or_default(),
            device_token: self.device_token.clone(),
        })
    }

    async fn get_accounts(&self) -> Result<Vec<serde_json::Value>, AppError> {
        if self.auth_token.read().await.is_empty() {
            return Err(AppError::BrokerNotLoggedIn);
        }

        // If accounts are empty we need to load them, but we only have &self.
        // Use the cached accounts; they are populated during login/restore.
        let accs = &self.accounts;
        if accs.is_empty() {
            // Attempt a one-off fetch
            let url = format!(
                "{}/accounts/?default_to_all_accounts=true",
                ROBINHOOD_API_URL
            );
            let resp_body = self.make_authenticated_request("GET", &url, None).await?;

            #[derive(Deserialize)]
            struct AccountsResponse {
                results: Vec<RobinhoodAccount>,
            }

            let parsed: AccountsResponse = serde_json::from_slice(&resp_body)
                .map_err(|e| AppError::ApiError(format!("failed to parse accounts: {}", e)))?;

            return Ok(build_account_list(&parsed.results));
        }

        Ok(build_account_list(accs))
    }

    async fn get_account_details(
        &self,
        account_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        let url = format!("{}/accounts/{}/", ROBINHOOD_API_URL, account_id);

        let resp_body = self.make_authenticated_request("GET", &url, None).await?;

        let result: serde_json::Value = serde_json::from_slice(&resp_body)
            .map_err(|e| AppError::ApiError(format!("failed to parse account details: {}", e)))?;

        Ok(result)
    }

    async fn get_account_holdings(
        &self,
        account_id: &str,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let mut holdings: Vec<serde_json::Value> = Vec::new();
        let mut next_url = format!(
            "{}/positions/?nonzero=true&account_number={}",
            ROBINHOOD_API_URL, account_id
        );

        loop {
            let resp_body = self
                .make_authenticated_request("GET", &next_url, None)
                .await?;

            #[derive(Deserialize)]
            struct PositionsResponse {
                #[serde(default)]
                results: Vec<HashMap<String, serde_json::Value>>,
                next: Option<String>,
            }

            let parsed: PositionsResponse = serde_json::from_slice(&resp_body)
                .map_err(|e| AppError::ApiError(format!("failed to parse positions: {}", e)))?;

            for pos in &parsed.results {
                let quantity = pos
                    .get("quantity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if quantity.is_empty()
                    || quantity == "0"
                    || quantity == "0.00000000"
                {
                    continue;
                }

                let instrument_url = match pos.get("instrument").and_then(|v| v.as_str()) {
                    Some(u) if !u.is_empty() => u,
                    _ => continue,
                };

                let info = self.get_instrument_details(instrument_url).await;
                if info.symbol.is_empty() {
                    continue;
                }

                let avg_cost = pos
                    .get("average_buy_price")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Calculate market value
                let qty: f64 = quantity.parse().unwrap_or(0.0);
                let prc: f64 = info.price.parse().unwrap_or(0.0);
                let market_value = qty * prc;

                holdings.push(json!({
                    "ticker": info.symbol,
                    "name": info.name,
                    "shares": quantity,
                    "price": info.price,
                    "marketValue": format!("{:.2}", market_value),
                    "costBasis": avg_cost,
                    "canSell": info.tradable,
                }));
            }

            match parsed.next {
                Some(ref u) if !u.is_empty() => next_url = u.clone(),
                _ => break,
            }
        }

        Ok(holdings)
    }

    async fn get_account_cash(
        &self,
        account_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        let url = format!("{}/accounts/{}/", ROBINHOOD_API_URL, account_id);

        let resp_body = self.make_authenticated_request("GET", &url, None).await?;

        let result: HashMap<String, serde_json::Value> = serde_json::from_slice(&resp_body)
            .map_err(|e| AppError::ApiError(format!("failed to parse account: {}", e)))?;

        let cash = result
            .get("cash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let buying_power = result
            .get("buying_power")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut cash_withdrawable = result
            .get("cash_available_for_withdrawal")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if cash_withdrawable.is_empty() {
            cash_withdrawable = cash.clone();
        }

        Ok(json!({
            "currency": "USD",
            "balance": {
                "canTrade": buying_power,
                "canWithdraw": cash_withdrawable,
                "buyingPower": buying_power,
            }
        }))
    }

    async fn place_order(
        &self,
        ticker: &str,
        side: &str,
        shares: f64,
        account: &str,
        sell_max: bool,
    ) -> String {
        if !*self.is_logged_in_flag.read().await {
            return "Error: Not logged in".to_string();
        }

        // Get instrument URL for the ticker
        let instrument_url = match self.get_instrument_url(ticker).await {
            Ok(u) => u,
            Err(e) => return format!("Error: {}", e),
        };

        // Find account URL and account number
        let mut account_url = String::new();
        let mut resolved_account_number = String::new();
        for acc in &self.accounts {
            let base_name = format!("{} Account", acc.account_type);
            let disambiguated = format!("{} ({})", base_name, acc.account_number);
            if acc.account_number == account
                || base_name == account
                || disambiguated == account
            {
                account_url = acc.url.clone();
                resolved_account_number = acc.account_number.clone();
                break;
            }
        }

        if account_url.is_empty() && !self.accounts.is_empty() {
            // Use first account if no match
            account_url = self.accounts[0].url.clone();
            resolved_account_number = self.accounts[0].account_number.clone();
        }

        if account_url.is_empty() {
            return "Error: No account found".to_string();
        }

        // Parse comma-separated accounts
        let requested_names: Vec<&str> = account.split(',').collect();
        if requested_names.len() > 1 || account == "All accounts" {
            // Multiple accounts - place on each
            let mut results: Vec<String> = Vec::new();
            for acc in &self.accounts {
                if account == "All accounts"
                    || Self::account_matches(acc, &requested_names)
                {
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
                                    self.emit("log", json!(format!(
                                        "Robinhood {}: No shares of {} in this account, skipping.",
                                        acc.account_number, ticker
                                    )));
                                    continue;
                                }
                                if sell_max {
                                    self.emit("log", json!(format!(
                                        "Robinhood {}: Selling all {} shares held.",
                                        acc.account_number, owned
                                    )));
                                    owned
                                } else {
                                    let capped = shares.min(owned);
                                    if (capped - shares).abs() > 0.001 {
                                        self.emit("log", json!(format!(
                                            "Robinhood {}: Requested {} shares but account holds {}, selling {}.",
                                            acc.account_number, shares, owned, capped
                                        )));
                                    }
                                    capped
                                }
                            }
                            Err(e) => {
                                if sell_max {
                                    self.emit("log", json!(format!(
                                        "Robinhood {}: Could not look up holdings: {}. Skipping account.",
                                        acc.account_number, e
                                    )));
                                    continue;
                                }
                                self.emit("log", json!(format!(
                                    "Robinhood {}: Could not look up holdings: {}. Using requested shares.",
                                    acc.account_number, e
                                )));
                                shares
                            }
                        }
                    } else {
                        shares
                    };
                    let result = self
                        .place_order_on_account(
                            ticker,
                            side,
                            order_shares,
                            &acc.url,
                            &instrument_url,
                        )
                        .await;
                    results.push(result);
                }
            }
            return results.join("; ");
        }

        // Single account - check holdings for sell orders
        let order_shares = if side.eq_ignore_ascii_case("sell") && !resolved_account_number.is_empty() {
            match self.get_account_holdings(&resolved_account_number).await {
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
                        self.emit("log", json!(format!(
                            "Robinhood {}: Selling all {} shares held.",
                            resolved_account_number, owned
                        )));
                        owned
                    } else {
                        let capped = shares.min(owned);
                        if (capped - shares).abs() > 0.001 {
                            self.emit("log", json!(format!(
                                "Robinhood {}: Requested {} shares but account holds {}, selling {}.",
                                resolved_account_number, shares, owned, capped
                            )));
                        }
                        capped
                    }
                }
                Err(e) => {
                    if sell_max {
                        return format!("Error: Could not look up holdings for {}: {}", resolved_account_number, e);
                    }
                    shares
                }
            }
        } else {
            shares
        };
        self.place_order_on_account(ticker, side, order_shares, &account_url, &instrument_url)
            .await
    }

    async fn is_market_open(&self) -> Result<bool, AppError> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let url = format!("{}/markets/XNYS/hours/{}/", ROBINHOOD_API_URL, today);

        match self.make_authenticated_request("GET", &url, None).await {
            Ok(resp_body) => {
                let result: HashMap<String, serde_json::Value> =
                    serde_json::from_slice(&resp_body).map_err(|e| {
                        AppError::ApiError(format!("failed to parse market hours: {}", e))
                    })?;

                let is_open = result
                    .get("is_open")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(is_open)
            }
            Err(_) => {
                // If we can't check, assume market is open during normal hours
                let now = Utc::now();
                let hour = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
                let weekday = now.format("%u").to_string().parse::<u32>().unwrap_or(0);
                // %u: Monday=1 .. Sunday=7
                if weekday >= 6 {
                    return Ok(false);
                }
                Ok(hour >= 9 && hour < 16)
            }
        }
    }

    async fn get_stock_quote(
        &self,
        ticker: &str,
    ) -> Result<serde_json::Value, AppError> {
        let result = self.get_stock_quote_internal(ticker).await?;
        Ok(serde_json::to_value(result)
            .map_err(|e| AppError::ApiError(format!("failed to serialize quote: {}", e)))?)
    }

    fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
}

/// Builds the normalized account list from raw RobinhoodAccount structs.
fn build_account_list(accounts: &[RobinhoodAccount]) -> Vec<serde_json::Value> {
    // First pass: count accounts per base name for deduplication
    let mut base_name_counts: HashMap<String, usize> = HashMap::new();
    for acc in accounts {
        let base = if acc.account_type.is_empty() {
            "Individual".to_string()
        } else {
            acc.account_type.clone()
        };
        let full = format!("{} Account", base);
        *base_name_counts.entry(full).or_insert(0) += 1;
    }

    // Second pass: build accounts with unique names
    let mut result = Vec::with_capacity(accounts.len());
    for acc in accounts {
        let base = if acc.account_type.is_empty() {
            "Individual".to_string()
        } else {
            acc.account_type.clone()
        };
        let base_name = format!("{} Account", base);

        let name = if base_name_counts.get(&base_name).copied().unwrap_or(0) > 1 {
            format!("{} ({})", base_name, acc.account_number)
        } else {
            base_name
        };

        let cash_value = if !acc.cash.is_empty() {
            &acc.cash
        } else {
            &acc.portfolio_cash
        };

        result.push(json!({
            "id": acc.account_number,
            "name": name,
            "status": "APPROVED",
            "isPrimary": accounts.len() == 1,
            "accountType": acc.account_type,
            "buyingPower": acc.buying_power,
            "cash": cash_value,
        }));
    }

    result
}
