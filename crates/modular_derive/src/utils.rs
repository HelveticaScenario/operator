use proc_macro2::TokenStream as TokenStream2;
use syn::{Attribute, Type};

/// Extract `///` doc comments from a list of attributes.
/// In Rust, `/// text` desugars to `#[doc = "text"]`.
pub fn extract_doc_comments(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(nv) = &attr.meta {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    {
                        return Some(s.value());
                    }
                }
            }
            None
        })
        .collect();
    if docs.is_empty() {
        None
    } else {
        // Each doc comment line has a leading space (" text") — trim it.
        // Join with newlines to preserve paragraph structure.
        Some(
            docs.iter()
                .map(|line| {
                    if let Some(stripped) = line.strip_prefix(' ') {
                        stripped.to_string()
                    } else {
                        line.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub fn unwrap_attr(attrs: &[Attribute], ident: &str) -> Option<TokenStream2> {
    attrs
        .iter()
        .find(|attr| attr.path().is_ident(ident))
        .and_then(|attr| {
            if let syn::Meta::List(list) = &attr.meta {
                Some(list.tokens.clone())
            } else {
                None
            }
        })
}

pub fn is_type(ty: &Type, is_type_fn: fn(&Type) -> bool) -> bool {
    if let Type::Path(type_path) = ty {
        let segments = &type_path.path.segments;
        if let Some(last) = segments.last() {
            if last.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                        return is_type_fn(inner_ty);
                    }
                }
            }
        }
    }
    false
}

/// Check if a type is exactly PolySignal (for default_connection code generation)
pub fn is_poly_signal_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "PolySignal")
            .unwrap_or(false),
        _ => false,
    }
}

/// Check if a type is Option<PolySignal>
pub fn is_option_poly_signal_type(ty: &Type) -> bool {
    is_type(ty, is_poly_signal_type)
}

/// Check if a type is exactly MonoSignal (for default_connection code generation)
pub fn is_mono_signal_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "MonoSignal")
            .unwrap_or(false),
        _ => false,
    }
}

/// Check if a type is Option<MonoSignal>
pub fn is_option_mono_signal_type(ty: &Type) -> bool {
    is_type(ty, is_mono_signal_type)
}

/// Check if a type is Option<Signal>
pub fn is_option_signal_type(ty: &Type) -> bool {
    is_type(ty, is_signal_type)
}

/// Check if a type is exactly Signal
pub fn is_signal_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "Signal")
            .unwrap_or(false),
        _ => false,
    }
}
