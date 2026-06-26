use tauri::State;

use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn get_all_broker_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, AppError> {
    let manager = state.broker_manager.read().await;
    let all_brokers = manager.get_all_brokers();

    if all_brokers.is_empty() {
        return Err(AppError::NoBrokerConnected);
    }

    let mut all_accounts: Vec<serde_json::Value> = Vec::new();

    for b in all_brokers {
        if !b.is_logged_in() {
            continue;
        }

        match b.get_accounts().await {
            Ok(accounts) => {
                for mut acc in accounts {
                    if let Some(obj) = acc.as_object_mut() {
                        obj.insert("brokerId".to_string(), serde_json::json!(b.get_id()));
                        obj.insert("brokerType".to_string(), serde_json::json!(b.get_type().to_string()));
                    }
                    all_accounts.push(acc);
                }
            }
            Err(e) => {
                log::warn!("Error getting accounts from broker {}: {}", b.get_id(), e);
            }
        }
    }

    Ok(all_accounts)
}

#[tauri::command]
pub async fn get_account_holdings(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<serde_json::Value>, AppError> {
    let manager = state.broker_manager.read().await;
    let broker = manager.get_active_broker().ok_or(AppError::NoBrokerConnected)?;
    broker.get_account_holdings(&account_id).await
}

#[tauri::command]
pub async fn get_account_cash(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<serde_json::Value, AppError> {
    let manager = state.broker_manager.read().await;
    let broker = manager.get_active_broker().ok_or(AppError::NoBrokerConnected)?;
    broker.get_account_cash(&account_id).await
}

#[tauri::command]
pub async fn get_broker_account_holdings(
    state: State<'_, AppState>,
    broker_id: String,
    account_id: String,
) -> Result<Vec<serde_json::Value>, AppError> {
    let manager = state.broker_manager.read().await;
    let broker = manager.get_broker(&broker_id)?;
    if !broker.is_logged_in() {
        return Err(AppError::BrokerNotLoggedIn);
    }
    broker.get_account_holdings(&account_id).await
}

#[tauri::command]
pub async fn get_broker_account_cash(
    state: State<'_, AppState>,
    broker_id: String,
    account_id: String,
) -> Result<serde_json::Value, AppError> {
    let manager = state.broker_manager.read().await;
    let broker = manager.get_broker(&broker_id)?;
    if !broker.is_logged_in() {
        return Err(AppError::BrokerNotLoggedIn);
    }
    broker.get_account_cash(&account_id).await
}
