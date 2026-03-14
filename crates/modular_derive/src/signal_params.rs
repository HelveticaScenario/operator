use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, Token};

use crate::utils::{
    extract_doc_comments, is_mono_signal_type, is_option_mono_signal_type,
    is_option_poly_signal_type, is_poly_signal_type,
};

/// Parsed `#[signal(...)]` attribute data for signal param metadata.
pub struct SignalAttr {
    pub signal_type: String,
    pub default_value: f64,
    pub min_value: f64,
    pub max_value: f64,
}

impl Default for SignalAttr {
    fn default() -> Self {
        Self {
            signal_type: "control".to_string(),
            default_value: 0.0,
            min_value: -5.0,
            max_value: 5.0,
        }
    }
}

/// Parse a numeric literal (float or int) from a parse stream, handling an
/// optional leading `-` sign.  Returns the value as `f64`.
fn parse_number_literal(input: syn::parse::ParseStream) -> syn::Result<f64> {
    let negative = input.peek(Token![-]);
    if negative {
        input.parse::<Token![-]>()?;
    }
    let lit: syn::Lit = input.parse()?;
    let value = match &lit {
        syn::Lit::Float(f) => f.base10_parse::<f64>()?,
        syn::Lit::Int(i) => i.base10_parse::<i64>()? as f64,
        _ => {
            return Err(syn::Error::new(lit.span(), "expected a number literal"));
        }
    };
    Ok(if negative { -value } else { value })
}

pub fn parse_signal_attr(attr: &Attribute) -> syn::Result<SignalAttr> {
    let mut result = SignalAttr::default();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("type") {
            let value: Ident = meta.value()?.parse()?;
            let type_str = value.to_string();
            match type_str.as_str() {
                "pitch" | "gate" | "trig" | "control" => {
                    result.signal_type = type_str;
                }
                other => {
                    return Err(meta.error(format!(
                        "Unknown signal type '{}'. Expected: pitch, gate, trig, control",
                        other
                    )));
                }
            }
            Ok(())
        } else if meta.path.is_ident("default") {
            let stream = meta.value()?;
            result.default_value = parse_number_literal(stream)?;
            Ok(())
        } else if meta.path.is_ident("range") {
            meta.value()?;
            let content;
            syn::parenthesized!(content in meta.input);
            let min_val = parse_number_literal(&content)?;
            content.parse::<Token![,]>()?;
            let max_val = parse_number_literal(&content)?;
            result.min_value = min_val;
            result.max_value = max_val;
            Ok(())
        } else {
            Err(meta.error("expected `type`, `default`, or `range`"))
        }
    })?;

    Ok(result)
}

pub fn impl_signal_params_macro(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;

    let schema_exprs: Vec<TokenStream2> = match &ast.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let mut exprs = Vec::new();
                for field in fields.named.iter() {
                    let field_ident = match &field.ident {
                        Some(id) => id,
                        None => continue,
                    };
                    let is_signal = is_poly_signal_type(&field.ty)
                        || is_mono_signal_type(&field.ty)
                        || is_option_poly_signal_type(&field.ty)
                        || is_option_mono_signal_type(&field.ty);
                    if !is_signal {
                        continue;
                    }

                    // Parse #[signal(...)] attribute if present, otherwise use defaults
                    let signal_attr = field.attrs.iter().find(|a| a.path().is_ident("signal"));

                    let attr = match signal_attr {
                        Some(a) => match parse_signal_attr(a) {
                            Ok(parsed) => parsed,
                            Err(e) => return e.to_compile_error().into(),
                        },
                        None => SignalAttr::default(),
                    };

                    // Extract doc comments for description
                    let description = extract_doc_comments(&field.attrs).unwrap_or_default();

                    let field_name = field_ident.to_string().to_case(Case::Camel);
                    let signal_type = &attr.signal_type;
                    let default_value = attr.default_value;
                    let min_value = attr.min_value;
                    let max_value = attr.max_value;

                    exprs.push(quote! {
                        crate::types::SignalParamSchema {
                            name: #field_name.to_string(),
                            description: #description.to_string(),
                            signal_type: #signal_type.to_string(),
                            default_value: #default_value,
                            min_value: #min_value,
                            max_value: #max_value,
                        }
                    });
                }
                exprs
            }
            Fields::Unnamed(_) | Fields::Unit => {
                return syn::Error::new(
                    ast.span(),
                    "#[derive(SignalParams)] only supports structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new(ast.span(), "#[derive(SignalParams)] only supports structs")
                .to_compile_error()
                .into();
        }
    };

    let generated = quote! {
        impl crate::types::SignalParamMeta for #name {
            fn signal_param_schemas() -> Vec<crate::types::SignalParamSchema> {
                vec![#(#schema_exprs,)*]
            }
        }
    };

    generated.into()
}
