//! The Key Management System (KMS).
//!
//! Lets an instance connect to one or more key providers, such as AWS KMS, and create keys in them that sign inside
//! the provider. Nothing signs with these keys yet. It is independent of the Stronghold-based `agent_secret_manager`.

pub mod adapter;
pub mod key;
pub mod key_provider;
pub mod state;

pub use state::{kms_state, KmsState};
