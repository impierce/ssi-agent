use agent_shared::config::BITS_PER_STATUS;
use utoipa::openapi::{
    schema::SchemaType, AllOfBuilder, Array, ArrayBuilder, Object, ObjectBuilder, OneOfBuilder, Ref, Schema,
    SchemaFormat, Type,
};

/// A status of the IETF OAuth Token Status List: the name of a registered status or the status value, up to the largest
/// value the status list holds.
pub fn status_type() -> Schema {
    Schema::OneOf(
        OneOfBuilder::new()
            .item(
                ObjectBuilder::new()
                    .schema_type(SchemaType::Type(Type::String))
                    .enum_values(Some(["VALID", "INVALID", "SUSPENDED"])),
            )
            .item(
                ObjectBuilder::new()
                    .schema_type(SchemaType::Type(Type::Integer))
                    .minimum(Some(0))
                    .maximum(Some((1u16 << BITS_PER_STATUS) - 1)),
            )
            .build(),
    )
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

/// A credential's content: the shared `Data`, whose `raw` content embeds the `refreshService` of an opted-in
/// credential.
pub(crate) fn credential_data() -> Schema {
    let refreshable_content = ObjectBuilder::new().schema_type(SchemaType::AnyValue).property(
        "raw",
        ObjectBuilder::new()
            .schema_type(SchemaType::AnyValue)
            .property("refreshService", Ref::from_schema_name("CredentialRefreshService")),
    );
    Schema::OneOf(
        OneOfBuilder::new()
            .item(
                AllOfBuilder::new()
                    .item(Ref::from_schema_name("Data"))
                    .item(refreshable_content),
            )
            .item(ObjectBuilder::new().schema_type(SchemaType::Type(Type::Null)))
            .build(),
    )
}

pub(crate) fn credential_configurations_supported() -> Object {
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::Object))
        .property(
            "format",
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::String))
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
        .property(
            "refreshService",
            Schema::OneOf(
                OneOfBuilder::new()
                    .item(Ref::from_schema_name("RefreshServiceConfiguration"))
                    .item(ObjectBuilder::new().schema_type(SchemaType::Type(Type::Null)))
                    .build(),
            ),
        )
        .required("format")
        .build()
}

// TODO: refactor: move to a more generic place (such as `/v0/openapi.rs`) and make a reusable Schema
fn display() -> Array {
    ArrayBuilder::new()
        .items(
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
                .build(),
        )
        .build()
}
