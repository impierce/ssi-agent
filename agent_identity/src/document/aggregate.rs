use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::document::openapi::algorithm;
use crate::services::IdentityServices;
use agent_secret_manager::subject::StorageKey;
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::did::{
    Context, Did, DidDocument, MethodType, VerificationMaterial, VerificationMethod, VerificationRelationship,
};
use identity_kit::jose::Jwk;
use identity_kit::methods::{jwk::DidJwk, key::DidKey};
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

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

        info!("Handling command: {:?}", command);

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

                // Remove all existing verification methods from the document.
                let method_ids = document
                    .methods(None)
                    .map(|method| method.id.clone())
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
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        debug!("Applying event: {:?}", event);

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
            PublicKeyUpdated { document_id, document }
            | DocumentDidWebOverwritten {
                document_id, document, ..
            } => {
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
        }
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
    async fn test_create_document(document_id: String, did_method: SupportedDidMethod, document: DidDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id: document_id.clone(),
                did_method,
                with_fixed_algorithm: None,
            })
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
    async fn test_create_document_derives_the_did_from_the_issuer_key(
        document_id: String,
        #[case] did_method: SupportedDidMethod,
        #[case] algorithm: Algorithm,
        #[case] expected_did: &str,
    ) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id,
                did_method,
                with_fixed_algorithm: Some(algorithm),
            })
            .inspect_result()
            .unwrap();

        let [DocumentEvent::DocumentCreated { document, .. }] = events.as_slice() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(document.id.as_str(), expected_did);
    }

    #[rstest]
    #[serial_test::serial]
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
            .when(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
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
            .when(DocumentCommand::AddService {
                service: Box::new(linked_domains_service),
                service_id: LINKED_DOMAINS_SERVICE_ID.to_string(),
            })
            .then_expect_events(vec![DocumentEvent::ServiceAdded {
                document_id: document_id.clone(),
                document: document_with_linked_domains_service,
            }])
    }

    #[rstest]
    #[serial_test::serial]
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
            .when(DocumentCommand::UpdateDocumentStatus {
                status: Status::Disabled,
            })
            .then_expect_events(vec![DocumentEvent::DocumentStatusUpdated {
                document_id,
                status: Status::Disabled,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn did_web_documents_keep_their_context(document_id: String) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id,
                did_method: SupportedDidMethod::Web,
                with_fixed_algorithm: None,
            })
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
            .when(DocumentCommand::OverwritePreviousDidWeb {
                previous_did: "did:web:other.example".parse().unwrap(),
                public_url: "https://new.example".parse().unwrap(),
            })
            .then_expect_error_message(
                "Invalid DID: Overwrite authorization for did:web:other.example does not match the current DID \
                 did:web:my-domain.example.org",
            )
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
