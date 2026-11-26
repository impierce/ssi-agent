use crate::key::aggregate::KeyReference;
use shared_kernel::authorization::CommandOperation;
use strum::EnumDiscriminants;

#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(name(KeyOperation))]
pub enum KeyCommand {
    /// Records a key this instance has just created in its provider.
    CreateKey { reference: KeyReference },
    /// Records a key that already existed in its provider.
    AdoptKey { reference: KeyReference },
    /// Records that the key was deleted from its provider.
    DeleteKey,
}

/// A key command's operation is known before its reference is: a key is authorized on its own id before its
/// provider creates or looks it up.
impl CommandOperation for KeyOperation {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateKey => "kms.keys.create",
            Self::AdoptKey => "kms.keys.adopt",
            Self::DeleteKey => "kms.keys.delete",
        }
    }
}

impl CommandOperation for KeyCommand {
    fn operation_name(&self) -> &'static str {
        KeyOperation::from(self).operation_name()
    }
}
