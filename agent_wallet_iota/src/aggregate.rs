use async_trait::async_trait;
use cqrs_es::Aggregate;
use iota_sdk::{
    client::{
        constants::SHIMMER_COIN_TYPE,
        secret::{stronghold::StrongholdSecretManager, SecretManager},
    },
    wallet::{ClientOptions, Wallet as IotaWallet},
};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{commands::WalletCommand, errors::WalletError, events::WalletEvent, services::WalletServices};

#[derive(Serialize, Deserialize)]
pub struct Wallet {
    id: String,
    alias: String,
}

#[async_trait]
impl Aggregate for Wallet {
    type Command = WalletCommand;
    type Event = WalletEvent;
    type Error = WalletError;
    type Services = WalletServices;

    fn aggregate_type() -> String {
        "wallet".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        // const NODE_URL: &str = "https://api.testnet.shimmer.network";
        const NODE_URL: &str = "http://localhost:14265";

        match command {
            WalletCommand::LoadAccount { alias } => {
                debug!("Building secret manager ...");
                let secret_manager = StrongholdSecretManager::builder()
                    .password("sup3rSecr3t".to_owned())
                    .build("vault.stronghold")
                    .expect("todo(): throw domain error");

                debug!("Setting client options ...");
                let client_options = ClientOptions::new()
                    .with_node(NODE_URL)
                    .expect("todo(): throw domain error");

                debug!("Building wallet ...");
                let wallet = IotaWallet::builder()
                    .with_secret_manager(SecretManager::Stronghold(secret_manager))
                    .with_client_options(client_options)
                    .with_coin_type(SHIMMER_COIN_TYPE)
                    .finish()
                    .await
                    .expect("todo(): throw domain error");

                debug!("Creating mnemonic seed phrase ...");
                let mnemonic = wallet.generate_mnemonic().expect("todo(): throw domain error");
                debug!("Mnemonic: {}", mnemonic.as_ref());
                wallet
                    .store_mnemonic(mnemonic)
                    .await
                    .expect("todo(): throw domain error");

                debug!("Creating account ...");
                let account = wallet
                    .create_account()
                    .with_alias(alias.clone())
                    .finish()
                    .await
                    .expect("todo(): throw domain error");

                let first_address = &account.addresses().await.expect("todo(): throw domain error")[0];
                debug!("Address: {}", first_address.address());

                Ok(vec![WalletEvent::AccountLoaded {
                    alias,
                    address: first_address.address().to_string(),
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        todo!()
    }
}

impl Default for Wallet {
    fn default() -> Self {
        Wallet {
            id: "".to_string(),
            alias: "".to_string(),
        }
    }
}

#[cfg(test)]
mod aggregate_tests {
    use cqrs_es::test::TestFramework;

    use crate::aggregate::Wallet;
    use crate::commands::WalletCommand;
    use crate::events::WalletEvent;
    use crate::services::WalletServices;

    type WalletTestFramework = TestFramework<Wallet>;

    #[ignore]
    #[test]
    fn test_load_nonexisting_account_creates_new() {
        let expected = WalletEvent::AccountLoaded {
            alias: "ferris".to_string(),
            address: "test123".to_string(),
        };
        let command = WalletCommand::LoadAccount {
            alias: "ferris".to_string(),
        };
        let services = WalletServices {};

        WalletTestFramework::with(services)
            .given_no_previous_events()
            .when(command)
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_load_existing_account() {}
}
