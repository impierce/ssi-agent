mod config;
mod integration_event;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

/// Procedural macro to derive configuration loading for structs.
///
/// This macro generates code to load configuration values from provisioned sources,
/// apply environment-specific defaults, and validate the resulting struct.
/// It supports the `#[config]` attribute for specifying default, development, and production values.
///
/// # Example
///
/// ```ignore
/// #[derive(Config)]
/// struct MyConfig {
///     #[config(default = "42", development_default = "100")]
///     value: u32,
/// }
/// ```
#[proc_macro_derive(Config, attributes(config))]
pub fn config_derive(input: TokenStream) -> TokenStream {
    config::expand_config(input)
}

/// Procedural macro to derive `shared_kernel::IntegrationEvent` and static documentation metadata (`Self::DOCS`)
/// for integration event enums.
///
/// Every enum, variant, and field must have a `///` doc comment (enforced at compile time).
///
/// # Attributes
///
/// - Enum level: `#[integration_event(source = "/services/...")]`
/// - Variant level: `#[integration_event(rename = "com.impierce.unicore....")]` (or `type = "..."`)
/// - Field level: `#[integration_event(subject)]` on the field that supplies the CloudEvent `subject`
#[proc_macro_derive(IntegrationEvent, attributes(integration_event))]
pub fn integration_event_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match integration_event::expand_integration_event(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
