use utoipa::openapi::{Object, ObjectBuilder};

// TODO: incomplete type
pub(crate) fn credential_metadata() -> Object {
    ObjectBuilder::new()
        .property("claims", ObjectBuilder::new().build())
        .property("display", ObjectBuilder::new().build())
        .build()
}
