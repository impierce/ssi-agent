use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("Profile already provisioned")]
    AlreadyProvisioned,
}
