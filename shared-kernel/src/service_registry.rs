use std::any::{Any, TypeId};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

use crate::application_service::{ApplicationContext, CommandEnvelope, QueryEnvelope};

// Completely generic container - no knowledge of specific service types
pub struct ServiceRegistry(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl ServiceRegistry {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    // Register any service without knowing its type ahead of time
    pub fn register<T: 'static + Send + Sync>(&mut self, service: T) -> &mut Self {
        self.0.insert(TypeId::of::<T>(), Box::new(service));
        self
    }

    // Generic service getter
    pub fn get_handle<T: 'static + Clone>(&self) -> Option<T> {
        self.0
            .get(&TypeId::of::<T>())
            .and_then(|s| s.downcast_ref::<T>())
            .cloned()
    }
}

pub struct ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    pub command_tx: mpsc::Sender<CommandEnvelope<AC>>,
    pub query_tx: mpsc::Sender<QueryEnvelope<AC>>,
}

impl<AC> ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    pub fn new(command_tx: mpsc::Sender<CommandEnvelope<AC>>, query_tx: mpsc::Sender<QueryEnvelope<AC>>) -> Self {
        Self { command_tx, query_tx }
    }

    pub async fn dispatch_command(
        &self,
        aggregate_id: String,
        command: AC::Command,
    ) -> Result<String, AC::CommandError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let msg = CommandEnvelope {
            id: aggregate_id,
            command,
            reply: reply_tx,
        };

        self.command_tx.send(msg).await.unwrap();

        // In a real app, map this Result<String, String> to a proper HTTP status code
        reply_rx.await.unwrap()
    }

    pub async fn dispatch_query(&self, query: AC::Query) -> Result<AC::View, AC::QueryError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let msg = QueryEnvelope { query, reply: reply_tx };

        self.query_tx.send(msg).await.unwrap();

        reply_rx.await.unwrap()
    }
}

impl<AC> Clone for ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    fn clone(&self) -> Self {
        Self {
            command_tx: self.command_tx.clone(),
            query_tx: self.query_tx.clone(),
        }
    }
}
