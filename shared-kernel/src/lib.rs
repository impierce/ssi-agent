//! Shared kernel for the SSI Agent.
//!
//! Provides infrastructure used by all bounded contexts:
//!
//! - [`application_service`] — Actor-style service that serialises command/query processing.
//! - [`command_handler`] — Traits and helpers for dispatching commands to aggregates.
//! - [`custom_queries`] — Reusable CQRS query types such as [`custom_queries::ListAllQuery`].
//! - [`view_repository`] — Dyn-compatible view repository wrappers and factory trait.
//! - [`service_registry`] — Type-keyed service registry and channel-based service handles.

pub mod application_service;
pub mod command_handler;
pub mod custom_queries;
pub mod service_registry;
pub mod view_repository;

#[cfg(any(feature = "test-utils", test))]
pub mod test_utils;

// Re-export commonly used items for convenience.
pub use async_trait::async_trait;
pub use chrono;
pub use convert_case;
pub use cqrs_es;
pub use slug::slugify;
pub use strum;
pub use uuid::Uuid;
