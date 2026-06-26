use utoipa::openapi::{schema::SchemaType, Object, ObjectBuilder, Type};

// TODO: incomplete type
pub(crate) fn credential_metadata() -> Object {
    ObjectBuilder::new()
        .property("claims", ObjectBuilder::new().build())
        .property("display", ObjectBuilder::new().build())
        .build()
}

pub(crate) fn tx_code_constraints() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::Object))
        .property(
            "input_mode",
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::String))
                .enum_values(Some(["numeric", "text"]))
                .build(),
        )
        .property(
            "length",
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::Integer))
                .description(Some("Allows a pin-length of 0-255."))
                .build(),
        )
        .property(
            "description",
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::String))
                .description(Some("The length of the string must not exceed 300 characters."))
                .build(),
        )
        .build()
}
