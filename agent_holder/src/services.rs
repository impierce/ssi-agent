use agent_shared::config::{config, get_all_enabled_did_methods, get_preferred_did_method};
use jsonwebtoken::Algorithm;
use oid4vc_core::{Subject, SubjectSyntaxType};
use oid4vci::Wallet;
use std::{str::FromStr, sync::Arc};

/// Holder services. This struct is used to sign credentials and validate credential requests.
pub struct HolderServices {
    pub holder: Arc<dyn Subject>,
    pub wallet: Wallet,
}

impl HolderServices {
    pub fn new(holder: Arc<dyn Subject>) -> Self {
        let signing_algorithms_supported: Vec<Algorithm> = config()
            .signing_algorithms_supported
            .iter()
            .filter(|(_, opts)| opts.enabled)
            .map(|(alg, _)| *alg)
            .collect();

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

        let supported_subject_syntax_types = enabled_did_methods
            .into_iter()
            .map(|method| SubjectSyntaxType::from_str(&method.to_string()).unwrap())
            .collect();

        let wallet = Wallet::new(
            holder.clone(),
            supported_subject_syntax_types,
            signing_algorithms_supported,
        )
        .unwrap();

        Self { holder, wallet }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;

    use super::*;

    pub fn test_holder_services() -> Arc<HolderServices> {
        Arc::new(HolderServices::new(Arc::new(futures::executor::block_on(async {
            Subject {
                secret_manager: secret_manager().await,
            }
        }))))
    }
}
