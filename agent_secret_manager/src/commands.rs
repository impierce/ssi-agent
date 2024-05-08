use did_manager::DidMethod;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum SecretManagerCommand {
    Initialize,
    EnableDidMethod { method: DidMethod },
}
