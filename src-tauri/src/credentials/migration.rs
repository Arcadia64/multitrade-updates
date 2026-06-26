use std::path::PathBuf;

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

use super::StoredData;

const LEGACY_SALT: &[u8] = b"FennelTraderCredentialsSalt2025";
const PBKDF2_ITERATIONS: u32 = 100_000;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Returns the path to the legacy Go (Fennel Trader) credential file.
fn legacy_credentials_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|appdata| {
            PathBuf::from(appdata)
                .join("Fennel")
                .join("Fennel Trader")
                .join("config")
                .join("credentials.enc")
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Legacy Go app only ran on Windows
        None
    }
}

/// Gets the machine-specific identifier matching Go's construction:
/// `os.Hostname() + user.Current().Username + user.Current().Uid`
///
/// On Windows:
/// - Hostname: from `hostname` command (matches Go's os.Hostname — DNS hostname, case-preserving)
/// - Username: USERDOMAIN\USERNAME env vars (preserves original case, unlike `whoami` which lowercases)
/// - Uid: SID from `whoami.exe /user` (SID is case-insensitive, always same format)
pub fn get_machine_identifier() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        // Get hostname via command (matches Go's GetComputerNameExW DNS hostname)
        let hostname_out = std::process::Command::new("hostname")
            .output()
            .map_err(|e| format!("Failed to run hostname: {}", e))?;
        let hostname = String::from_utf8_lossy(&hostname_out.stdout).trim().to_string();

        // Get username from env vars — these preserve the original case,
        // unlike `whoami` which lowercases everything.
        // Go's user.Current().Username returns "DOMAIN\Username" from LookupAccountSid.
        let domain = std::env::var("USERDOMAIN").unwrap_or_default();
        let user = std::env::var("USERNAME").unwrap_or_default();
        let username = format!("{}\\{}", domain, user);

        // Get SID from whoami.exe (must use full path to avoid bash's whoami)
        let whoami_out = std::process::Command::new("C:\\Windows\\System32\\whoami.exe")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .map_err(|e| format!("Failed to run whoami.exe: {}", e))?;

        let whoami_str = String::from_utf8_lossy(&whoami_out.stdout);
        let line = whoami_str.trim();

        // Parse CSV: "DOMAIN\user","SID" — we only need the SID (second field)
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            return Err(format!("Unexpected whoami output: {}", line));
        }
        let uid = parts[1].trim_matches('"');

        log::info!(
            "Legacy migration identifier parts: hostname={}, username={}, uid={}",
            hostname,
            username,
            uid
        );

        // Go's identifier: hostname + currentUser.Username + currentUser.Uid (no separators)
        Ok(format!("{}{}{}", hostname, username, uid))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Legacy credential migration is only supported on Windows".to_string())
    }
}

/// Derives the AES-256 encryption key using PBKDF2-HMAC-SHA256,
/// matching Go's `deriveEncryptionKey()` exactly.
fn derive_key(identifier: &str) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(
        identifier.as_bytes(),
        LEGACY_SALT,
        PBKDF2_ITERATIONS,
        &mut key,
    );
    key
}

/// Decrypts AES-256-GCM ciphertext where nonce is prepended (first 12 bytes).
/// Matches Go's `decrypt()` function.
fn decrypt_legacy(data: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_LEN {
        return Err("Ciphertext too short".to_string());
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Failed to decrypt: {}", e))
}

/// Checks whether legacy Go (Fennel Trader) credentials exist on this machine.
pub fn check_legacy_credentials() -> bool {
    legacy_credentials_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Imports and decrypts legacy Go (Fennel Trader) credentials.
/// Returns the parsed StoredData with all broker credentials.
pub fn import_legacy_credentials() -> Result<StoredData, String> {
    let path = legacy_credentials_path()
        .ok_or("Could not determine legacy credentials path")?;

    if !path.exists() {
        return Err("Legacy credentials file not found".to_string());
    }

    log::info!("Reading legacy credentials from: {}", path.display());

    let encrypted_data = std::fs::read(&path)
        .map_err(|e| format!("Failed to read legacy credentials file: {}", e))?;

    log::info!("Legacy file size: {} bytes", encrypted_data.len());

    let identifier = get_machine_identifier()?;
    let key = derive_key(&identifier);

    let plaintext = decrypt_legacy(&encrypted_data, &key)?;

    log::info!("Legacy credentials decrypted successfully ({} bytes)", plaintext.len());

    let data: StoredData = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("Failed to parse legacy credentials JSON: {}", e))?;

    log::info!(
        "Imported {} broker credentials from legacy store",
        data.credentials.len()
    );

    Ok(data)
}
