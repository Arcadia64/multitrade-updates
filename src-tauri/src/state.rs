use std::sync::Arc;
use tokio::sync::RwLock;

use crate::broker::manager::BrokerManager;
use crate::credentials::store::StrongholdStore;

/// Central application state managed by Tauri.
/// All Tauri commands receive this via State<AppState>.
pub struct AppState {
    pub broker_manager: Arc<RwLock<BrokerManager>>,
    pub credential_store: Arc<RwLock<StrongholdStore>>,
}
