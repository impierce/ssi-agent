use std::borrow::Cow;

use utoipa::{
    openapi::{
        schema::{AdditionalProperties, SchemaType},
        ArrayBuilder, KnownFormat, Object, ObjectBuilder, OneOfBuilder, Ref, RefOr, Schema, SchemaFormat, Type,
    },
    PartialSchema, ToSchema,
};

pub(crate) fn algorithm() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(["ES256", "EdDSA"]))
        .build()
}

/// OpenAPI representation of `identity_document::document::CoreDocument`.
///
/// Describes the properties defined by [DID Core](https://www.w3.org/TR/did-core/#core-properties). Additional
/// properties are allowed because a DID document may carry extension properties.
pub struct DidDocument;

impl PartialSchema for DidDocument {
    fn schema() -> RefOr<Schema> {
        let verification_relationship = || {
            ArrayBuilder::new().items(
                OneOfBuilder::new()
                    .item(string("A reference to a verification method."))
                    .item(Ref::from_schema_name(DidVerificationMethod::name())),
            )
        };

        ObjectBuilder::new()
            .description(Some("A DID document as defined by W3C DID Core."))
            .property("id", string("The DID subject."))
            .required("id")
            .property(
                "controller",
                OneOfBuilder::new()
                    .item(string("The DID of the controller."))
                    .item(ArrayBuilder::new().items(string("The DID of a controller."))),
            )
            .property("alsoKnownAs", ArrayBuilder::new().items(uri()))
            .property(
                "verificationMethod",
                ArrayBuilder::new().items(Ref::from_schema_name(DidVerificationMethod::name())),
            )
            .property("authentication", verification_relationship())
            .property("assertionMethod", verification_relationship())
            .property("keyAgreement", verification_relationship())
            .property("capabilityInvocation", verification_relationship())
            .property("capabilityDelegation", verification_relationship())
            .property(
                "service",
                ArrayBuilder::new().items(Ref::from_schema_name(DidService::name())),
            )
            .additional_properties(Some(AdditionalProperties::FreeForm(true)))
            .into()
    }
}

impl ToSchema for DidDocument {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("DidDocument")
    }

    fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
        schemas.extend([
            (DidVerificationMethod::name().into(), DidVerificationMethod::schema()),
            (DidService::name().into(), DidService::schema()),
        ]);
    }
}

/// A [verification method](https://www.w3.org/TR/did-core/#verification-methods) of a DID document.
pub struct DidVerificationMethod;

impl PartialSchema for DidVerificationMethod {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .description(Some("A verification method as defined by W3C DID Core."))
            .property("id", string("The DID URL identifying the verification method."))
            .required("id")
            .property("type", string("The verification method type, e.g. `JsonWebKey2020`."))
            .required("type")
            .property("controller", string("The DID of the controller."))
            .required("controller")
            .property(
                "publicKeyJwk",
                ObjectBuilder::new()
                    .description(Some("The public key as a JSON Web Key."))
                    .additional_properties(Some(AdditionalProperties::FreeForm(true))),
            )
            .property("publicKeyMultibase", string("The multibase-encoded public key."))
            .additional_properties(Some(AdditionalProperties::FreeForm(true)))
            .into()
    }
}

impl ToSchema for DidVerificationMethod {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("DidVerificationMethod")
    }
}

/// A [service](https://www.w3.org/TR/did-core/#services) of a DID document.
pub struct DidService;

impl PartialSchema for DidService {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .description(Some("A service as defined by W3C DID Core."))
            .property("id", string("The DID URL identifying the service."))
            .required("id")
            .property(
                "type",
                OneOfBuilder::new()
                    .item(string("The service type, e.g. `LinkedDomains`."))
                    .item(ArrayBuilder::new().items(string("A service type."))),
            )
            .required("type")
            .property(
                "serviceEndpoint",
                OneOfBuilder::new()
                    .item(uri())
                    .item(ArrayBuilder::new().items(uri()))
                    .item(
                        ObjectBuilder::new()
                            .description(Some("A map of service endpoints."))
                            .additional_properties(Some(AdditionalProperties::FreeForm(true))),
                    ),
            )
            .required("serviceEndpoint")
            .additional_properties(Some(AdditionalProperties::FreeForm(true)))
            .into()
    }
}

impl ToSchema for DidService {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("DidService")
    }
}

fn string(description: &str) -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .description(Some(description))
        .build()
}

fn uri() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uri)))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_document::document::CoreDocument;
    use serde_json::json;

    #[test]
    fn did_document_schema_describes_core_document_properties() {
        let schema = serde_json::to_value(DidDocument::schema()).unwrap();

        assert_eq!(schema["required"], json!(["id"]));
        assert_eq!(schema["additionalProperties"], json!(true));

        let document: CoreDocument = serde_json::from_value(json!({
            "id": "did:web:example.org",
            "verificationMethod": [{
                "id": "did:web:example.org#key-0",
                "type": "JsonWebKey2020",
                "controller": "did:web:example.org",
                "publicKeyJwk": { "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo" }
            }],
            "assertionMethod": ["did:web:example.org#key-0"],
            "service": [{
                "id": "did:web:example.org#linked-domain",
                "type": "LinkedDomains",
                "serviceEndpoint": "https://example.org/"
            }]
        }))
        .unwrap();
        let serialized = serde_json::to_value(document).unwrap();

        let properties = schema["properties"].as_object().unwrap();
        for property in serialized.as_object().unwrap().keys() {
            assert!(properties.contains_key(property), "`{property}` is not described");
        }
    }
}
