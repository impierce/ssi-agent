use did_manager::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum SecretManagerCommand {
    LoadStronghold,
    EnableDidMethod { method: Method },
}
