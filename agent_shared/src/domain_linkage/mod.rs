pub mod verifiable_credential_jwt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::SharedError;
use crate::from_jsonwebtoken_algorithm_to_jwsalgorithm;
use did_manager::SecretManager;
use identity_core::common::{Duration, Timestamp};
use identity_credential::credential::{Credential, Jwt};
use identity_credential::domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder};
use identity_did::DID;
use identity_document::document::CoreDocument;
use identity_storage::{JwkDocumentExt, JwsSignatureOptions, Storage};
use jsonwebtoken::{Algorithm, Header};
use tracing::info;
use verifiable_credential_jwt::VerifiableCredentialJwt;

pub async fn create_did_configuration_resource(
    url: url::Url,
    did_document: CoreDocument,
    secret_manager: &SecretManager,
) -> Result<DomainLinkageConfiguration, SharedError> {
    let url = if cfg!(feature = "local_development") {
        url::Url::parse("http://local.example.org:8080").unwrap()
    } else {
        url
    };

    let origin = identity_core::common::Url::parse(url.origin().ascii_serialization())
        .map_err(|e| SharedError::Generic(e.to_string()))?;
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
    let jwt: Jwt = match did_document.id().method() {
        "iota" => did_document
            .create_credential_jwt(
                &domain_linkage_credential,
                &storage,
                // TODO: make this dynamic
                "key-0",
                &JwsSignatureOptions::default(),
                None,
            )
            .await
            .map_err(|e| SharedError::Generic(e.to_string()))?,
        "web" => {
            let subject_did = did_document.id().to_string();
            let issuer_did = subject_did.clone();

            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
            let expires_in_secs = 60 * 60 * 24 * 365;

            // Create a new verifiable credential.
            let payload = VerifiableCredentialJwt::builder()
                .sub(&subject_did)
                .iss(&issuer_did)
                .nbf(now)
                .exp(now + expires_in_secs)
                .verifiable_credential(serde_json::json!(domain_linkage_credential))
                .build()
                .unwrap();

            // Compose JWT
            let header = Header {
                alg: Algorithm::EdDSA,
                typ: Some("JWT".to_string()),
                kid: Some(format!("{subject_did}#key-0")),
                ..Default::default()
            };

            let message = [
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap().as_slice()),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap().as_slice()),
            ]
            .join(".");

            let proof_value = secret_manager
                .sign(
                    message.as_bytes(),
                    from_jsonwebtoken_algorithm_to_jwsalgorithm(&crate::config::get_preferred_signing_algorithm()),
                )
                .await
                .unwrap();
            let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
            let message = [message, signature].join(".");

            Jwt::from(message)
        }
        _ => {
            unimplemented!("Unsupported DID method: {}", did_document.id().method());
        }
    };

    let configuration_resource: DomainLinkageConfiguration = DomainLinkageConfiguration::new(vec![jwt]);
    println!("Configuration Resource >>: {configuration_resource:#}");

    Ok(configuration_resource)
}
