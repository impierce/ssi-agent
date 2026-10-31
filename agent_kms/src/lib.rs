//! The Key Management System (KMS).
//!
//! Lets an instance connect to one or more key providers, such as AWS KMS, where its signing keys will live. This
//! first version only manages which providers are connected and whether they are reachable; key generation and
//! signing are not part of it yet. It is independent of the Stronghold-based `agent_secret_manager`.

pub mod adapter;
pub mod key_provider;
pub mod state;

pub use state::{kms_state, KmsState};
