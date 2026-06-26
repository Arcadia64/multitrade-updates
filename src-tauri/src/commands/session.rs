use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::events;
use crate::state::AppState;

#[tauri::command]
pub async fn is_logged_in(state: State<'_, AppState>) -> Result<bool, AppError> {
    let manager = state.broker_manager.read().await;
    Ok(manager
        .get_active_broker()
        .map(|b| b.is_logged_in())
        .unwrap_or(false))
}

#[tauri::command]
pub async fn get_current_email(state: State<'_, AppState>) -> Result<String, AppError> {
    let manager = state.broker_manager.read().await;
    Ok(manager
        .get_active_broker()
        .map(|b| b.get_current_email().to_string())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn get_login_time(state: State<'_, AppState>) -> Result<String, AppError> {
    let manager = state.broker_manager.read().await;
    Ok(manager
        .get_active_broker()
        .map(|b| b.get_login_time())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn logout(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let mut manager = state.broker_manager.write().await;

    if let Some(broker) = manager.get_active_broker_mut() {
        let broker_id = broker.get_id().to_string();
        broker.logout();

        // Remove credentials
        let mut store = state.credential_store.write().await;
        let _ = store.delete_credentials(&broker_id);
        drop(store);

        // Remove from manager
        let _ = manager.remove_broker(&broker_id);
    }

    let _ = app.emit(events::EVENT_LOGOUT_SUCCESS, ());
    Ok(())
}

#[tauri::command]
pub async fn check_token_validity(state: State<'_, AppState>) -> Result<bool, AppError> {
    let mut manager = state.broker_manager.write().await;
    match manager.get_active_broker_mut() {
        Some(b) => Ok(b.check_token_validity().await),
        None => Ok(false),
    }
}
