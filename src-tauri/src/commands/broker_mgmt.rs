use tauri::{AppHandle, Emitter, State};

use crate::broker::{self, BrokerInfo, BrokerType, EventEmitter};
use crate::broker::fennel::FennelBroker;
use crate::broker::public_invest::PublicBroker;
use crate::broker::robinhood::RobinhoodBroker;
use crate::broker::tastytrade::TastytradeBroker;
use crate::broker::webull::WebullBroker;
use crate::broker::lighthorse::LighthorseBroker;
use crate::error::AppError;
use crate::events;
use crate::state::AppState;
use std::sync::Arc;

fn create_event_emitter(app: &AppHandle) -> EventEmitter {
    let app_handle = app.clone();
    Arc::new(move |event: &str, data: serde_json::Value| {
        let _ = app_handle.emit(event, data);
    })
}

#[tauri::command]
pub async fn frontend_ready(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    log::info!("Frontend is ready to receive events");

    let store = state.credential_store.read().await;
    if !store.has_any_credentials() {
        // Check for legacy Go (Fennel Trader) credentials to import
        if crate::credentials::migration::check_legacy_credentials() {
            log::info!("Found legacy Fennel Trader credentials, prompting for import");
            let _ = app.emit(
                events::EVENT_STARTUP_COMPLETE,
                serde_json::json!({
                    "state": "legacy_found",
                    "linkedBrokers": [],
                    "activeBroker": null,
                }),
            );
            return Ok(());
        }

        log::info!("No cached credentials found, showing link broker UI");
        let _ = app.emit(
            events::EVENT_STARTUP_COMPLETE,
            serde_json::json!({
                "state": "need_link",
                "linkedBrokers": [],
                "activeBroker": null,
            }),
        );
        return Ok(());
    }

    log::info!("Found cached credentials, attempting to restore sessions...");
    let all_creds = store.load_all_credentials()?;
    drop(store); // Release read lock before acquiring write lock

    let mut manager = state.broker_manager.write().await;
    let mut linked_brokers: Vec<serde_json::Value> = Vec::new();
    let mut needs_reauth: Vec<String> = Vec::new();

    for (broker_id, creds) in &all_creds {
        log::info!("Restoring broker: {} type: {}", broker_id, creds.broker_type);

        let bt = match BrokerType::from_str(&creds.broker_type) {
            Ok(bt) => bt,
            Err(_) => {
                log::warn!("Unknown broker type: {}", creds.broker_type);
                continue;
            }
        };

        let bid = broker_id.to_string();
        let opt_bid = Some(bid.clone());
        let mut broker: Box<dyn broker::Broker> = match bt {
            BrokerType::Fennel => Box::new(FennelBroker::new(bid)),
            BrokerType::Public => Box::new(PublicBroker::new(opt_bid.clone())),
            BrokerType::Robinhood => Box::new(RobinhoodBroker::new(broker_id)),
            BrokerType::Tastytrade => Box::new(TastytradeBroker::new(opt_bid)),
            BrokerType::Webull => Box::new(WebullBroker::new(bid.clone())),
            BrokerType::Lighthorse => Box::new(LighthorseBroker::new(opt_bid)),
        };

        broker.set_event_emitter(create_event_emitter(&app));

        match broker.login_with_stored_credentials(creds).await {
            Ok(()) => {
                log::info!("Session restored for {}", broker_id);

                // Update stored credentials with potentially refreshed tokens
                if let Ok(new_creds) = broker.export_credentials() {
                    let mut store = state.credential_store.write().await;
                    let _ = store.save_credentials(broker_id, &new_creds);
                }

                linked_brokers.push(serde_json::json!({
                    "id": broker_id,
                    "type": creds.broker_type,
                    "email": creds.email,
                    "status": "connected",
                }));
            }
            Err(e) => {
                log::warn!("Failed to restore session for {}: {}", broker_id, e);
                needs_reauth.push(broker_id.clone());
                linked_brokers.push(serde_json::json!({
                    "id": broker_id,
                    "type": creds.broker_type,
                    "email": creds.email,
                    "status": "needs_reauth",
                }));
            }
        }

        let _ = manager.register_broker(broker);
    }

    // Set the last active broker
    let store = state.credential_store.read().await;
    let last_active_id = store.get_last_active_broker_id().to_string();
    drop(store);

    if !last_active_id.is_empty() {
        if let Err(e) = manager.set_active_broker(&last_active_id) {
            log::warn!("Failed to set last active broker: {}", e);
        }
    }

    let active_broker_info = manager.get_active_broker().and_then(|b| {
        if b.is_logged_in() {
            Some(serde_json::json!({
                "id": b.get_id(),
                "type": b.get_type().to_string(),
                "email": b.get_current_email(),
            }))
        } else {
            None
        }
    });

    let state_str = if active_broker_info.is_some() {
        "ready"
    } else {
        "need_link"
    };

    let _ = app.emit(
        events::EVENT_STARTUP_COMPLETE,
        serde_json::json!({
            "state": state_str,
            "linkedBrokers": linked_brokers,
            "activeBroker": active_broker_info,
            "needsReauth": needs_reauth,
        }),
    );

    if active_broker_info.is_some() {
        let _ = app.emit(events::EVENT_LOGIN_SUCCESS, ());
    }

    Ok(())
}

#[tauri::command]
pub async fn link_broker(
    app: AppHandle,
    state: State<'_, AppState>,
    broker_type: String,
) -> Result<(), AppError> {
    log::info!("link_broker: type={}", broker_type);

    let bt = BrokerType::from_str(&broker_type)?;
    let mut manager = state.broker_manager.write().await;

    // Check if there's already a broker of this type and remove it
    let existing_id = manager
        .get_all_brokers()
        .iter()
        .find(|b| b.get_type() == bt)
        .map(|b| b.get_id().to_string());

    // Preserve device token for Robinhood reauth
    let mut saved_device_token = String::new();
    if let Some(ref eid) = existing_id {
        let store = state.credential_store.read().await;
        if let Ok(old_creds) = store.load_credentials(eid) {
            saved_device_token = old_creds.device_token;
        }
    }

    // Remove existing broker
    if let Some(ref eid) = existing_id {
        let _ = manager.remove_broker(eid);
        let mut store = state.credential_store.write().await;
        let _ = store.delete_credentials(eid);
    }

    let mut broker: Box<dyn broker::Broker> = match bt {
        BrokerType::Fennel => Box::new(FennelBroker::new(String::new())),
        BrokerType::Public => Box::new(PublicBroker::new(None)),
        BrokerType::Robinhood => {
            let mut rb = RobinhoodBroker::new("");
            if !saved_device_token.is_empty() {
                rb.set_device_token(&saved_device_token);
            }
            Box::new(rb)
        }
        BrokerType::Tastytrade => Box::new(TastytradeBroker::new(None)),
        BrokerType::Webull => Box::new(WebullBroker::new(String::new())),
        BrokerType::Lighthorse => Box::new(LighthorseBroker::new(None)),
    };

    broker.set_event_emitter(create_event_emitter(&app));
    let id = broker.get_id().to_string();
    manager.register_broker(broker)?;
    manager.set_active_broker(&id)?;

    let _ = app.emit(
        events::EVENT_LINK_BROKER_READY,
        serde_json::json!({
            "id": id,
            "type": broker_type,
        }),
    );

    Ok(())
}

#[tauri::command]
pub async fn select_broker(
    app: AppHandle,
    state: State<'_, AppState>,
    broker_id: String,
) -> Result<(), AppError> {
    log::info!("select_broker: id={}", broker_id);

    let mut manager = state.broker_manager.write().await;
    manager.set_active_broker(&broker_id)?;

    let mut store = state.credential_store.write().await;
    let _ = store.set_last_active_broker_id(&broker_id);
    drop(store);

    if let Some(b) = manager.get_active_broker() {
        let _ = app.emit(
            events::EVENT_BROKER_SELECTED,
            serde_json::json!({
                "id": b.get_id(),
                "type": b.get_type().to_string(),
                "email": b.get_current_email(),
            }),
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn unlink_broker(
    app: AppHandle,
    state: State<'_, AppState>,
    broker_id: String,
) -> Result<(), AppError> {
    log::info!("unlink_broker: id={}", broker_id);

    let mut manager = state.broker_manager.write().await;

    // Get the broker and logout
    {
        let broker = manager.get_broker_mut(&broker_id)?;
        broker.logout();
    }

    // Remove from manager
    manager.remove_broker(&broker_id)?;

    // Remove credentials
    let mut store = state.credential_store.write().await;
    let _ = store.delete_credentials(&broker_id);

    let _ = app.emit(events::EVENT_BROKER_UNLINKED, &broker_id);

    Ok(())
}

#[tauri::command]
pub async fn get_linked_brokers(
    state: State<'_, AppState>,
) -> Result<Vec<BrokerInfo>, AppError> {
    let manager = state.broker_manager.read().await;
    Ok(manager.get_linked_brokers())
}

#[tauri::command]
pub async fn import_legacy_credentials(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    log::info!("Importing legacy Fennel Trader credentials...");

    let legacy_data = crate::credentials::migration::import_legacy_credentials()
        .map_err(|e| {
            log::error!("Legacy import failed: {}", e);
            AppError::CredentialError(format!("Failed to import legacy credentials: {}", e))
        })?;

    // Save each imported credential to the Rust credential store
    let mut store = state.credential_store.write().await;
    let mut imported_count = 0;

    for (broker_id, creds) in &legacy_data.credentials {
        log::info!(
            "Importing broker: {} (type: {}, email: {})",
            broker_id,
            creds.broker_type,
            creds.email
        );
        if let Err(e) = store.save_credentials(broker_id, creds) {
            log::warn!("Failed to save imported credentials for {}: {}", broker_id, e);
        } else {
            imported_count += 1;
        }
    }

    // Set last active broker if available
    if !legacy_data.last_active.is_empty() {
        let _ = store.set_last_active_broker_id(&legacy_data.last_active);
    }
    drop(store);

    log::info!("Successfully imported {} broker credentials", imported_count);

    let _ = app.emit(
        "legacy_import_complete",
        serde_json::json!({
            "imported": imported_count,
        }),
    );

    Ok(())
}

#[tauri::command]
pub async fn skip_legacy_import(
    app: AppHandle,
) -> Result<(), AppError> {
    log::info!("User skipped legacy credential import");

    let _ = app.emit(
        events::EVENT_STARTUP_COMPLETE,
        serde_json::json!({
            "state": "need_link",
            "linkedBrokers": [],
            "activeBroker": null,
        }),
    );

    Ok(())
}
