use jsonwebtoken::Algorithm;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum KeyCommand {
    GenerateKey {
        alias: String,
        signature_algorithm: Algorithm,
    },
    RemoveKey {},
    RenameAlias {
        new_alias: String,
    },
    SetSigningKey {},
}
