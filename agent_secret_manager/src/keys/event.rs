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
        alias: String,
        modified_at: String,
    },
    AliasRenamed {
        old_alias: String,
        new_alias: String,
        modified_at: String,
    },
    SigningKeySet {
        alias: String,
        modified_at: String,
    },
}
