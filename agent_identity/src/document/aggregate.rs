use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::document::openapi::algorithm;
use crate::services::IdentityServices;
use agent_secret_manager::subject::StorageKey;
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::did::{
    Context, Did, DidDocument, DidUrl, MethodType, VerificationMaterial, VerificationMethod, VerificationRelationship,
};
use identity_kit::jose::Jwk;
use identity_kit::methods::{jwk::DidJwk, key::DidKey};
use indexmap::IndexMap;
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub enum Status {
    SignAndValidate,
    // TODO: Make a distinction between enabling both signing AND validation and just validation.
    // ValidateOnly,
    #[default]
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Document {
    #[serde(rename = "id")]
    pub document_id: String,
    pub document: Option<DidDocument>,
    pub did_method: Option<SupportedDidMethod>,
    // Applicable only for DID documents whose methods mandate a fixed verification algorithm,
    // such as `did:key` and `did:jwk`.
    #[schema(schema_with = algorithm)]
    pub with_fixed_algorithm: Option<Algorithm>,
    #[schema(inline)]
    pub status: Status,
    /// The verification methods of keys that aren't configured issuer keys, such as KMS keys, in the order they were
    /// added.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub added_verification_methods: Vec<DidUrl>,
    /// The ID in the Key Management System of the key of each added verification method, where it was recorded.
    #[serde(default)]
    #[schema(value_type = std::collections::HashMap<String, String>)]
    pub added_keys: IndexMap<DidUrl, String>,
    /// The verification method selected to sign with, per algorithm. Without one, an algorithm signs with the
    /// configured issuer key.
    #[serde(default)]
    #[schema(value_type = std::collections::HashMap<String, String>)]
    pub signing_methods: IndexMap<Algorithm, DidUrl>,
}

impl Aggregate for Document {
    type Command = DocumentCommand;
    type Event = DocumentEvent;
    type Error = DocumentError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "document";

    // TODO: Most of how these commands are handled is not Domain logic, but rather Application logic, so it should be moved
    // to the Application layer. The Aggregate should only handle the Domain logic, such as creating a new Document, updating public keys, etc.
    // The Application layer should handle the specifics of how to create a Document based on the DID method, how to publish it, etc.
    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use DocumentCommand::*;
        use DocumentError::*;
        use DocumentEvent::*;

        let events: Vec<Self::Event> = match command {
            CreateDocument {
                document_id,
                did_method,
                with_fixed_algorithm,
            } => {
                let subject = &services.subject;

                let document = match &did_method {
                    SupportedDidMethod::Web => {
                        let controller = super::web::did_web(&services.public_url)?;

                        // Patch the generated DID document since it's not according to spec.
                        DidDocument::builder(controller)
                            .context(get_context(&MethodType::JSON_WEB_KEY_2020))
                            .build()
                            .map_err(ProduceDocumentError)?
                    }
                    SupportedDidMethod::Jwk | SupportedDidMethod::Key => {
                        let algorithm = with_fixed_algorithm.ok_or(MissingFixedAlgorithmError(did_method))?;
                        let key_id = match algorithm {
                            Algorithm::EdDSA => config().secret_manager.issuer_eddsa_key_id.clone(),
                            Algorithm::ES256 => config().secret_manager.issuer_es256_key_id.clone(),
                            algorithm => return Err(UnsupportedSigningAlgorithmError(algorithm)),
                        };

                        // Retrieve the public key from Stronghold.
                        let public_key_jwk = subject
                            .get_public_key(key_id.clone())
                            .await
                            .map_err(|err| MissingKeyError(err.to_string()))?;

                        // Generate a DID from the public key.
                        let controller = match did_method {
                            SupportedDidMethod::Jwk => DidJwk::did(&public_key_jwk).ok(),
                            SupportedDidMethod::Key => DidKey::did(&public_key_jwk).ok(),
                            _ => None,
                        }
                        .ok_or(GenerateDidError(key_id))?;

                        DidDocument::new(controller)
                    }
                };

                let status = Status::SignAndValidate;

                Ok::<Vec<Self::Event>, Self::Error>(vec![DocumentCreated {
                    document_id,
                    did_method,
                    status,
                    document,
                    with_fixed_algorithm,
                }])
            }
            UpdatePublicKeys {
                // TODO: decide whether the public keys should be supplied through the command or not.
                public_key_jwks: _,
            } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let did = document.id.clone();

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                let subject = &services.subject;

                // Remove the verification methods of the configured keys from the document; added keys stay.
                let method_ids = document
                    .methods(None)
                    .map(|method| method.id.clone())
                    .filter(|method_id| !self.added_keys.contains_key(method_id))
                    .collect::<Vec<_>>();
                for method_id in method_ids {
                    document.remove_verification_method(&method_id);
                }

                // Insert the new verification methods based on the signing algorithms.
                let mut events = vec![];
                for signing_algorithm in self
                    .with_fixed_algorithm
                    .map(|signing_algorithm| vec![signing_algorithm])
                    .unwrap_or_else(get_all_enabled_signing_algorithms_supported)
                {
                    let key_id = match signing_algorithm {
                        Algorithm::EdDSA => config().secret_manager.issuer_eddsa_key_id.clone(),
                        Algorithm::ES256 => config().secret_manager.issuer_es256_key_id.clone(),
                        algorithm => return Err(UnsupportedSigningAlgorithmError(algorithm)),
                    };

                    let public_key_jwk = subject
                        .get_public_key(key_id)
                        .await
                        .map_err(|err| MissingKeyError(err.to_string()))?;

                    let verification_method = verification_method(&did, did_method, &public_key_jwk)?;

                    subject
                        .insert_verification_method_id(
                            StorageKey::new(did_method, signing_algorithm),
                            verification_method.id.clone(),
                        )
                        .await
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    // Attach all possible relationships to the verification method.
                    document
                        .insert_verification_method(verification_method, VerificationRelationship::ALL.iter().copied()) // TODO: adjust KID insertion elsewhere
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    events.push(PublicKeyUpdated {
                        document_id: self.document_id.clone(),
                        document: document.clone(),
                    })
                }

                Ok(events)
            }
            OverwritePreviousDidWeb {
                previous_did,
                public_url,
            } => {
                if self.did_method != Some(SupportedDidMethod::Web) {
                    return Err(InvalidDidError(
                        "Only did:web supports overwriting the deployment identity".into(),
                    ));
                }
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let previous = document.id.clone();
                if previous != previous_did {
                    return Err(InvalidDidError(format!(
                        "Overwrite authorization for {previous_did} does not match the current DID {previous}"
                    )));
                }
                let replacement = super::web::did_web(&public_url)?;
                document.map_dids(|did| if did == previous { replacement.clone() } else { did });
                Ok(vec![DocumentDidWebOverwritten {
                    document_id: self.document_id.clone(),
                    previous_did: previous,
                    document,
                }])
            }
            UpdateDocumentStatus { status } => Ok(vec![DocumentStatusUpdated {
                document_id: self.document_id.clone(),
                status,
            }]),
            AddService {
                service_id,
                mut service,
            } => {
                let document_id = self.document_id.clone();
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                // Set the service ID.
                service.id = document
                    .id
                    .join(&format!("#{service_id}"))
                    .map_err(|_| InvalidDidError(service_id.to_string()))?;

                // Overwrite the service if it already exists.
                document.remove_service(&service.id);
                document
                    .insert_service(*service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document_id, document }])
            }
            RemoveService { service_id } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let id = document
                    .id
                    .join(&format!("#{service_id}"))
                    .map_err(|err| InvalidDidError(err.to_string()))?;
                if document.remove_service(&id).is_none() {
                    return Ok(());
                }
                Ok(vec![ServiceRemoved {
                    document_id: self.document_id.clone(),
                    document,
                }])
            }
            AddPublicKey { public_key, key_id } => {
                if self.did_method != Some(SupportedDidMethod::Web) {
                    return Err(DidWebOnlyError);
                }
                let method = self.added_verification_method(*public_key)?;
                if self.added_keys.contains_key(&method.id) {
                    return Ok(());
                }

                let verification_method_id = method.id.clone();
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                document
                    .insert_verification_method(method, VerificationRelationship::ALL.iter().copied())
                    .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;
                Ok(vec![PublicKeyAdded {
                    document_id: self.document_id.clone(),
                    verification_method_id,
                    key_id,
                    document,
                }])
            }
            RemovePublicKey { verification_method_id } => {
                if !self.added_keys.contains_key(&verification_method_id) {
                    return Err(NotAnAddedKeyError(verification_method_id));
                }
                if self
                    .signing_methods
                    .values()
                    .any(|method| *method == verification_method_id)
                {
                    return Err(SelectedSigningKeyError(verification_method_id));
                }

                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                document.remove_verification_method(&verification_method_id);
                Ok(vec![PublicKeyRemoved {
                    document_id: self.document_id.clone(),
                    verification_method_id,
                    document,
                }])
            }
            SelectSigningKey { verification_method_id } => {
                let document = self.document.as_ref().ok_or(MissingDocumentError)?;
                let Some(method) = document.verification_method(&verification_method_id) else {
                    return Err(UnknownVerificationMethodError(verification_method_id));
                };
                let Some(algorithm) = key_algorithm(method) else {
                    return Err(NoKeyAlgorithmError(verification_method_id));
                };
                if self.signing_methods.get(&algorithm) == Some(&verification_method_id) {
                    return Ok(());
                }

                Ok(vec![SigningKeySelected {
                    document_id: self.document_id.clone(),
                    algorithm,
                    verification_method_id,
                }])
            }
            DeselectSigningKey { algorithm } => {
                if !self.signing_methods.contains_key(&algorithm) {
                    return Ok(());
                }

                Ok(vec![SigningKeyDeselected {
                    document_id: self.document_id.clone(),
                    algorithm,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        match event {
            DocumentCreated {
                document_id,
                did_method,
                status,
                document,
                with_fixed_algorithm,
            } => {
                self.document_id = document_id;
                self.did_method.replace(did_method);
                self.status = status;
                self.document.replace(document);
                self.with_fixed_algorithm = with_fixed_algorithm;
            }
            PublicKeyUpdated { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            DocumentDidWebOverwritten {
                document_id,
                previous_did,
                document,
            } => {
                let moved = |method: &DidUrl| {
                    if *method.did() == previous_did {
                        method.with_did(document.id.clone())
                    } else {
                        method.clone()
                    }
                };
                self.added_verification_methods = self.added_verification_methods.iter().map(moved).collect();
                self.added_keys = self
                    .added_keys
                    .iter()
                    .map(|(method, key_id)| (moved(method), key_id.clone()))
                    .collect();
                self.signing_methods = self
                    .signing_methods
                    .iter()
                    .map(|(algorithm, method)| (*algorithm, moved(method)))
                    .collect();
                self.document_id = document_id;
                self.document.replace(document);
            }
            DocumentStatusUpdated { document_id, status } => {
                self.document_id = document_id;
                self.status = status;
            }
            ServiceAdded { document_id, document } | ServiceRemoved { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            DocumentDeleted { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            PublicKeyAdded {
                document_id,
                verification_method_id,
                key_id,
                document,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.added_verification_methods.push(verification_method_id.clone());
                self.added_keys.insert(verification_method_id, key_id);
            }
            PublicKeyRemoved {
                document_id,
                verification_method_id,
                document,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.added_verification_methods
                    .retain(|method| *method != verification_method_id);
                self.added_keys.shift_remove(&verification_method_id);
            }
            SigningKeySelected {
                document_id,
                algorithm,
                verification_method_id,
            } => {
                self.document_id = document_id;
                self.signing_methods.insert(algorithm, verification_method_id);
            }
            SigningKeyDeselected { document_id, algorithm } => {
                self.document_id = document_id;
                self.signing_methods.shift_remove(&algorithm);
            }
        }
    }
}

/// The algorithm the key of `method` names, when UniCore signs with it.
pub(crate) fn key_algorithm(method: &VerificationMethod) -> Option<Algorithm> {
    match method.public_key_jwk()?.alg()?.parse().ok()? {
        algorithm @ (Algorithm::ES256 | Algorithm::EdDSA) => Some(algorithm),
        _ => None,
    }
}

impl Document {
    /// The verification method `public_key` is added to this document as: named by the key's thumbprint, which also
    /// becomes the key's `kid`.
    ///
    /// # Errors
    ///
    /// Fails when there is no document yet, or the key names no algorithm UniCore signs with.
    pub fn added_verification_method(&self, public_key: Jwk) -> Result<VerificationMethod, DocumentError> {
        let did = &self.document.as_ref().ok_or(DocumentError::MissingDocumentError)?.id;
        let thumbprint = public_key
            .thumbprint_sha256()
            .map_err(|err| DocumentError::VerificationMethodBuilderError(err.to_string()))?;
        let method = verification_method(did, SupportedDidMethod::Web, &public_key.with_kid(thumbprint))?;
        if key_algorithm(&method).is_none() {
            return Err(DocumentError::NoKeyAlgorithmError(method.id));
        }
        Ok(method)
    }
}

/// A `JsonWebKey2020` verification method for `public_key_jwk`. Its fragment is the method-specific identifier for
/// `did:key`, the fragment `did_method` mandates, if any, or else the key's `kid`.
fn verification_method(
    did: &Did,
    did_method: SupportedDidMethod,
    public_key_jwk: &Jwk,
) -> Result<VerificationMethod, DocumentError> {
    let fragment = (did_method == SupportedDidMethod::Key)
        .then_some(did.method_specific_id())
        .or(did_method.fragment())
        .or(public_key_jwk.kid())
        .ok_or_else(|| {
            DocumentError::VerificationMethodBuilderError("an explicit fragment or JWK kid is required".into())
        })?;
    let id = did
        .join(&format!("#{fragment}"))
        .map_err(|err| DocumentError::VerificationMethodBuilderError(err.to_string()))?;

    Ok(VerificationMethod::new(
        id,
        MethodType::JSON_WEB_KEY_2020,
        did.clone(),
        VerificationMaterial::jwk(public_key_jwk),
    ))
}

// TODO: Can we remove this? It does not seem to be required: https://w3c-ccg.github.io/did-method-web/#key-material-and-document-handling
pub fn get_context(method_type: &MethodType) -> Context {
    match method_type.as_str() {
        "Ed25519VerificationKey2018" => {
            Context::uris([Context::DID_V1, "https://w3id.org/security/suites/ed25519-2018/v1"])
        }
        "JsonWebKey2020" => Context::uris([Context::DID_V1, "https://w3id.org/security/suites/jws-2020/v1"]),
        _ => unimplemented!("Unsupported method type"),
    }
}

#[cfg(test)]
pub mod document_tests {
    use crate::state::LINKED_DOMAINS_SERVICE_ID;

    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use identity_kit::did::Service;
    use rstest::rstest;

    type DocumentTestFramework = TestFramework<Document>;

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_document(document_id: String, did_method: SupportedDidMethod, document: DidDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when_async(DocumentCommand::CreateDocument {
                document_id: document_id.clone(),
                did_method,
                with_fixed_algorithm: None,
            })
            .await
            .then_expect_events(vec![DocumentEvent::DocumentCreated {
                document_id,
                did_method,
                document,
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
            }])
    }

    #[rstest]
    #[case::did_key_eddsa(
        SupportedDidMethod::Key,
        Algorithm::EdDSA,
        "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt"
    )]
    #[case::did_key_es256(
        SupportedDidMethod::Key,
        Algorithm::ES256,
        "did:key:zDnaeRwT4g6AZCHzxvNL7DLjqTaT88am4XR6TUGrKr6DXj6Tz"
    )]
    #[case::did_jwk_eddsa(
        SupportedDidMethod::Jwk,
        Algorithm::EdDSA,
        "did:jwk:eyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ"
    )]
    #[case::did_jwk_es256(
        SupportedDidMethod::Jwk,
        Algorithm::ES256,
        "did:jwk:eyJhbGciOiJFUzI1NiIsImNydiI6IlAtMjU2Iiwia2lkIjoib09ZMmRNVlU3R0s1YWwxcTdFQXh1b1lsb3hNUWx2NVpOWk9hdGlVWFFIZyIsImt0eSI6IkVDIiwieCI6IkZtazEzZ08yU0dMYnVYZUwyNHFKUEhDTm5jbkk2bEJ1NlpRTDJFVlp2NEUiLCJ5IjoiZnoyS3ZNaHVmelVwTWVMOS1LMnJlOWZ3QTNtemcxYnBmYmNlSVFTdWloWSJ9"
    )]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_document_derives_the_did_from_the_issuer_key(
        document_id: String,
        #[case] did_method: SupportedDidMethod,
        #[case] algorithm: Algorithm,
        #[case] expected_did: &str,
    ) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when_async(DocumentCommand::CreateDocument {
                document_id,
                did_method,
                with_fixed_algorithm: Some(algorithm),
            })
            .await
            .inspect_result()
            .unwrap();

        let [DocumentEvent::DocumentCreated { document, .. }] = events.as_slice() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(document.id.as_str(), expected_did);
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_set_public_key_jwks(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        document_with_single_verification_method: DidDocument,
        document_with_multiple_verification_methods: DidDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id: document_id.clone(),
                did_method,
                document: document.clone(),
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
            }])
            .when_async(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
            .await
            .then_expect_events(vec![
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_single_verification_method,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id,
                    document: document_with_multiple_verification_methods,
                },
            ])
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_add_service(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        linked_domains_service: Service,
        document_with_multiple_verification_methods: DidDocument,
        document_with_linked_domains_service: DidDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                DocumentEvent::DocumentCreated {
                    document_id: document_id.clone(),
                    did_method,
                    document,
                    status: Status::SignAndValidate,
                    with_fixed_algorithm: None,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_multiple_verification_methods,
                },
            ])
            .when_async(DocumentCommand::AddService {
                service: Box::new(linked_domains_service),
                service_id: LINKED_DOMAINS_SERVICE_ID.to_string(),
            })
            .await
            .then_expect_events(vec![DocumentEvent::ServiceAdded {
                document_id: document_id.clone(),
                document: document_with_linked_domains_service,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_set_status(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        document_with_multiple_verification_methods: DidDocument,
        document_with_linked_domains_service: DidDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                DocumentEvent::DocumentCreated {
                    document_id: document_id.clone(),
                    did_method,
                    document,
                    status: Status::SignAndValidate,
                    with_fixed_algorithm: None,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_multiple_verification_methods.clone(),
                },
                DocumentEvent::ServiceAdded {
                    document_id: document_id.clone(),
                    document: document_with_linked_domains_service,
                },
            ])
            .when_async(DocumentCommand::UpdateDocumentStatus {
                status: Status::Disabled,
            })
            .await
            .then_expect_events(vec![DocumentEvent::DocumentStatusUpdated {
                document_id,
                status: Status::Disabled,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn did_web_documents_keep_their_context(document_id: String) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when_async(DocumentCommand::CreateDocument {
                document_id,
                did_method: SupportedDidMethod::Web,
                with_fixed_algorithm: None,
            })
            .await
            .inspect_result()
            .unwrap();

        let [DocumentEvent::DocumentCreated { document, .. }] = events.as_slice() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap()["@context"],
            serde_json::json!([
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/suites/jws-2020/v1"
            ])
        );
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn overwrite_rejects_an_authorization_for_another_did(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id,
                did_method,
                document,
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
            }])
            .when_async(DocumentCommand::OverwritePreviousDidWeb {
                previous_did: "did:web:other.example".parse().unwrap(),
                public_url: "https://new.example".parse().unwrap(),
            })
            .await
            .then_expect_error_message(
                "Invalid DID: Overwrite authorization for did:web:other.example does not match the current DID \
                 did:web:my-domain.example.org",
            )
    }

    /// A KMS key's public key, naming its algorithm.
    fn kms_public_key() -> Jwk {
        serde_json::from_value(serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "alg": "ES256",
            "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
            "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
        }))
        .unwrap()
    }

    /// The verification method [`kms_public_key`] is added as.
    fn kms_method() -> DidUrl {
        "did:web:my-domain.example.org#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U"
            .parse()
            .unwrap()
    }

    fn configured_es256_method() -> DidUrl {
        es256_verification_method::default().id
    }

    /// A `did:web` document with the verification methods of the configured issuer keys.
    fn did_web_document() -> Vec<DocumentEvent> {
        vec![
            DocumentEvent::DocumentCreated {
                document_id: document_id::default(),
                did_method: SupportedDidMethod::Web,
                document: document::default(),
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
            },
            DocumentEvent::PublicKeyUpdated {
                document_id: document_id::default(),
                document: document_with_multiple_verification_methods::default(),
            },
        ]
    }

    /// [`did_web_document`] with `method` added to it.
    fn with_method(method: VerificationMethod) -> DidDocument {
        let mut document = document_with_multiple_verification_methods::default();
        document
            .insert_verification_method(method, VerificationRelationship::ALL.iter().copied())
            .unwrap();
        document
    }

    fn kms_key_added() -> DocumentEvent {
        DocumentEvent::PublicKeyAdded {
            document_id: document_id::default(),
            verification_method_id: kms_method(),
            key_id: "key-id".to_string(),
            document: with_method(VerificationMethod::new(
                kms_method(),
                MethodType::JSON_WEB_KEY_2020,
                document::default().id,
                VerificationMaterial::jwk(&kms_public_key().with_kid("oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U")),
            )),
        }
    }

    fn kms_key_selected() -> DocumentEvent {
        DocumentEvent::SigningKeySelected {
            document_id: document_id::default(),
            algorithm: Algorithm::ES256,
            verification_method_id: kms_method(),
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn adds_a_public_key_named_by_its_thumbprint() {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given(did_web_document())
            .when_async(DocumentCommand::AddPublicKey {
                public_key: Box::new(kms_public_key()),
                key_id: "key-id".to_string(),
            })
            .await
            .inspect_result()
            .unwrap();

        let [DocumentEvent::PublicKeyAdded {
            verification_method_id, ..
        }] = events.as_slice()
        else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(*verification_method_id, kms_method());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn adding_a_public_key_again_adds_nothing() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added()]].concat())
            .when_async(DocumentCommand::AddPublicKey {
                public_key: Box::new(kms_public_key()),
                key_id: "key-id".to_string(),
            })
            .await
            .then_expect_events(vec![]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_public_key_to_add_must_name_its_algorithm() {
        let mut public_key = serde_json::to_value(kms_public_key()).unwrap();
        public_key.as_object_mut().unwrap().remove("alg");

        DocumentTestFramework::with(IdentityServices::default())
            .given(did_web_document())
            .when_async(DocumentCommand::AddPublicKey {
                public_key: Box::new(serde_json::from_value(public_key).unwrap()),
                key_id: "key-id".to_string(),
            })
            .await
            .then_expect_error_message(
                "The key of `did:web:my-domain.example.org#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U` names no \
                 supported algorithm",
            );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn only_did_web_documents_add_public_keys() {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id: document_id::default(),
                did_method: SupportedDidMethod::Key,
                document: DidDocument::new(
                    "did:key:zDnaeRwT4g6AZCHzxvNL7DLjqTaT88am4XR6TUGrKr6DXj6Tz"
                        .parse()
                        .unwrap(),
                ),
                status: Status::SignAndValidate,
                with_fixed_algorithm: Some(Algorithm::ES256),
            }])
            .when_async(DocumentCommand::AddPublicKey {
                public_key: Box::new(kms_public_key()),
                key_id: "key-id".to_string(),
            })
            .await
            .then_expect_error_message("Only `did:web` documents add public keys");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn updating_the_public_keys_keeps_the_added_ones() {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added()]].concat())
            .when_async(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
            .await
            .inspect_result()
            .unwrap();

        let Some(DocumentEvent::PublicKeyUpdated { document, .. }) = events.last() else {
            panic!("unexpected events: {events:?}");
        };
        assert!(document.verification_method(&kms_method()).is_some());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn removes_an_added_public_key() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added()]].concat())
            .when_async(DocumentCommand::RemovePublicKey {
                verification_method_id: kms_method(),
            })
            .await
            .then_expect_events(vec![DocumentEvent::PublicKeyRemoved {
                document_id: document_id::default(),
                verification_method_id: kms_method(),
                document: document_with_multiple_verification_methods::default(),
            }]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn refuses_to_remove_a_configured_key() {
        DocumentTestFramework::with(IdentityServices::default())
            .given(did_web_document())
            .when_async(DocumentCommand::RemovePublicKey {
                verification_method_id: configured_es256_method(),
            })
            .await
            .then_expect_error_message(
                "`did:web:my-domain.example.org#oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg` is not the verification \
                 method of an added key",
            );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn refuses_to_remove_the_selected_signing_key() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added(), kms_key_selected()]].concat())
            .when_async(DocumentCommand::RemovePublicKey {
                verification_method_id: kms_method(),
            })
            .await
            .then_expect_error_message(
                "`did:web:my-domain.example.org#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U` is the selected signing \
                 key of its algorithm; select another key or deselect it first",
            );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn selects_a_signing_key_for_the_algorithm_of_its_verification_method() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added()]].concat())
            .when_async(DocumentCommand::SelectSigningKey {
                verification_method_id: kms_method(),
            })
            .await
            .then_expect_events(vec![kms_key_selected()]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn selecting_the_selected_signing_key_again_selects_nothing() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added(), kms_key_selected()]].concat())
            .when_async(DocumentCommand::SelectSigningKey {
                verification_method_id: kms_method(),
            })
            .await
            .then_expect_events(vec![]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn only_a_verification_method_of_the_document_can_sign() {
        DocumentTestFramework::with(IdentityServices::default())
            .given(did_web_document())
            .when_async(DocumentCommand::SelectSigningKey {
                verification_method_id: kms_method(),
            })
            .await
            .then_expect_error_message(
                "The document has no verification method \
                 `did:web:my-domain.example.org#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U`",
            );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_signing_key_must_name_its_algorithm() {
        let method: DidUrl = "did:web:my-domain.example.org#no-algorithm".parse().unwrap();
        let mut public_key = serde_json::to_value(kms_public_key()).unwrap();
        public_key.as_object_mut().unwrap().remove("alg");

        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                did_web_document().remove(0),
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id::default(),
                    document: with_method(VerificationMethod::new(
                        method.clone(),
                        MethodType::JSON_WEB_KEY_2020,
                        document::default().id,
                        VerificationMaterial::jwk(&serde_json::from_value(public_key).unwrap()),
                    )),
                },
            ])
            .when_async(DocumentCommand::SelectSigningKey {
                verification_method_id: method,
            })
            .await
            .then_expect_error_message(
                "The key of `did:web:my-domain.example.org#no-algorithm` names no supported algorithm",
            );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn deselects_the_signing_key_of_an_algorithm() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added(), kms_key_selected()]].concat())
            .when_async(DocumentCommand::DeselectSigningKey {
                algorithm: Algorithm::ES256,
            })
            .await
            .then_expect_events(vec![DocumentEvent::SigningKeyDeselected {
                document_id: document_id::default(),
                algorithm: Algorithm::ES256,
            }]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn deselecting_an_algorithm_without_a_selected_key_deselects_nothing() {
        DocumentTestFramework::with(IdentityServices::default())
            .given([did_web_document(), vec![kms_key_added()]].concat())
            .when_async(DocumentCommand::DeselectSigningKey {
                algorithm: Algorithm::ES256,
            })
            .await
            .then_expect_events(vec![]);
    }

    #[test]
    fn overwriting_the_did_moves_the_added_keys_and_the_selection_to_it() {
        let mut aggregate = Document::default();
        for event in [did_web_document(), vec![kms_key_added(), kms_key_selected()]].concat() {
            aggregate.apply(event);
        }

        let mut document = aggregate.document.clone().unwrap();
        let new_did: Did = "did:web:new.example".parse().unwrap();
        document.map_dids(|_| new_did.clone());
        aggregate.apply(DocumentEvent::DocumentDidWebOverwritten {
            document_id: document_id::default(),
            previous_did: "did:web:my-domain.example.org".parse().unwrap(),
            document,
        });

        let moved: DidUrl = "did:web:new.example#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U"
            .parse()
            .unwrap();
        assert_eq!(
            (
                aggregate.added_verification_methods,
                aggregate.added_keys.keys().cloned().collect::<Vec<_>>(),
                aggregate.signing_methods.values().cloned().collect::<Vec<_>>(),
            ),
            (vec![moved.clone()], vec![moved.clone()], vec![moved])
        );
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::get_context;
    use crate::state::LINKED_DOMAINS_SERVICE_ID;
    use agent_shared::config::{config, SupportedDidMethod};
    use identity_kit::did::{
        Did, DidDocument, MethodType, Service, VerificationMaterial, VerificationMethod, VerificationRelationship,
    };
    use identity_kit::jose::Jwk;
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn document_id() -> String {
        "document_id".to_string()
    }

    #[fixture]
    pub fn did_method() -> SupportedDidMethod {
        SupportedDidMethod::Web
    }

    fn controller() -> Did {
        "did:web:my-domain.example.org".parse().unwrap()
    }

    #[fixture]
    pub fn document() -> DidDocument {
        DidDocument::builder(controller())
            .context(get_context(&MethodType::JSON_WEB_KEY_2020))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn es256_verification_method() -> VerificationMethod {
        VerificationMethod::new(
            "did:web:my-domain.example.org#oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg"
                .parse()
                .unwrap(),
            MethodType::JSON_WEB_KEY_2020,
            controller(),
            VerificationMaterial::jwk(
                &serde_json::from_value::<Jwk>(json!({
                    "kty": "EC",
                    "alg": "ES256",
                    "kid": "oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg",
                    "crv": "P-256",
                    "x": "Fmk13gO2SGLbuXeL24qJPHCNncnI6lBu6ZQL2EVZv4E",
                    "y": "fz2KvMhufzUpMeL9-K2re9fwA3mzg1bpfbceIQSuihY"
                }))
                .unwrap(),
            ),
        )
    }

    #[fixture]
    pub fn eddsa_verification_method() -> VerificationMethod {
        VerificationMethod::new(
            "did:web:my-domain.example.org#bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0"
                .parse()
                .unwrap(),
            MethodType::JSON_WEB_KEY_2020,
            controller(),
            VerificationMaterial::jwk(
                &serde_json::from_value::<Jwk>(json!({
                    "kty": "OKP",
                    "alg": "EdDSA",
                    "kid": "bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0",
                    "crv": "Ed25519",
                    "x": "GlnK9ePs802XxAglROQzoGurm9Qpv0IFPEbdMCILN_U"
                }))
                .unwrap(),
            ),
        )
    }

    #[fixture]
    pub fn both_verification_methods(
        es256_verification_method: VerificationMethod,
        eddsa_verification_method: VerificationMethod,
    ) -> Vec<VerificationMethod> {
        vec![es256_verification_method, eddsa_verification_method]
    }

    #[fixture]
    pub fn document_with_single_verification_method(
        mut document: DidDocument,
        es256_verification_method: VerificationMethod,
    ) -> DidDocument {
        // Attach all possible relationships to the verification method.
        document
            .insert_verification_method(es256_verification_method, VerificationRelationship::ALL.iter().copied())
            .unwrap();

        document
    }

    #[fixture]
    pub fn document_with_multiple_verification_methods(
        mut document: DidDocument,
        both_verification_methods: Vec<VerificationMethod>,
    ) -> DidDocument {
        for verification_method in both_verification_methods {
            document
                .insert_verification_method(verification_method, VerificationRelationship::ALL.iter().copied())
                .unwrap();
        }

        document
    }

    #[fixture]
    pub fn linked_domains_service() -> Service {
        serde_json::from_value(json!({
            "id": format!("did:web:my-domain.example.org#{LINKED_DOMAINS_SERVICE_ID}"),
            "type": "LinkedDomains",
            // An origin as UniCore writes it: a URL with an empty path.
            "serviceEndpoint": {
                "origins": [format!("{}/", config().public_url.origin().ascii_serialization())],
            },
        }))
        .unwrap()
    }

    #[fixture]
    pub fn document_with_linked_domains_service(
        mut document_with_multiple_verification_methods: DidDocument,
        linked_domains_service: Service,
    ) -> DidDocument {
        document_with_multiple_verification_methods
            .insert_service(linked_domains_service)
            .unwrap();

        document_with_multiple_verification_methods
    }
}
