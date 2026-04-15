use utoipa::openapi::{schema::SchemaType, Object, ObjectBuilder, Type};

// TODO: type properly
pub(crate) fn credential_metadata() -> Object {
    ObjectBuilder::new()
        .property("claims", ObjectBuilder::new().build())
        .property("display", ObjectBuilder::new().build())
        .build()
}

// TODO: type properly
pub(crate) fn authorization() -> Object {
    ObjectBuilder::new()
        .property(
            "pre_authorized",
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::Boolean))
                .build(),
        )
        .property("tx_code_constraints", ObjectBuilder::new().build())
        .required("pre_authorized")
        .build()
}
