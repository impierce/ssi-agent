use agent_shared::handlers::query_handler;
use tracing::info;

use crate::state::{AuthorizationState, AUTHORIZATION_SERVER_CONFIG_ID};

/// Initialize the authorization state.
pub async fn run_startup_tasks(state: &AuthorizationState) -> Result<(), anyhow::Error> {
    info!("Initializing the identity state ...");
    initialize_authorization_server_config(state).await?;
    // You could add other startup tasks here in the future
    println!("Startup tasks complete.");
    Ok(())
}

// The specific logic for creating the config. Keeping it private to this module.
async fn initialize_authorization_server_config(state: &AuthorizationState) -> Result<(), anyhow::Error> {
    const CONFIG_ID: &str = "singleton_auth_server_config";

    let existing_config = query_handler(AUTHORIZATION_SERVER_CONFIG_ID, &state.query.authorization_server_config)
        .await
        .expect("ERROR");

    if existing_config.is_none() {
        println!("Configuration not found. Dispatching CreateAuthorizationServerConfig command...");

        let command = AuthorizationServerConfigCommand::CreateAuthorizationServerConfig {
            id: CONFIG_ID.to_string(),
            initial_grant_types: vec!["authorization_code".to_string(), "client_credentials".to_string()],
            // ... other default fields
        };

        state.command_handler.handle_create_config(command).await?;

        println!("Successfully created default AuthorizationServerConfig.");
    } else {
        println!("AuthorizationServerConfig already exists. Skipping creation.");
    }

    Ok(())
}
