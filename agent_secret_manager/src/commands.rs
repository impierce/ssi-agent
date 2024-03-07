use producer::did_document::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum SecretManagerCommand {
    LoadStronghold,
    EnableDidMethod { method: Method },
}
