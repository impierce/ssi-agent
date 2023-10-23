use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct InMemory {
    store: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl InMemory {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn append(&self, value: serde_json::Value) {
        let mut store = self.store.lock().unwrap();
        store.push(value);
    }

    pub fn get_all(&self) -> Vec<serde_json::Value> {
        let store = self.store.lock().unwrap();
        store.clone()
    }
}
