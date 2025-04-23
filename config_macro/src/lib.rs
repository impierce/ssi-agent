use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{meta::ParseNestedMeta, parse_macro_input, token, Data, DeriveInput, Error, Fields, Ident, Lit};

#[proc_macro_derive(Config, attributes(config))]
pub fn config_derive(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct
    let struct_name = input.ident;

    let provisioning_metadata_name = format_ident!("{}_PROVISIONING_METADATA", struct_name.to_string().to_uppercase());

    // Ensure the input is a struct with named fields
    let fields = if let Data::Struct(data) = input.data {
        if let Fields::Named(fields) = data.fields {
            fields.named
        } else {
            panic!("Config can only be derived for structs with named fields");
        }
    } else {
        panic!("Config can only be derived for structs");
    };

    // Vectors to store generated code for different parts of the implementation
    let mut generated_code = Vec::new();
    let mut field_checks = Vec::new();
    let mut b = Vec::new();

    // Iterate over each field in the struct
    for field in fields {
        if field.attrs.iter().any(|attr| attr.path().is_ident("config")) {
            let field_type = field.ty;
            let field_name = field.ident.unwrap();
            let field_name_str = field_name.to_string();
            let fn_field_name = Ident::new(&format!("fn_{field_name}"), field_name.span());

            // Generate code to check if the field is provisioned and add it to the JSON object
            field_checks.push(quote! {
                if Self::is_provisioned(#field_name_str) {
                    provisioned_config[#field_name_str] = json!(self.#field_name);
                }
            });

            // Parse the `#[config]` attribute to extract default values
            let mut default_value = None;
            let mut development_default_value = None;
            let mut production_default_value = None;

            for attr in &field.attrs {
                if attr.path().is_ident("config") {
                    attr.parse_nested_meta(|meta| {
                        if meta.path.is_ident("default") {
                            if !meta.input.peek(token::Eq) {
                                default_value = Some("Default::default()".to_string());
                            } else {
                                default_value = parse_string_literal(&meta)?;
                            }
                        } else if meta.path.is_ident("development_default") {
                            development_default_value = parse_string_literal(&meta)?;
                        } else if meta.path.is_ident("production_default") {
                            production_default_value = parse_string_literal(&meta)?;
                        }
                        Ok(())
                    })
                    .unwrap();
                }
            }

            // Generate default values for the field
            let default = generate_default_value(default_value);
            let development_default = generate_default_value(development_default_value);
            let production_default = generate_default_value(production_default_value);

            // Add code to construct the struct
            b.push(quote! {
                #field_name: #fn_field_name(&provisioned_config, &application_profile).unwrap(),
                // .map_err(|e| {
                //     // FIXME!!
                //     ConfigError::Message(format!(
                //         "Configuration is not suitable for production: UniCore URL must be provided",
                //     ))
                // })?,
            });

            // Generate the function to load the field
            generated_code.push(quote! {
                pub fn #fn_field_name(
                    provisioned_config: &config::Config,
                    application_profile: &ApplicationProfile,
                ) -> Result<#field_type, SharedError> {
                    // Load the provisioned value if it exists
                    let provisioned_value: Option<#field_type> = if let Ok(value) = provisioned_config.get::<config::Value>(#field_name_str) {
                        let inner = value
                            .try_deserialize::<#field_type>()
                            .map_err(|e| SharedError::ConfigurationNotSuitableForProduction(e.to_string()))?;

                        Some(inner)
                    } else {
                        None // No provisioned value found
                    };

                    // Use the provisioned value or fall back to defaults
                    provisioned_value
                        .or_else(|| {
                            match application_profile {
                                ApplicationProfile::Development => #development_default,
                                ApplicationProfile::Production => #production_default,
                            }
                            .or_else(|| #default)
                        })
                        .ok_or_else(|| {
                            SharedError::ConfigurationNotSuitableForProduction(format!(
                                "No default value found for the configuration: {}",
                                stringify!(#field_name)
                            ))
                        })
                }
            });
        }
    }

    // Combine all generated implementations
    let expanded = quote! {
        // Static metadata for provisioning
        pub static #provisioning_metadata_name: Lazy<RwLock<serde_json::Value>> = Lazy::new(|| RwLock::new(json!({})));

        impl #struct_name {
            // Load the configuration with provisioned values and defaults
            pub fn load(
                provisioned_config: config::Config,
                application_profile: ApplicationProfile,
            ) -> Result<Self, ConfigError> {
                let mut metadata = #provisioning_metadata_name.write().unwrap();

                println!("Loading configuration for {}...", stringify!(#struct_name));
                println!("Provisioned config: {:#?}", provisioned_config);

                *metadata = provisioned_config.clone().try_deserialize().unwrap();

                let res = #struct_name {
                    #(#b)*
                };

                // Overwrite all fields from res into metadata
                overwrite_existing_fields(&mut metadata, &json!(res));

                // If the application is running in production mode, the configuration is validated.
                match application_profile {
                    ApplicationProfile::Production => res
                        .validate()
                        .map_err(|e| ConfigError::Message(e.to_string()))?,
                    _ => {}
                }

                Ok(res)
            }

            // Convert the configuration to a provisioned JSON object
            pub fn get_provisioned_config(&self) -> serde_json::Value {
                #provisioning_metadata_name.read().unwrap().clone()
            }
        }

        // Generated functions for each field
        #(#generated_code)*
    };

    // Convert the generated code into a TokenStream and return it
    TokenStream::from(expanded)
}

// Helper function to generate default values
fn generate_default_value(default_value: Option<String>) -> proc_macro2::TokenStream {
    if let Some(value) = default_value {
        let parsed_default: proc_macro2::TokenStream = value.parse().unwrap();
        quote! { Some(#parsed_default) }
    } else {
        quote! { None }
    }
}

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
