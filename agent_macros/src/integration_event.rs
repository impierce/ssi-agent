use quote::{quote, ToTokens};
use syn::{Attribute, Data, DeriveInput, Error, Expr, Fields, GenericArgument, Ident, Lit, Meta, PathArguments, Type};

pub fn expand_integration_event(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let enum_doc = require_doc_comment(&input.attrs, &input.ident, "enum")?;
    let source = parse_enum_source(&input.attrs, &input.ident)?;

    let Data::Enum(data_enum) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "IntegrationEvent can only be derived for enums",
        ));
    };

    let mut event_type_arms = Vec::new();
    let mut subject_arms = Vec::new();
    let mut variant_docs = Vec::new();

    for variant in &data_enum.variants {
        let variant_ident = &variant.ident;
        let variant_name_str = variant_ident.to_string();
        let variant_doc = require_doc_comment(&variant.attrs, &variant.ident, "variant")?;
        let event_type = parse_variant_event_type(&variant.attrs, &variant.ident)?;

        let mut field_docs = Vec::new();
        let mut subject_field: Option<(Ident, bool)> = None;

        match &variant.fields {
            Fields::Named(named) => {
                for field in &named.named {
                    let field_ident = field.ident.clone().expect("named field has ident");
                    let field_name_str = field_ident.to_string().trim_start_matches("r#").to_string();
                    let field_doc = require_doc_comment(&field.attrs, &field_ident, "field")?;
                    let (inner_ty, required) = unwrap_option_type(&field.ty);
                    let ty_str = format_type(inner_ty);

                    if has_subject_attr(&field.attrs)? {
                        if subject_field.is_some() {
                            return Err(Error::new_spanned(
                                field,
                                "Only one field per variant may be marked with `#[integration_event(subject)]`",
                            ));
                        }
                        subject_field = Some((field_ident, required));
                    }

                    field_docs.push(quote! {
                        ::shared_kernel::integration_event::IntegrationEventFieldDoc {
                            name: #field_name_str,
                            ty: #ty_str,
                            required: #required,
                            description: #field_doc,
                        }
                    });
                }
            }
            Fields::Unit => {}
            Fields::Unnamed(_) => {
                return Err(Error::new_spanned(
                    variant,
                    "IntegrationEvent variants must use named fields or unit variants",
                ));
            }
        }

        event_type_arms.push(quote! {
            Self::#variant_ident { .. } => #event_type,
        });

        let subject_field_doc = if let Some((ref subj_ident, required)) = subject_field {
            let subj_str = subj_ident.to_string().trim_start_matches("r#").to_string();
            if required {
                subject_arms.push(quote! {
                    Self::#variant_ident { #subj_ident, .. } => ::std::option::Option::Some(::std::string::ToString::to_string(#subj_ident)),
                });
            } else {
                subject_arms.push(quote! {
                    Self::#variant_ident { #subj_ident, .. } => #subj_ident.as_ref().map(::std::string::ToString::to_string),
                });
            }
            quote! { ::std::option::Option::Some(#subj_str) }
        } else {
            subject_arms.push(quote! {
                Self::#variant_ident { .. } => ::std::option::Option::None,
            });
            quote! { ::std::option::Option::None }
        };

        variant_docs.push(quote! {
            ::shared_kernel::integration_event::IntegrationEventVariantDoc {
                variant_name: #variant_name_str,
                event_type: #event_type,
                description: #variant_doc,
                subject_field: #subject_field_doc,
                fields: &[ #(#field_docs),* ],
            }
        });
    }

    Ok(quote! {
        impl ::shared_kernel::IntegrationEvent for #enum_name {
            const SOURCE: &'static str = #source;

            const DOCS: ::shared_kernel::integration_event::IntegrationEventGroupDoc =
                ::shared_kernel::integration_event::IntegrationEventGroupDoc {
                    name: stringify!(#enum_name),
                    source: #source,
                    description: #enum_doc,
                    variants: &[
                        #(#variant_docs),*
                    ],
                };

            fn event_type(&self) -> &'static str {
                match self {
                    #(#event_type_arms)*
                }
            }

            fn subject(&self) -> ::std::option::Option<::std::string::String> {
                match self {
                    #(#subject_arms)*
                }
            }
        }
    })
}

/// Extracts `///` doc comments (which Rust desugars to `#[doc = "..."]` attributes) and fails
/// compilation if documentation is missing or empty.
fn require_doc_comment(attrs: &[Attribute], span_target: &impl ToTokens, item_kind: &str) -> syn::Result<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let Expr::Lit(expr_lit) = &nv.value {
                    if let Lit::Str(lit_str) = &expr_lit.lit {
                        let raw = lit_str.value();
                        let trimmed = raw.strip_prefix(' ').unwrap_or(&raw);
                        lines.push(trimmed.trim_end().to_string());
                    }
                }
            }
        }
    }
    let doc = lines.join("\n").trim().to_string();
    if doc.is_empty() {
        return Err(Error::new_spanned(
            span_target,
            format!("missing `///` doc comment on integration event {item_kind}"),
        ));
    }
    Ok(doc)
}

fn parse_enum_source(attrs: &[Attribute], span_target: &impl ToTokens) -> syn::Result<String> {
    let mut source = None;
    for attr in attrs {
        if attr.path().is_ident("integration_event") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("source") {
                    let lit: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = lit {
                        source = Some(s.value());
                        Ok(())
                    } else {
                        Err(meta.error("expected string literal for `source`"))
                    }
                } else {
                    Err(meta.error("unsupported enum-level `integration_event` attribute; expected `source = \"...\"`"))
                }
            })?;
        }
    }
    source.ok_or_else(|| {
        Error::new_spanned(
            span_target,
            "missing `#[integration_event(source = \"/services/...\")]` on enum",
        )
    })
}

fn parse_variant_event_type(attrs: &[Attribute], span_target: &impl ToTokens) -> syn::Result<String> {
    let mut event_type = None;
    for attr in attrs {
        if attr.path().is_ident("integration_event") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") || meta.path.is_ident("type") || meta.path.is_ident("event_type") {
                    let lit: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = lit {
                        event_type = Some(s.value());
                        Ok(())
                    } else {
                        Err(meta.error("expected string literal for integration event type"))
                    }
                } else {
                    Err(meta.error(
                        "unsupported variant-level `integration_event` attribute; expected `rename = \"...\"` or `type = \"...\"`",
                    ))
                }
            })?;
        }
    }
    event_type.ok_or_else(|| {
        Error::new_spanned(
            span_target,
            "missing `#[integration_event(rename = \"com.impierce.unicore....\")]` on variant",
        )
    })
}

fn has_subject_attr(attrs: &[Attribute]) -> syn::Result<bool> {
    let mut is_subject = false;
    for attr in attrs {
        if attr.path().is_ident("integration_event") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("subject") {
                    is_subject = true;
                    Ok(())
                } else {
                    Err(meta.error("unsupported field-level `integration_event` attribute; expected `subject`"))
                }
            })?;
        }
    }
    Ok(is_subject)
}

/// If `ty` is `Option<T>`, returns `(T, false)` (not required); otherwise `(ty, true)` (required).
fn unwrap_option_type(ty: &Type) -> (&Type, bool) {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(GenericArgument::Type(inner_ty)) = args.args.first() {
                        return (inner_ty, false);
                    }
                }
            }
        }
    }
    (ty, true)
}

fn format_type(ty: &Type) -> String {
    ty.to_token_stream()
        .to_string()
        .replace(" < ", "<")
        .replace(" >", ">")
        .replace(" , ", ", ")
}
