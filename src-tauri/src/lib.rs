mod broker;
mod commands;
mod credentials;
mod error;
mod events;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()
                .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;

            log::info!("App data directory: {}", data_dir.display());

            let credential_store = credentials::store::StrongholdStore::new(data_dir)
                .unwrap_or_else(|e| {
                    log::warn!("Failed to initialize credential store: {}", e);
                    credentials::store::StrongholdStore::default()
                });

            app.manage(AppState {
                broker_manager: Arc::new(RwLock::new(broker::manager::BrokerManager::new())),
                credential_store: Arc::new(RwLock::new(credential_store)),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::start_2fa,
            commands::auth::login,
            commands::broker_mgmt::frontend_ready,
            commands::broker_mgmt::link_broker,
            commands::broker_mgmt::select_broker,
            commands::broker_mgmt::unlink_broker,
            commands::broker_mgmt::get_linked_brokers,
            commands::broker_mgmt::import_legacy_credentials,
            commands::broker_mgmt::skip_legacy_import,
            commands::accounts::get_all_broker_accounts,
            commands::accounts::get_account_holdings,
            commands::accounts::get_account_cash,
            commands::accounts::get_broker_account_holdings,
            commands::accounts::get_broker_account_cash,
            commands::trading::place_order_multi_broker,
            commands::session::is_logged_in,
            commands::session::get_current_email,
            commands::session::get_login_time,
            commands::session::logout,
            commands::session::check_token_validity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
