use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use iota_stronghold::{Client, KeyProvider, Stronghold, SnapshotPath};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::credentials::{StoredCredentials, StoredData};
use crate::error::AppError;

const STORE_KEY: &[u8] = b"credentials_data";
const CLIENT_PATH: &[u8] = b"credentials";
const SALT: &[u8] = b"MultiTradeCredentialsSalt2025";
const PBKDF2_ITERATIONS: u32 = 100_000;
const KEY_LEN: usize = 32;

pub struct StrongholdStore {
    stronghold: Stronghold,
    keyprovider: KeyProvider,
    snapshot_path: SnapshotPath,
    data: StoredData,
}

impl StrongholdStore {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&app_data_dir)
            .map_err(|e| AppError::CredentialError(format!("Failed to create data dir: {}", e)))?;

        let snapshot_path = SnapshotPath::from_path(app_data_dir.join("credentials.stronghold"));

        // Derive encryption key from machine-specific identifier (fast, no subprocess)
        let identifier = get_store_identifier();

        let mut key_bytes = [0u8; KEY_LEN];
        pbkdf2_hmac::<Sha256>(
            identifier.as_bytes(),
            SALT,
            PBKDF2_ITERATIONS,
            &mut key_bytes,
        );

        let keyprovider = KeyProvider::try_from(Zeroizing::new(key_bytes.to_vec()))
            .map_err(|e| AppError::CredentialError(format!("Failed to create key provider: {:?}", e)))?;

        let stronghold = Stronghold::default();

        let mut store = Self {
            stronghold,
            keyprovider,
            snapshot_path,
            data: StoredData::default(),
        };

        // Try to load from Stronghold snapshot
        if store.snapshot_path.exists() {
            if let Err(e) = store.load_from_stronghold() {
                log::warn!("Could not load from Stronghold snapshot: {}", e);
            }
        } else {
            // Check for legacy plain JSON file and migrate it
            store.migrate_from_json(&app_data_dir);
        }

        Ok(store)
    }

    /// Load data from the encrypted Stronghold snapshot
    fn load_from_stronghold(&mut self) -> Result<(), AppError> {
        self.stronghold
            .load_snapshot(&self.keyprovider, &self.snapshot_path)
            .map_err(|e| AppError::CredentialError(format!("Failed to load snapshot: {}", e)))?;

        let client = self.stronghold
            .load_client(CLIENT_PATH)
            .map_err(|e| AppError::CredentialError(format!("Failed to load client: {}", e)))?;

        let store = client.store();
        match store.get(STORE_KEY) {
            Ok(Some(bytes)) => {
                self.data = serde_json::from_slice(&bytes)
                    .map_err(|e| AppError::CredentialError(format!("Failed to parse credentials: {}", e)))?;
                log::info!("Loaded {} credentials from Stronghold", self.data.credentials.len());
            }
            Ok(None) => {
                log::info!("Stronghold snapshot exists but no credentials stored yet");
            }
            Err(e) => {
                return Err(AppError::CredentialError(format!("Failed to read store: {}", e)));
            }
        }

        Ok(())
    }

    /// Migrate from legacy plain JSON file if it exists
    fn migrate_from_json(&mut self, app_data_dir: &PathBuf) {
        let json_path = app_data_dir.join("credentials.json");
        if !json_path.exists() {
            // Also check the old config subdirectory path
            let old_config_path = app_data_dir.join("config").join("credentials.json");
            if old_config_path.exists() {
                self.do_migrate_json(&old_config_path);
            }
            return;
        }
        self.do_migrate_json(&json_path);
    }

    fn do_migrate_json(&mut self, json_path: &PathBuf) {
        log::info!("Found legacy JSON credentials at {}, migrating to Stronghold...", json_path.display());
        match fs::read_to_string(json_path) {
            Ok(contents) => {
                match serde_json::from_str::<StoredData>(&contents) {
                    Ok(data) => {
                        self.data = data;
                        if let Err(e) = self.save() {
                            log::error!("Failed to save migrated credentials to Stronghold: {}", e);
                            return;
                        }
                        // Delete the plain JSON file after successful migration
                        if let Err(e) = fs::remove_file(json_path) {
                            log::warn!("Failed to delete legacy JSON file: {}", e);
                        } else {
                            log::info!("Migrated credentials to Stronghold and deleted plain JSON file");
                        }
                    }
                    Err(e) => log::warn!("Failed to parse legacy JSON credentials: {}", e),
                }
            }
            Err(e) => log::warn!("Failed to read legacy JSON credentials: {}", e),
        }
    }

    /// Save data to the encrypted Stronghold snapshot
    fn save(&self) -> Result<(), AppError> {
        let value = serde_json::to_vec(&self.data)
            .map_err(|e| AppError::CredentialError(format!("Failed to serialize credentials: {}", e)))?;

        // Get or create the client
        let client = self.get_or_create_client()?;

        client.store()
            .insert(STORE_KEY.to_vec(), value, None)
            .map_err(|e| AppError::CredentialError(format!("Failed to write to store: {}", e)))?;

        // Write the client state to snapshot first
        self.stronghold
            .write_client(CLIENT_PATH)
            .map_err(|e| AppError::CredentialError(format!("Failed to write client: {}", e)))?;

        // Commit snapshot to disk
        self.stronghold
            .commit_with_keyprovider(&self.snapshot_path, &self.keyprovider)
            .map_err(|e| AppError::CredentialError(format!("Failed to commit snapshot: {}", e)))?;

        Ok(())
    }

    fn get_or_create_client(&self) -> Result<Client, AppError> {
        match self.stronghold.get_client(CLIENT_PATH) {
            Ok(client) => Ok(client),
            Err(_) => {
                self.stronghold
                    .create_client(CLIENT_PATH)
                    .map_err(|e| AppError::CredentialError(format!("Failed to create client: {}", e)))
            }
        }
    }

    pub fn save_credentials(&mut self, broker_id: &str, creds: &StoredCredentials) -> Result<(), AppError> {
        self.data.credentials.insert(broker_id.to_string(), creds.clone());
        self.save()
    }

    pub fn load_credentials(&self, broker_id: &str) -> Result<StoredCredentials, AppError> {
        self.data
            .credentials
            .get(broker_id)
            .cloned()
            .ok_or_else(|| AppError::CredentialError(format!("No credentials for broker: {}", broker_id)))
    }

    pub fn load_all_credentials(&self) -> Result<HashMap<String, StoredCredentials>, AppError> {
        Ok(self.data.credentials.clone())
    }

    pub fn delete_credentials(&mut self, broker_id: &str) -> Result<(), AppError> {
        self.data.credentials.remove(broker_id);
        self.save()
    }

    pub fn has_any_credentials(&self) -> bool {
        !self.data.credentials.is_empty()
    }

    pub fn get_last_active_broker_id(&self) -> &str {
        &self.data.last_active
    }

    pub fn set_last_active_broker_id(&mut self, broker_id: &str) -> Result<(), AppError> {
        self.data.last_active = broker_id.to_string();
        self.save()
    }
}

/// Fast machine-specific identifier using env vars (instant, no subprocess).
/// Only needs to be consistent across restarts on the same machine — does NOT
/// need to match Go's format (that's handled separately in migration.rs).
fn get_store_identifier() -> String {
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "default-host".to_string());
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "default-user".to_string());
    format!("{}{}", hostname, username)
}

impl Default for StrongholdStore {
    fn default() -> Self {
        Self {
            stronghold: Stronghold::default(),
            keyprovider: KeyProvider::try_from(Zeroizing::new(vec![0u8; KEY_LEN]))
                .expect("Failed to create default key provider"),
            snapshot_path: SnapshotPath::from_path(""),
            data: StoredData::default(),
        }
    }
}
