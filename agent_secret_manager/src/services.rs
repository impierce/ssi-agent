use crate::{
    managed_key::{aggregate::SigningAlgorithm, command::ManagedKeyCommand, views::ManagedKeyView},
    state::SecretManagerState,
    stronghold_storage,
};
use agent_shared::handlers::query_handler;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use std::sync::{Arc, OnceLock};

pub struct SecretManagerServices {
    pub stronghold_storage: StrongholdExtStorage,
}

impl SecretManagerServices {
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        Self { stronghold_storage }
    }
}

pub static SECRET_MANAGER_DOMAIN_SERVICE: OnceLock<Arc<SecretManagerDomainService>> = OnceLock::new();

pub struct SecretManagerDomainService {
    pub secret_manager_state: Arc<SecretManagerState>,
}

impl SecretManagerDomainService {
    pub fn new(secret_manager_state: Arc<SecretManagerState>) -> Self {
        Self { secret_manager_state }
    }

    pub async fn current_signing_key(&self, signing_algorithm: &SigningAlgorithm) -> Option<ManagedKeyView> {
        let all_managed_keys_view =
            query_handler("all_managed_keys", &self.secret_manager_state.query.all_managed_keys)
                .await
                .unwrap()?;

        all_managed_keys_view
            .managed_keys
            .into_iter()
            .find_map(|(_, managed_key_view)| {
                if managed_key_view.signing_algorithm.as_ref() == Some(signing_algorithm)
                    && managed_key_view.is_signing_key
                    && !managed_key_view.is_removed
                {
                    Some(managed_key_view.clone())
                } else {
                    None
                }
            })
    }

    pub async fn unset_signing_keys(&self, signing_algorithm: &SigningAlgorithm) {
        if let Some(all_managed_keys_view) =
            query_handler("all_managed_keys", &self.secret_manager_state.query.all_managed_keys)
                .await
                .unwrap()
        {
            for (_, managed_key_view) in all_managed_keys_view.managed_keys.into_iter() {
                if managed_key_view.signing_algorithm.as_ref() == Some(signing_algorithm)
                    && managed_key_view.is_signing_key
                    && !managed_key_view.is_removed
                {
                    let command = ManagedKeyCommand::UnsetSigningKey;

                    let _ = agent_shared::handlers::command_handler(
                        &managed_key_view.managed_key_id,
                        &self.secret_manager_state.command.managed_key,
                        command,
                    )
                    .await;
                }
            }
        }
    }
}

impl std::fmt::Debug for SecretManagerDomainService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretManagerDomainService").finish()
    }
}
