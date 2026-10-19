use crate::key_provider::aggregate::KeyProviderType;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug)]
pub enum KeyProviderCommand {
    AddKeyProvider {
        provider_type: KeyProviderType,
        name: String,
    },
    RemoveKeyProvider {
        provider_type: KeyProviderType,
    },
}

impl CommandOperation for KeyProviderCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::AddKeyProvider { .. } => "kms.key_providers.add",
            Self::RemoveKeyProvider { .. } => "kms.key_providers.remove",
        }
    }
}

/// Checking whether a provider is reachable changes no state, but it does call out to the provider, so it is
/// authorized like a command.
#[derive(Debug)]
pub struct TestKeyProviderConnection;

impl CommandOperation for TestKeyProviderConnection {
    fn operation_name(&self) -> &'static str {
        "kms.key_providers.test_connection"
    }
}
