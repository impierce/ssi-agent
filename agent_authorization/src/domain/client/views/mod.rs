pub mod all_clients;

use super::aggregate::Client;
use cqrs_es::{EventEnvelope, View};

pub type ClientView = Client;

impl View<Client> for Client {
    fn update(&mut self, event: &EventEnvelope<Client>) {
        use super::event::ClientEvent::*;

        match &event.payload {
            ClientRegistered {
                client_id,
                client_secret,
                client_name,
                logo_uri,
                policy_uri,
                tos_uri,
                redirect_uris,
                grant_types,
                response_types,
                token_endpoint_auth_method,
                require_pkce,
                code_challenge_methods_supported: code_challenge_methods,
                require_pushed_authorization_request,
            } => {
                self.client_id = client_id.clone();
                self.client_secret = client_secret.clone();
                self.client_name = client_name.clone();
                self.logo_uri = logo_uri.clone();
                self.policy_uri = policy_uri.clone();
                self.tos_uri = tos_uri.clone();
                self.redirect_uris = redirect_uris.clone();
                self.grant_types = grant_types.clone();
                self.response_types = response_types.clone();
                self.token_endpoint_auth_method = token_endpoint_auth_method.clone();
                self.require_pkce = *require_pkce;
                self.code_challenge_methods_supported = code_challenge_methods.clone();
                self.require_pushed_authorization_request = *require_pushed_authorization_request;
            }
        }
    }
}
