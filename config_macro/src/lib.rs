use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, token, Data, DeriveInput, Error, Fields, Ident, Lit};

#[proc_macro_derive(Config, attributes(config))]
pub fn config_derive(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct
    let struct_name = input.ident;

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
    let mut a = Vec::new();
    let mut b = Vec::new();

    // Iterate over each field in the struct
    for field in fields {
        if field.attrs.iter().any(|attr| attr.path().is_ident("config")) {
            let field_type = field.ty;
            let field_name = field.ident.unwrap();
            let field_name_str = field_name.to_string();
            let type_config = Ident::new(&format!("{field_name}_config"), field_name.span());
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
                                let lit_str = meta.value()?.parse::<Lit>()?;
                                if let Lit::Str(lit_str) = lit_str {
                                    default_value = Some(lit_str.value());
                                } else {
                                    return Err(Error::new_spanned(lit_str, "Expected a string literal"));
                                }
                            }
                        } else if meta.path.is_ident("development_default") {
                            let lit_str = meta.value()?.parse::<Lit>()?;
                            if let Lit::Str(lit_str) = lit_str {
                                development_default_value = Some(lit_str.value());
                            } else {
                                return Err(Error::new_spanned(lit_str, "Expected a string literal"));
                            }
                        } else if meta.path.is_ident("production_default") {
                            let lit_str = meta.value()?.parse::<Lit>()?;
                            if let Lit::Str(lit_str) = lit_str {
                                production_default_value = Some(lit_str.value());
                            } else {
                                return Err(Error::new_spanned(lit_str, "Expected a string literal"));
                            }
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

            // Add code to initialize the field during loading
            a.push(quote! {
                let (provisioned, #type_config) = #fn_field_name(&provisioned_config, &application_profile).unwrap();

                metadata.insert(#field_name_str.to_string(), provisioned);
            });

            // Add code to construct the struct
            b.push(quote! {
                #field_name: #type_config,
            });

            // Generate the function to load the field
            generated_code.push(quote! {
                pub fn #fn_field_name(
                    provisioned_config: &config::Config,
                    application_profile: &ApplicationProfile,
                ) -> Result<(bool, #field_type), SharedError> {
                    // Load the provisioned value if it exists
                    let provisioned_value: Option<(bool, #field_type)> = if let Ok(value) = provisioned_config.get::<config::Value>(#field_name_str) {
                        let inner = value
                            .try_deserialize::<#field_type>()
                            .map_err(|e| SharedError::ConfigurationNotSuitableForProduction(e.to_string()))?;

                        Some((true, inner))
                    } else {
                        None // No provisioned value found
                    };

                    // Use the provisioned value or fall back to defaults
                    provisioned_value
                        .or_else(|| {
                            let inner = match application_profile {
                                ApplicationProfile::Development => #development_default,
                                ApplicationProfile::Production => #production_default,
                            }
                            .or_else(|| #default);

                            inner.map(|inner| (false, inner))
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
        pub static PROVISIONING_METADATA: Lazy<RwLock<HashMap<String, bool>>> = Lazy::new(|| RwLock::new(HashMap::new()));

        // Static configuration instance
        pub static CONFIG: Lazy<RwLock<#struct_name>> =
            Lazy::new(|| RwLock::new(#struct_name::load().unwrap()));

        // Accessor for the configuration
        pub fn config() -> RwLockReadGuard<'static, #struct_name> {
            CONFIG.read().unwrap()
        }

        impl #struct_name {
            // Load the configuration with provisioned values and defaults
            pub fn load2(
                provisioned_config: config::Config,
                application_profile: ApplicationProfile,
            ) -> Result<Self, SharedError> {
                let mut metadata = PROVISIONING_METADATA.write().unwrap();

                #(#a)*

                Ok(#struct_name {
                    #(#b)*
                })
            }

            // Convert the configuration to a provisioned JSON object
            pub fn to_provisioned_config(&self) -> serde_json::Value {
                let mut provisioned_config = json!({});

                #(#field_checks)*

                provisioned_config
            }

            // Check if a field is provisioned
            fn is_provisioned(field: &str) -> bool {
                PROVISIONING_METADATA
                    .read()
                    .unwrap()
                    .get(field)
                    .cloned()
                    .unwrap_or(false)
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
