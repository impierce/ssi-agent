use utoipa::openapi::{schema::SchemaType, Object, ObjectBuilder, Type};

pub(crate) fn algorithm() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(["ES256", "EdDSA"]))
        .build()
}

// TODO: replace with actual type of `identity_document::document::CoreDocument`
pub(crate) fn core_document() -> Object {
    ObjectBuilder::new().build()
}
