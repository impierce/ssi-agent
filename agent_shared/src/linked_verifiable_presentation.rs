use crate::error::SharedError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager::SecretManager;
use identity_credential::{credential::Jwt, presentation::Presentation};
use identity_document::document::CoreDocument;
use jsonwebtoken::{Algorithm, Header};

pub async fn create_linked_verifiable_presentation_resource(
    url: url::Url,
    verifiable_credential_jwt: Jwt,
    did_document: CoreDocument,
    secret_manager: SecretManager,
) -> Result<Jwt, SharedError> {
    let presentation = Presentation::builder(url.into(), identity_core::common::Object::new())
        .credential(Jwt::from(verifiable_credential_jwt))
        .build()
        .map_err(|e| SharedError::Generic(e.to_string()))?;

    let payload = presentation.serialize_jwt(&Default::default()).expect("FIX THISS");

    // TODO: make distinction between different DID methods.
    let subject_did = did_document.id().to_string();

    // Compose JWT
    let header = Header {
        alg: Algorithm::EdDSA,
        typ: Some("JWT".to_string()),
        kid: Some(format!("{subject_did}#key-0")),
        ..Default::default()
    };

    let message = [
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap().as_slice()),
        URL_SAFE_NO_PAD.encode(payload.as_bytes()),
    ]
    .join(".");

    let proof_value = secret_manager.sign(message.as_bytes()).await.unwrap();
    let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
    let message = [message, signature].join(".");

    Ok(Jwt::from(message))
}
