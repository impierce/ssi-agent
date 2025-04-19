use case::CaseExt;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Error, Fields, Lit};

#[proc_macro_derive(ConfigImpl, attributes(config_impl))]
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
            panic!("ConfigImpl can only be derived for structs with named fields");
        }
    } else {
        panic!("ConfigImpl can only be derived for structs");
    };

    // Generate new types and `ConfigImpl` implementations for fields marked with `#[config_impl]`
    let mut generated_code = Vec::new();
    for field in fields {
        if field.attrs.iter().any(|attr| attr.path().is_ident("config_impl")) {
            let field_name = field.ident.unwrap();
            let field_type = field.ty;

            // Parse the `#[config_impl]` attribute to extract the `default`, `development_default`, and `production_default` values
            let mut default_value = None;
            let mut development_default_value = None;
            let mut production_default_value = None;

            for attr in &field.attrs {
                if attr.path().is_ident("config_impl") {
                    attr.parse_nested_meta(|meta| {
                        if meta.path.is_ident("default") {
                            let lit_str = meta.value()?.parse::<Lit>()?;
                            if let Lit::Str(lit_str) = lit_str {
                                default_value = Some(lit_str.value());
                            } else {
                                return Err(Error::new_spanned(lit_str, "Expected a string literal"));
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

            // Generate the `default` method implementation
            let default_method = if let Some(default_value) = default_value {
                let parsed_default: proc_macro2::TokenStream = default_value.parse().unwrap();
                quote! {
                    fn default() -> Option<Self::Target> {
                        Some(#parsed_default)
                    }
                }
            } else {
                quote! {}
            };

            // Generate the `development_default` method implementation
            let development_default_method = if let Some(development_default_value) = development_default_value {
                let parsed_default: proc_macro2::TokenStream = development_default_value.parse().unwrap();
                quote! {
                    fn development_default() -> Option<Self::Target> {
                        Some(#parsed_default)
                    }
                }
            } else {
                quote! {}
            };

            // Generate the `production_default` method implementation
            let production_default_method = if let Some(production_default_value) = production_default_value {
                let parsed_default: proc_macro2::TokenStream = production_default_value.parse().unwrap();
                quote! {
                    fn production_default() -> Option<Self::Target> {
                        Some(#parsed_default)
                    }
                }
            } else {
                quote! {}
            };

            // Generate a new type based on the field name
            let type_name = syn::Ident::new(&format!("{}", field_name.to_string().to_camel()), field_name.span());

            // Generate the new type and its `ConfigImpl` implementation
            generated_code.push(quote! {
                #[derive(Debug, Clone, Serialize, Deserialize, derive_more::Deref)]
                pub struct #type_name {
                    pub #field_name: #field_type,
                }

                impl ConfigImpl for #type_name {
                    const NAME: &str = stringify!(#field_name);

                    fn from_inner(inner: Self::Target) -> Self {
                        #type_name { #field_name: inner }
                    }

                    #default_method
                    #development_default_method
                    #production_default_method
                }
            });
        }
    }

    // Combine all generated implementations
    let expanded = quote! {
        #(#generated_code)*
    };

    // Convert the generated code into a TokenStream and return it
    TokenStream::from(expanded)
}
