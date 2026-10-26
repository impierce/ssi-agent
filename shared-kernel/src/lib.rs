//! Shared kernel for the SSI Agent.
//!
//! Provides infrastructure used by all bounded contexts:
//!
//! - [`application_service`] — Actor-style service that serialises command/query processing.
//! - [`authorization`] — Shared authorization primitives for application command/query execution.
//! - [`command_handler`] — Traits and helpers for dispatching commands to aggregates.
//! - [`custom_queries`] — Reusable CQRS query types such as [`custom_queries::ListAllQuery`].
//! - [`view_repository`] — Dyn-compatible view repository wrappers and factory trait.
//! - [`service_registry`] — Type-keyed service registry and channel-based service handles.

extern crate self as shared_kernel;

pub mod application_service;
pub mod authorization;
pub mod command_handler;
pub mod custom_queries;
pub mod event_bus;
pub mod integration_event;
pub mod service_registry;
pub mod view_repository;

#[cfg(any(feature = "test-utils", test))]
pub mod test_utils;

// Re-export commonly used items for convenience.
pub use agent_macros::IntegrationEvent;
pub use async_trait::async_trait;
pub use chrono;
pub use convert_case;
pub use cqrs_es;
pub use event_bus::{
    BusEventStream, CloudEvent, EventBus, EventBusError, EventBusHandle, EventFilter, EventSource, EventSourceStream,
    Extension, Position, SourceEvent, SubscribePosition,
};
pub use integration_event::{
    collect_projector_docs, render_integration_events_groups_markdown, render_integration_events_markdown,
    render_integration_events_summary_table, EventProjector, IntegrationEvent, IntegrationEventFieldDoc,
    IntegrationEventGroupDoc, IntegrationEventVariantDoc, ProjectIntegrationEvent,
};
pub use slug::slugify;
pub use strum;
pub use uuid::Uuid;
