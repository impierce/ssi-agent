use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum KeyEvent {
    KeyGenerated {
        alias: String,
        signature_algorithm: Algorithm,
        modified_at: String,
    },
    KeyRemoved {
        modified_at: String,
    },
    AliasRenamed {
        new_alias: String,
        modified_at: String,
    },
    SigningKeySet {
        modified_at: String,
    },
}
