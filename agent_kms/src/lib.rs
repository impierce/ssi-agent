//! The Key Management System (KMS).
//!
//! Lets an instance connect to one or more key providers, such as AWS KMS, and create keys in them that sign inside
//! the provider. A key signs for the `did:web` document once it is added to the document and selected there. The KMS
//! is independent of the Stronghold-based `agent_secret_manager`.

pub mod adapter;
pub mod key;
pub mod key_provider;
pub mod state;

pub use state::{kms_state, KmsState};
