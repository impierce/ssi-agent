use utoipa::openapi::{schema::SchemaType, Array, ArrayBuilder, Object, ObjectBuilder, SchemaFormat, Type};

pub(crate) fn status_type() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(["VALID", "INVALID", "SUSPENDED", "UNDEFINED"]))
        .build()
}

pub(crate) fn holder_notifications() -> Array {
    ArrayBuilder::new()
        .description(Some("this is notification request"))
        .items(
            ObjectBuilder::new()
                .property(
                    "notification_id",
                    ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
                )
                .property(
                    "event",
                    ObjectBuilder::new()
                        .schema_type(Type::String)
                        .enum_values(Some([
                            "credential_accepted",
                            "credential_failure",
                            "credential_deleted",
                        ]))
                        .build(),
                )
                .property(
                    "event_description",
                    ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
                )
                .required("notification_id")
                .required("event")
                .build(),
        )
        .build()
}

pub(crate) fn credential_configurations_supported() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::Object))
        .property(
            "credential_format",
            ObjectBuilder::new()
                .enum_values(Some(["jwt_vc_json", "dc+sd-jwt", "vc+sd-jwt"]))
                .build(),
        )
        .property("scope", ObjectBuilder::new().build())
        .property(
            "cryptographic_binding_methods_supported",
            ArrayBuilder::new()
                .items(ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build())
                .build(),
        )
        .property(
            "credential_signing_alg_values_supported",
            ArrayBuilder::new()
                .items(ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build())
                .build(),
        )
        .property("proof_types_supported", ObjectBuilder::new().build())
        .property(
            "credential_metadata",
            ObjectBuilder::new()
                .property("claims", ArrayBuilder::new().build())
                .property("display", display())
                .build(),
        )
        .required("credential_format")
        .build()
}

// TODO: refactor: move to a more generic place (such as `/v0/openapi.rs`) and make a reusable Schema
fn display() -> Array {
    ArrayBuilder::new()
        .items(display_item())
        .build()
}

pub(crate) fn display_schema() -> Object {
    display_item()
}

fn display_item() -> Object {
    ObjectBuilder::new()
        .property(
            "name",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .property(
            "locale",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .property(
            "logo",
            ObjectBuilder::new()
                .property(
                    "uri",
                    ObjectBuilder::new()
                        .schema_type(SchemaType::Type(Type::String))
                        .format(Some(SchemaFormat::KnownFormat(utoipa::openapi::KnownFormat::Uri)))
                        .build(),
                )
                .property(
                    "alt_text",
                    ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
                )
                .required("uri")
                .build(),
        )
        .property(
            "description",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .property(
            "background_image",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .property(
            "background_color",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .property(
            "text_color",
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)).build(),
        )
        .required("name")
        .build()
}
