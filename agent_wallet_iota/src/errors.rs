use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Loading Account Failed")]
    LoadingAccountFailed,
}
