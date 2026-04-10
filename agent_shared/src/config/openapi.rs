use utoipa::openapi::{schema::SchemaType, Object, ObjectBuilder, Type};

pub(crate) fn credential_metadata() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(["VALID", "INVALID", "SUSPENDED", "UNDEFINED"]))
        .build()
}

pub(crate) fn authorization() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(["VALID", "INVALID", "SUSPENDED", "UNDEFINED"]))
        .build()
}
