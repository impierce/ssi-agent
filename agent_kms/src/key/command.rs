use crate::key::aggregate::KeyReference;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug)]
pub enum KeyCommand {
    /// Records a key that was just created in its provider.
    RecordCreatedKey { reference: KeyReference },
}

impl CommandOperation for KeyCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::RecordCreatedKey { .. } => CreateKey.operation_name(),
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
