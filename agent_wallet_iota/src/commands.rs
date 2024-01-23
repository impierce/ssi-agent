use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum WalletCommand {
    LoadAccount { alias: String },
}
