use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{punctuated::Punctuated, Attribute, Data, DeriveInput, Fields, LitStr, Token};

use crate::utils::{
    is_mono_signal_type, is_option_mono_signal_type, is_option_poly_signal_type,
    is_option_signal_type, is_poly_signal_type,
};

/// Parsed `#[default_connection(...)]` attribute data
struct DefaultConnectionAttr {
    module: Ident,
    port: String,
    /// For Signal: single channel. For PolySignal: multiple channels.
    channels: Vec<usize>,
}

/// Parse `#[default_connection(id = "...", port = "...", channel = N)]` for Signal
/// or `#[default_connection(id = "...", port = "...", channels = [N, M, ...])]` for PolySignal
fn parse_default_connection_attr(attr: &Attribute) -> syn::Result<DefaultConnectionAttr> {
    let mut module: Option<Ident> = None;
    let mut port: Option<String> = None;
    let mut channels: Vec<usize> = Vec::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("module") {
            let value: Ident = meta.value()?.parse()?;
            module = Some(value);
            Ok(())
        } else if meta.path.is_ident("port") {
            let value: LitStr = meta.value()?.parse()?;
            port = Some(value.value());
            Ok(())
        } else if meta.path.is_ident("channel") {
            let value: syn::LitInt = meta.value()?.parse()?;
            channels = vec![value.base10_parse()?];
            Ok(())
        } else if meta.path.is_ident("channels") {
            meta.value()?;
            let content;
            syn::bracketed!(content in meta.input);
            let parsed: Punctuated<syn::LitInt, Token![,]> =
                Punctuated::parse_terminated(&content)?;
            channels = parsed
                .into_iter()
                .map(|lit| lit.base10_parse())
                .collect::<syn::Result<Vec<usize>>>()?;
            Ok(())
        } else {
            Err(meta.error("expected `module`, `port`, `channel`, or `channels`"))
        }
    })?;

    let module = module
        .ok_or_else(|| syn::Error::new(attr.span(), "missing `module` in default_connection"))?;
    let port =
        port.ok_or_else(|| syn::Error::new(attr.span(), "missing `port` in default_connection"))?;
    if channels.is_empty() {
        return Err(syn::Error::new(
            attr.span(),
            "missing `channel` or `channels` in default_connection",
        ));
    }

    Ok(DefaultConnectionAttr {
        module,
        port,
        channels,
    })
}

pub fn impl_connect_macro(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;

    let (default_connection_stmts, connect_body) = match &ast.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let mut default_stmts = TokenStream2::new();
                let mut connect_stmts = TokenStream2::new();

                for field in fields.named.iter() {
                    let Some(field_ident) = &field.ident else {
                        continue;
                    };

                    // Check for #[default_connection(...)] attribute
                    for attr in &field.attrs {
                        if attr.path().is_ident("default_connection") {
                            match parse_default_connection_attr(attr) {
                                Ok(dc) => {
                                    let module = &dc.module;
                                    let port = &dc.port;
                                    let is_poly = is_poly_signal_type(&field.ty);
                                    let is_mono = is_mono_signal_type(&field.ty);
                                    let is_option_poly = is_option_poly_signal_type(&field.ty);
                                    let is_option_mono = is_option_mono_signal_type(&field.ty);
                                    let is_option_signal = is_option_signal_type(&field.ty);

                                    if is_poly || is_mono || is_option_poly || is_option_mono {
                                        // Generate PolySignal/MonoSignal default
                                        let cable_exprs: Vec<TokenStream2> = dc
                                            .channels
                                            .iter()
                                            .map(|ch| {
                                                quote! {
                                                    crate::types::WellKnownModule::#module.to_cable(#ch, #port)
                                                }
                                            })
                                            .collect();

                                        if is_option_poly {
                                            default_stmts.extend(quote_spanned! {field.span()=>
                                                if self.#field_ident.is_none() {
                                                    self.#field_ident = Some(crate::poly::PolySignal::poly(&[
                                                        #(#cable_exprs),*
                                                    ]));
                                                }
                                            });
                                        } else if is_option_mono {
                                            default_stmts.extend(quote_spanned! {field.span()=>
                                                if self.#field_ident.is_none() {
                                                    self.#field_ident = Some(crate::poly::MonoSignal::from_poly(crate::poly::PolySignal::poly(&[
                                                        #(#cable_exprs),*
                                                    ])));
                                                }
                                            });
                                        } else {
                                            // Bare PolySignal/MonoSignal fields are required — they
                                            // should not have #[default_connection] since the user
                                            // must always provide them.
                                            return syn::Error::new(
                                                field.span(),
                                                "#[default_connection] is not supported on bare (required) signal fields. \
                                                 Use Option<PolySignal> or Option<MonoSignal> instead.",
                                            )
                                            .to_compile_error()
                                            .into();
                                        }
                                    } else if is_option_signal {
                                        // Option<Signal> default (single channel)
                                        let ch = dc.channels.first().copied().unwrap_or(0);
                                        default_stmts.extend(quote_spanned! {field.span()=>
                                            if self.#field_ident.is_none() {
                                                self.#field_ident = Some(crate::types::WellKnownModule::#module.to_cable(#ch, #port));
                                            }
                                        });
                                    } else {
                                        // Bare Signal fields are required — they should not have
                                        // #[default_connection].
                                        return syn::Error::new(
                                            field.span(),
                                            "#[default_connection] is not supported on bare (required) signal fields. \
                                             Use Option<Signal> instead.",
                                        )
                                        .to_compile_error()
                                        .into();
                                    }
                                }
                                Err(e) => return e.to_compile_error().into(),
                            }
                        }
                    }

                    // Always call connect on every field (no-op impls handle primitives)
                    connect_stmts.extend(quote_spanned! {field.span()=>
                        crate::types::Connect::connect(&mut self.#field_ident, patch);
                    });
                }

                (default_stmts, connect_stmts)
            }
            Fields::Unnamed(_) | Fields::Unit => {
                return syn::Error::new(
                    ast.span(),
                    "#[derive(Connect)] only supports structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new(ast.span(), "#[derive(Connect)] only supports structs")
                .to_compile_error()
                .into();
        }
    };

    let generated = quote! {
        impl crate::types::Connect for #name {
            fn connect(&mut self, patch: &crate::Patch) {
                // Apply default connections for disconnected inputs
                #default_connection_stmts
                // Connect all fields
                #connect_body
            }
        }
    };

    generated.into()
}
