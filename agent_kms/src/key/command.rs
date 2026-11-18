use crate::key::aggregate::KeyReference;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug)]
pub enum KeyCommand {
    /// Records a key that was just created in its provider.
    RecordCreatedKey { reference: KeyReference },
    /// Records a key that exists in its provider, but that this instance didn't record when it was created.
    RecordAdoptedKey { reference: KeyReference },
    /// Records that the key was deleted from its provider.
    DeleteKey,
}

impl CommandOperation for KeyCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::RecordCreatedKey { .. } => CreateKey.operation_name(),
            Self::RecordAdoptedKey { .. } => AdoptKey.operation_name(),
            Self::DeleteKey => "kms.keys.delete",
        }
    }
}

/// Creating a key calls out to the provider before anything is recorded, so it is authorized before the key is
/// generated, under the same operation as recording it.
#[derive(Debug)]
pub struct CreateKey;

impl CommandOperation for CreateKey {
    fn operation_name(&self) -> &'static str {
        "kms.keys.create"
    }
}

/// Adopting a key calls out to the provider before anything is recorded, so it is authorized first, like
/// [`CreateKey`].
#[derive(Debug)]
pub struct AdoptKey;

impl CommandOperation for AdoptKey {
    fn operation_name(&self) -> &'static str {
        "kms.keys.adopt"
    }
}
