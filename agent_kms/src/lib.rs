//! The Key Management System (KMS).
//!
//! Lets an instance connect to one or more key providers, such as AWS KMS, and create keys in them that never leave
//! the provider, and hands them out to sign with. It is independent of the Stronghold-based `agent_secret_manager`.

pub mod adapter;
pub mod key;
pub mod key_provider;
pub mod state;

pub use state::{kms_state, KmsState};
