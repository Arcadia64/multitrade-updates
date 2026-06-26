use std::collections::HashMap;

use crate::broker::{Broker, BrokerInfo, EventEmitter};
use crate::error::AppError;

/// BrokerManager manages multiple broker instances.
/// Thread safety is provided by wrapping this in Arc<RwLock<>> at the AppState level.
pub struct BrokerManager {
    brokers: HashMap<String, Box<dyn Broker>>,
    active_broker_id: Option<String>,
    event_emitter: Option<EventEmitter>,
}

impl BrokerManager {
    pub fn new() -> Self {
        Self {
            brokers: HashMap::new(),
            active_broker_id: None,
            event_emitter: None,
        }
    }

    #[allow(dead_code)]
    pub fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter.clone());
        for broker in self.brokers.values_mut() {
            broker.set_event_emitter(emitter.clone());
        }
    }

    pub fn register_broker(&mut self, mut broker: Box<dyn Broker>) -> Result<(), AppError> {
        let id = broker.get_id().to_string();
        if id.is_empty() {
            return Err(AppError::Other("Broker ID cannot be empty".to_string()));
        }

        if let Some(ref emitter) = self.event_emitter {
            broker.set_event_emitter(emitter.clone());
        }

        // If this is the first broker, make it active
        if self.active_broker_id.is_none() {
            self.active_broker_id = Some(id.clone());
        }

        self.brokers.insert(id, broker);
        Ok(())
    }

    pub fn remove_broker(&mut self, broker_id: &str) -> Result<(), AppError> {
        if !self.brokers.contains_key(broker_id) {
            return Err(AppError::BrokerNotFound(broker_id.to_string()));
        }

        self.brokers.remove(broker_id);

        // If we removed the active broker, pick a new one
        if self.active_broker_id.as_deref() == Some(broker_id) {
            self.active_broker_id = self.brokers.keys().next().map(|k| k.clone());
        }

        Ok(())
    }

    pub fn get_broker(&self, broker_id: &str) -> Result<&dyn Broker, AppError> {
        self.brokers
            .get(broker_id)
            .map(|b| b.as_ref())
            .ok_or_else(|| AppError::BrokerNotFound(broker_id.to_string()))
    }

    pub fn get_broker_mut(&mut self, broker_id: &str) -> Result<&mut Box<dyn Broker>, AppError> {
        self.brokers
            .get_mut(broker_id)
            .ok_or_else(|| AppError::BrokerNotFound(broker_id.to_string()))
    }

    pub fn get_active_broker(&self) -> Option<&dyn Broker> {
        self.active_broker_id
            .as_ref()
            .and_then(|id| self.brokers.get(id))
            .map(|b| b.as_ref())
    }

    pub fn get_active_broker_mut(&mut self) -> Option<&mut Box<dyn Broker>> {
        if let Some(ref id) = self.active_broker_id {
            self.brokers.get_mut(id)
        } else {
            None
        }
    }

    pub fn get_active_broker_id(&self) -> Option<&str> {
        self.active_broker_id.as_deref()
    }

    pub fn set_active_broker(&mut self, broker_id: &str) -> Result<(), AppError> {
        if !self.brokers.contains_key(broker_id) {
            return Err(AppError::BrokerNotFound(broker_id.to_string()));
        }
        self.active_broker_id = Some(broker_id.to_string());
        Ok(())
    }

    pub fn get_all_brokers(&self) -> Vec<&dyn Broker> {
        self.brokers.values().map(|b| b.as_ref()).collect()
    }

    #[allow(dead_code)]
    pub fn get_all_brokers_mut(&mut self) -> Vec<&mut Box<dyn Broker>> {
        self.brokers.values_mut().collect()
    }

    pub fn get_linked_brokers(&self) -> Vec<BrokerInfo> {
        self.brokers
            .iter()
            .map(|(id, b)| {
                let status = if b.is_logged_in() {
                    "connected"
                } else {
                    "disconnected"
                };
                BrokerInfo {
                    id: id.clone(),
                    broker_type: b.get_type(),
                    name: b.get_name().to_string(),
                    email: b.get_current_email().to_string(),
                    status: status.to_string(),
                }
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn has_brokers(&self) -> bool {
        !self.brokers.is_empty()
    }

    #[allow(dead_code)]
    pub fn clear_all(&mut self) {
        for broker in self.brokers.values_mut() {
            broker.logout();
        }
        self.brokers.clear();
        self.active_broker_id = None;
    }
}
