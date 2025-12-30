use crate::managed_key::aggregate::SigningAlgorithm;

#[derive(Debug)]
pub enum ManagedKeyCommand {
    GenerateKey {
        managed_key_id: String,
        alias: String,
        signing_algorithm: SigningAlgorithm,
    },
    RemoveKey,
    UpdateKeyAlias {
        new_alias: String,
    },
    SetSigningKey,
}
