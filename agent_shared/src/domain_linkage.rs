use did_manager::SecretManager;
use identity_core::common::{Duration, Timestamp};
use identity_core::convert::ToJson;
use identity_credential::credential::{Credential, Jwt};
use identity_credential::domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder};
use identity_did::DID;
use identity_document::document::CoreDocument;
use identity_storage::{JwkDocumentExt, JwsSignatureOptions, Storage};
use tracing::info;

use crate::error::SharedError;

pub async fn create_did_configuration_resource(
    url: url::Url,
    did_document: CoreDocument,
    secret_manager: SecretManager,
) -> Result<String, SharedError> {
    let origin = identity_core::common::Url::parse(url.origin().ascii_serialization())
        .map_err(|e| SharedError::Generic(e.to_string()))?;
    // info!("DID Document: {did_document:#}");
    // info!("CoreDID: {}", did_document.id());
    let domain_linkage_credential: Credential = DomainLinkageCredentialBuilder::new()
        .issuer(did_document.id().clone())
        .origin(origin)
        .issuance_date(Timestamp::now_utc())
        // Expires after a year.
        .expiration_date(
            Timestamp::now_utc()
                .checked_add(Duration::days(365))
                .ok_or_else(|| SharedError::Generic("calculation should not overflow".to_string()))?,
        )
        .build()
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    info!("Domain Linkage Credential: {domain_linkage_credential:#}");

    // Construct a `Storage` (identity_stronghold) for temporary usage: create JWS, etc.
    let key_storage = secret_manager.stronghold_storage.clone();
    let key_id_storage = secret_manager.stronghold_storage.clone();

    let storage = Storage::new(key_storage, key_id_storage);

    info!("DID Document: {did_document:#}");

    // identity.rs currently doesn't know how to handle a `did:web` document in `create_credential_jwt()`.

    // Compose JWT and sign

    // header: { "alg": "EdDSA", "typ": "JWT", kid: <did_fragment> }

    let jwt: Jwt = match did_document.id().method() {
        "iota" => did_document
            .create_credential_jwt(
                &domain_linkage_credential,
                &storage,
                "key-0",
                &JwsSignatureOptions::default(),
                None,
            )
            .await
            .map_err(|e| SharedError::Generic(e.to_string()))?,
        _ => {
            unimplemented!("Unsupported DID method: {}", did_document.id().method());
        }
    };

    let configuration_resource: DomainLinkageConfiguration = DomainLinkageConfiguration::new(vec![jwt]);
    println!("Configuration Resource >>: {configuration_resource:#}");

    // The DID Configuration resource can be made available on `https://foo.example.com/.well-known/did-configuration.json`.
    let configuration_resource_json: String = configuration_resource
        .to_json()
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    Ok(configuration_resource_json)
}
