use proc_macro::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{meta::ParseNestedMeta, parse_macro_input, token, Data, DeriveInput, Error, Fields, Ident, Lit};

/// Holds default values for a struct field, including flags for attribute presence.
#[derive(Default)]
struct FieldDefaults {
    default: Option<String>,
    development: Option<String>,
    production: Option<String>,
    transform_with: Option<String>,
}

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
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct
    let struct_name = input.ident.clone();
    let provisioning_metadata_name = format_ident!("{}_PROVISIONING_METADATA", struct_name.to_string().to_uppercase());

    // Ensure the input is a struct with named fields
    let fields = if let Data::Struct(ref data) = input.data {
        if let Fields::Named(fields) = &data.fields {
            fields.named.to_owned()
        } else {
            return Error::new_spanned(input, "Config can only be derived for structs with named fields")
                .to_compile_error()
                .into();
        }
    } else {
        return Error::new_spanned(input, "Config can only be derived for structs")
            .to_compile_error()
            .into();
    };

    // Vectors to store generated code for different parts of the implementation
    let mut generated_code = Vec::new();
    let mut load_configuration_variables = Vec::new();

    // Iterate over each field in the struct
    for field in fields {
        if field.attrs.iter().any(|attr| attr.path().is_ident("config")) {
            let field_type = field.ty;
            let field_name = field.ident.expect("Field should have a name");
            let field_name_str = field_name.to_string();

            // Parse the `#[config]` attribute to extract default values into FieldDefaults
            let mut defaults = FieldDefaults::default();

            for attr in &field.attrs {
                if attr.path().is_ident("config") {
                    attr.parse_nested_meta(|meta| {
                        if meta.path.is_ident("default") {
                            if !meta.input.peek(token::Eq) {
                                defaults.default = Some("Default::default()".to_string());
                            } else {
                                defaults.default = parse_string_literal(&meta)?;
                            }
                        } else if meta.path.is_ident("development_default") {
                            defaults.development = parse_string_literal(&meta)?;
                        } else if meta.path.is_ident("production_default") {
                            defaults.production = parse_string_literal(&meta)?;
                        } else if meta.path.is_ident("transform_with") {
                            defaults.transform_with = parse_string_literal(&meta)?;
                        }
                        Ok(())
                    })
                    .ok();
                }
            }

            let (loader_fn, loader_call) = generate_field_loader(&field_name, &field_type, &field_name_str, &defaults);
            generated_code.push(loader_fn);
            load_configuration_variables.push(loader_call);
        }
    }

    // Combine all generated implementations
    let expanded = quote! {
        // Static metadata for provisioning
        pub static #provisioning_metadata_name: Lazy<RwLock<serde_json::Value>> = Lazy::new(|| RwLock::new(json!({})));

        impl #struct_name {
            /// Loads the configuration with provisioned values and defaults.
            ///
            /// Applies environment-specific defaults and validates the resulting struct.
            pub fn load(
                provisioned_config: config::Config,
                application_profile: ApplicationProfile,
            ) -> Result<Self, SharedError> {
                let mut metadata = #provisioning_metadata_name.write().unwrap();

                *metadata = provisioned_config.clone().try_deserialize().expect("Failed to deserialize config");

                let res = #struct_name {
                    #(#load_configuration_variables)*
                };

                // Overwrite all fields from res into metadata
                overwrite_existing_fields(&mut metadata, &json!(res));

                // If the application is running in production mode, the configuration is validated.
                match application_profile {
                    ApplicationProfile::Development => res.validate_development()?,
                    ApplicationProfile::Production => res.validate()?,
                }

                Ok(res)
            }

            /// Converts the configuration to a provisioned JSON object.
            pub fn get_provisioned_config(&self) -> serde_json::Value {
                #provisioning_metadata_name.read().unwrap().clone()
            }

            // Generated functions for each field
            #(#generated_code)*
        }

        fn overwrite_existing_fields(a: &mut serde_json::Value, b: &serde_json::Value) {
            if let (Some(a_map), Some(b_map)) = (a.as_object_mut(), b.as_object()) {
                // Overwrite and recurse for keys that exist in both
                for (k, a_v) in a_map.iter_mut() {
                    if let Some(b_v) = b_map.get(k) {
                        overwrite_existing_fields(a_v, b_v);
                    }
                }
                // Remove keys from a that do not exist in b
                let keys_to_remove: Vec<_> = a_map.keys().filter(|k| !b_map.contains_key(*k)).cloned().collect();
                for k in keys_to_remove {
                    a_map.remove(&k);
                }
            } else {
                *a = b.clone();
            }
        }
    };

    // Convert the generated code into a TokenStream and return it
    TokenStream::from(expanded)
}

// Helper function to generate default values
fn generate_default_value(default_value: Option<String>) -> proc_macro2::TokenStream {
    if let Some(value) = default_value {
        let parsed_default: proc_macro2::TokenStream = value.parse().expect("Failed to parse default value");
        quote! { Some(#parsed_default) }
    } else {
        quote! { None }
    }
}

/// Parses a string literal from a nested meta attribute.
fn parse_string_literal(meta: &ParseNestedMeta) -> Result<Option<String>, syn::Error> {
    let lit_str = meta.value()?.parse::<Lit>()?;
    let default_value;
    if let Lit::Str(lit_str) = lit_str {
        default_value = Some(lit_str.value());
    } else {
        return Err(Error::new_spanned(lit_str, "Expected a string literal"));
    }

    Ok(default_value)
}

/// Generates the loader function and call for a struct field.
///
/// This function creates a loader function for the field that attempts to load the value from the provisioned config,
/// applies environment-specific and general defaults, and returns an error if no value is found.
fn generate_field_loader(
    field_name: &Ident,
    field_type: &syn::Type,
    field_name_str: &str,
    defaults: &FieldDefaults,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let default = generate_default_value(defaults.default.clone());
    let development_default = generate_default_value(defaults.development.clone());
    let production_default = generate_default_value(defaults.production.clone());
    let transform_with_fn = defaults
        .transform_with
        .as_ref()
        .map(|s| Ident::new(s, field_name.span()));
    let fn_field_name = Ident::new(&format!("fn_{}", field_name), field_name.span());
    let value_expr = if let Some(transform_fn) = &transform_with_fn {
        quote! { let value: #field_type = #transform_fn(config_value); }
    } else {
        quote! { let value: #field_type = config_value; }
    };
    let loader_fn = quote_spanned! { field_name.span() =>
        /// Loads the value for this field from the provisioned config or applies defaults.
        fn #fn_field_name(
            provisioned_config: &config::Config,
            application_profile: &ApplicationProfile,
        ) -> Result<#field_type, SharedError> {
            let provisioned_value: Option<#field_type> = if let Ok(value) = provisioned_config.get::<config::Value>(#field_name_str) {
                let inner = value
                    .try_deserialize::<#field_type>()
                    .map_err(|e| SharedError::GenericConfigurationError(stringify!(#field_name).to_string(), e.to_string()))?;
                Some(inner)
            } else {
                None
            };

            let config_value: #field_type = provisioned_value
                .or_else(|| {
                    match application_profile {
                        ApplicationProfile::Development => #development_default,
                        ApplicationProfile::Production => #production_default,
                    }
                    .or_else(|| #default)
                })
                .ok_or_else(|| {
                    match application_profile {
                        ApplicationProfile::Development => SharedError::MissingDefaultValueForDevelopment(stringify!(#field_name).to_string()),
                        ApplicationProfile::Production => {
                            SharedError::ConfigurationNotSuitableForProduction(format!(
                                "`{}` must be provided",
                                stringify!(#field_name)
                            ))
                        }
                    }
                })?;

            #value_expr

            Ok(value)
        }
    };
    let loader_call = quote! {
        #field_name: Self::#fn_field_name(&provisioned_config, &application_profile)?,
    };

    (loader_fn, loader_call)
}
