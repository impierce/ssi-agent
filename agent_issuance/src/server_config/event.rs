use agent_library::template::event::Authorization;
use cqrs_es::DomainEvent;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata,
    credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ServerConfigEvent {
    ServerMetadataInitialized {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
    },
    IssuerUrlUpdated {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    },
    IssuerDisplayUpdated {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    },
    CryptographicBindingMethodsUpdated {
        cryptographic_binding_methods_supported: Vec<String>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    SigningAlgorithmsUpdated {
        signing_algorithms_supported: Vec<Algorithm>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },

    CredentialConfigurationCreated {
        credential_configuration_id: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationIdUpdated {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationDisplayUpdated {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationAuthorizationUpdated {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationRemoved {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    },
}

impl DomainEvent for ServerConfigEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
