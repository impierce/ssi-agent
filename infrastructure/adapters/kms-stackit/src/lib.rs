//! STACKIT KMS key provider: keys are generated in, and sign inside, a key ring of a STACKIT project, which can be the
//! customer's own.
//!
//! `UniCore` signs in as a STACKIT service account with the workload identity token its host gives it (ADR 0011), so
//! it stores no secret. Keys are P-256 and sign with ES256.

mod answer;
mod api;
mod key_name;
mod key_store;
mod provider;
mod sign_in;

#[cfg(test)]
mod fake_stackit;

pub use api::StackitKmsError;
pub use provider::{StackitKmsFactory, StackitKmsKeyProvider};
pub use sign_in::{ServiceAccountKey, SignIn, SignInError};
