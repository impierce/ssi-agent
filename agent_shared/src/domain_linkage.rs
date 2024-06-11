use did_manager::SecretManager;
use identity_core::common::{Duration, Timestamp, Url};
use identity_core::convert::ToJson;
use identity_credential::credential::{Credential, Jwt};
use identity_credential::domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder};
use identity_document::document::CoreDocument;
use identity_storage::{JwkDocumentExt, JwkStorage, JwsSignatureOptions, KeyIdStorage, Storage};

use crate::error::SharedError;

pub async fn create_did_configuration_resource(
    domain: Url,
    did_document: CoreDocument,
    secret_manager: SecretManager,
) -> Result<String, SharedError> {
    let domain_linkage_credential: Credential = DomainLinkageCredentialBuilder::new()
        .issuer(did_document.id().clone().into())
        .origin(domain.clone())
        .issuance_date(Timestamp::now_utc())
        // Expires after a year.
        .expiration_date(
            Timestamp::now_utc()
                .checked_add(Duration::days(365))
                .ok_or_else(|| SharedError::Generic("calculation should not overflow".to_string()))?,
        )
        .build()
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    let storage = secret_manager.stronghold_storage;

    // let storage: &Storage<_, _> = &secret_manager.stronghold_storage;

    // let payload = domain_linkage_credential
    //     .serialize_jwt(None)
    //     .map_err(|e| SharedError::Generic(e.to_string()))?;

    // let storage: Storage<>

    let jwt: Jwt = did_document
        .create_credential_jwt(
            &domain_linkage_credential,
            &storage,
            "fragment-0",
            &JwsSignatureOptions::default(),
            None,
        )
        .await
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    // let jwt: Jwt = Jwt::new("ey...".to_string());

    let configuration_resource: DomainLinkageConfiguration = DomainLinkageConfiguration::new(vec![jwt]);
    println!("Configuration Resource >>: {configuration_resource:#}");

    // The DID Configuration resource can be made available on `https://foo.example.com/.well-known/did-configuration.json`.
    let configuration_resource_json: String = configuration_resource
        .to_json()
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    Ok(configuration_resource_json)
}
