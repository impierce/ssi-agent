use utoipa::openapi::{
    schema::{AdditionalProperties, SchemaType},
    ArrayBuilder, KnownFormat, Object, ObjectBuilder, OneOfBuilder, SchemaFormat, Type,
};

pub(super) fn credential() -> Object {
    ObjectBuilder::new()
        .property("@context", one_or_many(context()))
        .property("id", uri())
        .property("type", one_or_many(string().into()))
        .property("credentialSubject", one_or_many(free_form_object().into()))
        .property(
            "issuer",
            OneOfBuilder::new().item(uri()).item(
                ObjectBuilder::new()
                    .property("id", uri())
                    .required("id")
                    .additional_properties(Some(AdditionalProperties::FreeForm(true))),
            ),
        )
        .property("issuanceDate", date_time())
        .property("expirationDate", date_time())
        .property("credentialStatus", free_form_object())
        .property("credentialSchema", one_or_many(free_form_object().into()))
        .property("refreshService", one_or_many(free_form_object().into()))
        .property("termsOfUse", one_or_many(free_form_object().into()))
        .property("evidence", one_or_many(free_form_object().into()))
        .property("nonTransferable", boolean())
        .property("proof", free_form_object())
        .required("@context")
        .required("type")
        .required("credentialSubject")
        .required("issuer")
        .required("issuanceDate")
        .additional_properties(Some(AdditionalProperties::FreeForm(true)))
        .build()
}

fn one_or_many(item: utoipa::openapi::Schema) -> OneOfBuilder {
    let item = utoipa::openapi::RefOr::T(item);
    OneOfBuilder::new()
        .item(item.clone())
        .item(ArrayBuilder::new().items(item))
}

fn context() -> utoipa::openapi::Schema {
    OneOfBuilder::new().item(uri()).item(free_form_object()).build().into()
}

fn string() -> Object {
    ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build()
}

fn uri() -> Object {
    ObjectBuilder::from(string())
        .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uri)))
        .build()
}

fn date_time() -> Object {
    ObjectBuilder::from(string())
        .format(Some(SchemaFormat::KnownFormat(KnownFormat::DateTime)))
        .build()
}

fn boolean() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::Boolean))
        .build()
}

fn free_form_object() -> Object {
    ObjectBuilder::new()
        .additional_properties(Some(AdditionalProperties::FreeForm(true)))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_schema_exposes_credential_fields() {
        let schema = serde_json::to_value(credential()).unwrap();
        let credential = schema["properties"].as_object().unwrap();

        assert!(credential.contains_key("issuer"));
        assert!(credential.contains_key("credentialSubject"));
        assert!(credential.contains_key("issuanceDate"));
    }
}
