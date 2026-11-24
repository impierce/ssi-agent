use crate::v0::kms::key_providers::{
    __path_add_key_provider, __path_get_all_key_providers, __path_get_key_provider_keys, __path_remove_key_provider,
    __path_test_key_provider_connection,
};
use crate::v0::kms::keys::{__path_adopt_key, __path_create_key, __path_delete_key, __path_get_all_keys};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        add_key_provider, get_all_key_providers, get_key_provider_keys, remove_key_provider, test_key_provider_connection,
        create_key, adopt_key, delete_key, get_all_keys
    ),
    tags(
        (name = "Key Management", description = "Connect key providers and create keys in them that never leave the provider. Only available when the `kms` feature flag is enabled.")
    )
)]
pub struct KmsApi;
