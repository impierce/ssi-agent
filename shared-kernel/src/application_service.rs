use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

/// A generic application service that can handle any context implementing `ApplicationContext`.
#[async_trait]
pub trait ApplicationContext: Send + Sync + 'static {
    // Inputs/Outputs
    type Command: Send;
    type Query: Send;
    type View: Send;

    // Errors
    type CommandError: std::error::Error + Send;
    type QueryError: std::error::Error + Send;

    async fn handle_command(&self, aggregate_id: &str, command: Self::Command) -> Result<String, Self::CommandError>;
    async fn handle_query(&self, query: Self::Query) -> Result<Self::View, Self::QueryError>;
}

pub struct CommandEnvelope<AC: ApplicationContext> {
    pub id: String,
    pub command: AC::Command,
    pub reply: oneshot::Sender<Result<String, AC::CommandError>>,
}

pub struct QueryEnvelope<AC: ApplicationContext> {
    pub query: AC::Query,
    pub reply: oneshot::Sender<Result<AC::View, AC::QueryError>>,
}

pub struct ApplicationService<AC: ApplicationContext> {
    context: AC,
    command_rx: mpsc::Receiver<CommandEnvelope<AC>>,
    query_rx: mpsc::Receiver<QueryEnvelope<AC>>,
}

impl<AC: ApplicationContext> ApplicationService<AC> {
    pub fn new(
        context: AC,
        command_rx: mpsc::Receiver<CommandEnvelope<AC>>,
        query_rx: mpsc::Receiver<QueryEnvelope<AC>>,
    ) -> Self {
        Self {
            context,
            command_rx,
            query_rx,
        }
    }

    pub async fn start(mut self) {
        loop {
            // Using select! allows us to listen to both channels simultaneously
            // while maintaining single-threaded safety for 'context'
            tokio::select! {
                Some(msg) = self.command_rx.recv() => {
                    self.process_command(msg).await;
                }
                Some(msg) = self.query_rx.recv() => {
                    self.process_query(msg).await;
                }
                else => break, // Exit if channels close
            }
        }
    }

    // Helper to keep the loop clean
    async fn process_command(&mut self, msg: CommandEnvelope<AC>) {
        let result = self.context.handle_command(&msg.id, msg.command).await;
        // Logging...
        let _ = msg.reply.send(result);
    }

    async fn process_query(&mut self, msg: QueryEnvelope<AC>) {
        let result = self.context.handle_query(msg.query).await;
        // Logging...
        let _ = msg.reply.send(result);
    }
}
