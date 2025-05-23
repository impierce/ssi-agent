use agent_secret_manager::service::Service;
use agent_shared::config::{
    get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported, get_preferred_did_method,
};
use oid4vc_core::{Subject, SubjectSyntaxType};
use oid4vci::Wallet;
use std::sync::Arc;

/// Holder services. This struct is used to sign credentials and validate credential requests.
pub struct HolderServices {
    pub holder: Arc<dyn Subject>,
    pub wallet: Wallet,
}

impl Service for HolderServices {
    fn new(holder: Arc<dyn Subject>) -> Self {
        let signing_algorithms_supported = get_all_enabled_signing_algorithms_supported();

        let mut enabled_did_methods = get_all_enabled_did_methods();
        let preferred_did_method = get_preferred_did_method();
        enabled_did_methods.sort_by(|a, b| {
            if *a == preferred_did_method {
                std::cmp::Ordering::Less
            } else if *b == preferred_did_method {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let supported_subject_syntax_types: Vec<SubjectSyntaxType> =
            enabled_did_methods.into_iter().map(Into::into).collect();

        let wallet = Wallet::new(
            holder.clone(),
            supported_subject_syntax_types,
            signing_algorithms_supported,
        )
        // TODO: make `Wallet::new` return `Wallet` instead of `Result<Self, _>`
        .expect("Failed to create wallet");

        Self { holder, wallet }
    }
}
