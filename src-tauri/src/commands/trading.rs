use std::collections::HashMap;
use tauri::State;

use crate::error::AppError;
use crate::state::AppState;

fn broker_order_priority(broker_type: &str) -> i32 {
    match broker_type {
        "robinhood" => 98,
        "fennel" => 99,
        _ => 0,
    }
}

#[tauri::command]
pub async fn place_order_multi_broker(
    state: State<'_, AppState>,
    ticker: String,
    side: String,
    shares: f64,
    accounts_by_broker: HashMap<String, String>,
    sell_max: Option<bool>,
) -> Result<HashMap<String, String>, AppError> {
    let manager = state.broker_manager.read().await;
    let mut results: HashMap<String, String> = HashMap::new();
    let sell_max = sell_max.unwrap_or(false);

    // Sort broker IDs: other brokers first, then Robinhood, then Fennel
    let mut broker_ids: Vec<String> = accounts_by_broker.keys().cloned().collect();
    broker_ids.sort_by(|a, b| {
        let a_type = manager
            .get_broker(a)
            .map(|br| br.get_type().to_string())
            .unwrap_or_default();
        let b_type = manager
            .get_broker(b)
            .map(|br| br.get_type().to_string())
            .unwrap_or_default();
        broker_order_priority(&a_type).cmp(&broker_order_priority(&b_type))
    });

    for broker_id in &broker_ids {
        let accounts = match accounts_by_broker.get(broker_id) {
            Some(a) => a.clone(),
            None => continue,
        };

        let broker = match manager.get_broker(broker_id) {
            Ok(b) => b,
            Err(e) => {
                results.insert(broker_id.clone(), format!("Error: {}", e));
                continue;
            }
        };

        if !broker.is_logged_in() {
            results.insert(broker_id.clone(), "Error: Broker not logged in".to_string());
            continue;
        }

        let result = broker.place_order(&ticker, &side, shares, &accounts, sell_max).await;
        results.insert(broker_id.clone(), result);
    }

    Ok(results)
}
