use tauri::{AppHandle, Emitter, State};

use crate::broker::{self, BrokerType, EventEmitter};
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

fn create_broker(broker_type: BrokerType, id: &str) -> Box<dyn broker::Broker> {
    let opt_id = if id.is_empty() { None } else { Some(id.to_string()) };
    match broker_type {
        BrokerType::Fennel => Box::new(FennelBroker::new(id.to_string())),
        BrokerType::Public => Box::new(PublicBroker::new(opt_id.clone())),
        BrokerType::Robinhood => Box::new(RobinhoodBroker::new(id)),
        BrokerType::Tastytrade => Box::new(TastytradeBroker::new(opt_id)),
        BrokerType::Webull => Box::new(WebullBroker::new(id.to_string())),
        BrokerType::Lighthorse => Box::new(LighthorseBroker::new(opt_id)),
    }
}

fn emit_broker_log(app: &AppHandle, broker_id: &str, broker_type: &str, message: impl Into<String>) {
    let _ = app.emit(
        events::EVENT_LOG,
        serde_json::json!({
            "message": message.into(),
            "brokerId": broker_id,
            "brokerType": broker_type,
        }),
    );
}

#[tauri::command]
pub async fn start_2fa(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    broker_type: Option<String>,
) -> Result<(), AppError> {
    log::info!("start_2fa: email={}, broker_type={:?}", email, broker_type);

    let mut manager = state.broker_manager.write().await;

    if let Some(ref bt_str) = broker_type {
        let bt = BrokerType::from_str(bt_str)?;

        // Look for existing broker of this type that's not logged in
        let existing_id = manager
            .get_all_brokers()
            .iter()
            .find(|b| b.get_type() == bt && !b.is_logged_in())
            .map(|b| b.get_id().to_string());

        let target_id = if let Some(id) = existing_id {
            id
        } else {
            // Create a new broker
            let mut broker = create_broker(bt, "");
            broker.set_event_emitter(create_event_emitter(&app));
            let id = broker.get_id().to_string();
            manager.register_broker(broker)?;
            id
        };

        manager.set_active_broker(&target_id)?;
    } else if manager.get_active_broker().is_none() {
        // Legacy fallback: create a Fennel broker
        let mut broker = create_broker(BrokerType::Fennel, "");
        broker.set_event_emitter(create_event_emitter(&app));
        let id = broker.get_id().to_string();
        manager.register_broker(broker)?;
        manager.set_active_broker(&id)?;
    }

    // Get the active broker and call start_2fa
    let active_id = manager
        .get_active_broker_id()
        .map(|s| s.to_string())
        .ok_or(AppError::NoBrokerConnected)?;

    let broker = manager.get_broker_mut(&active_id)?;
    let active_broker_id = broker.get_id().to_string();
    let active_broker_type = broker.get_type().to_string();
    match broker.start_2fa(&email).await {
        Ok(()) => {
            emit_broker_log(
                &app,
                &active_broker_id,
                &active_broker_type,
                "2FA initiated successfully. Check your email for the OTP.",
            );
            let _ = app.emit(events::EVENT_2FA_STARTED, &email);
            Ok(())
        }
        Err(e) => {
            let msg = format!("Failed to start 2FA: {}", e);
            emit_broker_log(
                &app,
                &active_broker_id,
                &active_broker_type,
                format!("Error starting 2FA: {}", e),
            );
            let _ = app.emit(events::EVENT_LOGIN_FAILURE, &msg);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn login(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
    email: String,
) -> Result<(), AppError> {
    log::info!("login: email={}", email);

    let mut manager = state.broker_manager.write().await;
    let active_id = manager
        .get_active_broker_id()
        .map(|s| s.to_string())
        .ok_or(AppError::NoBrokerConnected)?;

    let broker = manager.get_broker_mut(&active_id)?;
    let active_broker_id = broker.get_id().to_string();
    let active_broker_type = broker.get_type().to_string();

    if broker.is_logged_in() {
        emit_broker_log(
            &app,
            &active_broker_id,
            &active_broker_type,
            "Already logged in.",
        );
        return Ok(());
    }

    match broker.login(&code, &email).await {
        Ok(()) => {
            log::info!("Login successful, saving credentials...");

            // Save credentials
            let creds = broker.export_credentials().ok();
            let broker_id = broker.get_id().to_string();
            let broker_type = broker.get_type().to_string();
            let broker_email = broker.get_current_email().to_string();

            if let Some(ref creds) = creds {
                let mut store = state.credential_store.write().await;
                if let Err(e) = store.save_credentials(&broker_id, creds) {
                    log::warn!("Failed to save credentials: {}", e);
                } else {
                    let _ = store.set_last_active_broker_id(&broker_id);
                }
            }

            emit_broker_log(&app, &broker_id, &broker_type, "Logged in successfully");
            let _ = app.emit(events::EVENT_LOGIN_SUCCESS, ());
            let _ = app.emit(
                events::EVENT_BROKER_LINKED,
                serde_json::json!({
                    "id": broker_id,
                    "type": broker_type,
                    "email": broker_email,
                    "status": "connected",
                }),
            );
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            emit_broker_log(
                &app,
                &active_broker_id,
                &active_broker_type,
                format!("Login error: {}", msg),
            );
            let _ = app.emit(events::EVENT_LOGIN_FAILURE, &msg);
            Err(e)
        }
    }
}
