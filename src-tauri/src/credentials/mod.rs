pub mod migration;
pub mod store;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredCredentials {
    pub broker_type: String,
    pub broker_id: String,
    pub email: String,
    pub access_token: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token_type: String,
    pub refresh_token: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub device_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredData {
    pub version: u32,
    pub credentials: HashMap<String, StoredCredentials>,
    pub last_active: String,
}

impl Default for StoredData {
    fn default() -> Self {
        Self {
            version: 1,
            credentials: HashMap::new(),
            last_active: String::new(),
        }
    }
}
