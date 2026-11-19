//! A lease that makes a single process the writer of an event store.
//!
//! The lease holder appends events; every append checks, in the same transaction, that the lease
//! is still held with the fencing token it was acquired with. A process that lost the lease (its
//! connection broke, or it stopped renewing) can therefore not append, even if it has not noticed
//! the loss yet. A process waiting for the lease does not start before the holder releases it, or
//! before the lease expires.

use crate::{mongodb_lease::MongoWriterLease, postgres_lease::PostgresWriterLease};
use std::{fmt, sync::Arc};

/// Lease identifier, shared by all processes writing to one event store.
pub(crate) const LEASE_ID: &str = "event_store_writer";

/// The event-store writer lease of this process.
#[derive(Clone)]
pub enum WriterLease {
    MongoDb(Arc<MongoWriterLease>),
    Postgres(Arc<PostgresWriterLease>),
}

impl WriterLease {
    /// Completes once the lease is lost: from then on, no events can be appended.
    pub async fn lost(&self) {
        match self {
            Self::MongoDb(lease) => lease.lost().await,
            Self::Postgres(lease) => lease.lost().await,
        }
    }

    /// Releases the lease, so a waiting process acquires it without waiting for it to expire.
    pub async fn release(&self) {
        match self {
            Self::MongoDb(lease) => lease.release().await,
            Self::Postgres(lease) => lease.release().await,
        }
    }
}

/// An event is appended without holding the writer lease.
#[derive(Debug)]
pub(crate) struct LeaseNotHeld;

impl fmt::Display for LeaseNotHeld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the event-store writer lease is not held by this process")
    }
}

impl std::error::Error for LeaseNotHeld {}
