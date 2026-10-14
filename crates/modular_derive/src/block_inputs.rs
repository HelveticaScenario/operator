use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitFloat};

use crate::signal_params::{SignalAttr, parse_signal_attr};
use crate::utils::{is_option_poly_signal_type, is_poly_signal_type};

/// `#[block_input(optional)]`: a disconnected input resolves to `None`
/// instead of its `#[signal(default)]`.
fn is_optional(field: &syn::Field) -> syn::Result<bool> {
    let mut optional = false;
    for attr in field
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("block_input"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("optional") {
                optional = true;
                Ok(())
            } else {
                Err(meta.error("expected `optional`"))
            }
        })?;
    }
    Ok(optional)
}

/// An `f32` literal spelling `value` as written, so it rounds once.
fn f32_literal(value: f64) -> TokenStream2 {
    let lit = LitFloat::new(&format!("{:?}f32", value.abs()), Span::call_site());
    if value.is_sign_negative() {
        quote! { -#lit }
    } else {
        quote! { #lit }
    }
}

pub fn impl_block_inputs_macro(ast: &DeriveInput) -> TokenStream {
    match block_inputs(ast) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn block_inputs(ast: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &ast.ident;
    let vis = &ast.vis;
    let inputs_name = format_ident!("{}BlockInputs", name);
    let fields = match &ast.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new(
                    ast.span(),
                    "#[derive(BlockInputs)] only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                ast.span(),
                "#[derive(BlockInputs)] only supports structs",
            ));
        }
    };

    let mut field_decls = Vec::new();
    let mut resolutions = Vec::new();
    let mut scratches = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("named field");
        let scratch = format_ident!("scratch_{}", scratches.len());
        let resolution = if is_poly_signal_type(&field.ty) {
            field_decls.push(quote! { pub #ident: crate::block_input::BlockInput<'a> });
            quote! { self.#ident.block_input(range.clone(), channels, #scratch)? }
        } else if is_option_poly_signal_type(&field.ty) {
            if is_optional(field)? {
                field_decls.push(quote! { pub #ident: Option<crate::block_input::BlockInput<'a>> });
                quote! {
                    match &self.#ident {
                        Some(signal) => Some(signal.block_input(range.clone(), channels, #scratch)?),
                        None => None,
                    }
                }
            } else {
                let attr = match field.attrs.iter().find(|a| a.path().is_ident("signal")) {
                    Some(a) => parse_signal_attr(a)?,
                    None => SignalAttr::default(),
                };
                let default = f32_literal(attr.default_value);
                field_decls.push(quote! { pub #ident: crate::block_input::BlockInput<'a> });
                quote! {
                    crate::block_input::OptionBlockInput::block_input_or(
                        &self.#ident, range.clone(), channels, #default, #scratch,
                    )?
                }
            }
        } else {
            continue;
        };
        resolutions.push(quote! { #ident: #resolution });
        scratches.push(scratch);
    }
    let count = scratches.len();

    Ok(quote! {
        /// Every signal input resolved for one range of slots; see
        /// `crate::block_input`.
        #vis struct #inputs_name<'a> {
            #(#field_decls,)*
        }

        impl #name {
            /// Resolve every signal input for slots `range` across `channels`
            /// channels, in field order. A disconnected input reads as its
            /// `#[signal(default)]`, or as `None` when marked
            /// `#[block_input(optional)]`. `None` when any input declines.
            #[inline(always)]
            #vis fn block_inputs<'a>(
                &'a self,
                range: ::std::ops::Range<usize>,
                channels: usize,
                scratch: &'a mut [crate::block_input::InputScratch; #count],
            ) -> Option<#inputs_name<'a>> {
                let [#(#scratches,)*] = scratch;
                Some(#inputs_name {
                    #(#resolutions,)*
                })
            }
        }
    })
}
