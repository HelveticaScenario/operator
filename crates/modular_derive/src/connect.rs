use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, LitStr, Token, punctuated::Punctuated};

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

/// Per-struct-field codegen: emits `default_connection` defaulting plus
/// `connect`, `collect_cables`, and `inject_index_ptr` calls. Used by
/// struct-with-named-fields (variant fields don't carry `default_connection`).
fn emit_field_named_struct(
    field: &syn::Field,
    default_stmts: &mut TokenStream2,
    connect_stmts: &mut TokenStream2,
    collect_cables_stmts: &mut TokenStream2,
    inject_index_stmts: &mut TokenStream2,
) -> Result<(), syn::Error> {
    let Some(field_ident) = &field.ident else {
        return Ok(());
    };

    for attr in &field.attrs {
        if !attr.path().is_ident("default_connection") {
            continue;
        }
        let dc = parse_default_connection_attr(attr)?;
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
                    quote! { crate::types::WellKnownModule::#module.to_cable(#ch, #port) }
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
                return Err(syn::Error::new(
                    field.span(),
                    "#[default_connection] is not supported on bare (required) signal fields. \
                     Use Option<PolySignal> or Option<MonoSignal> instead.",
                ));
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
            return Err(syn::Error::new(
                field.span(),
                "#[default_connection] is not supported on bare (required) signal fields. \
                 Use Option<Signal> instead.",
            ));
        }
    }

    connect_stmts.extend(quote_spanned! {field.span()=>
        crate::types::Connect::connect(&mut self.#field_ident, patch);
    });
    collect_cables_stmts.extend(quote_spanned! {field.span()=>
        crate::types::Connect::collect_cables(&self.#field_ident, sink);
    });
    inject_index_stmts.extend(quote_spanned! {field.span()=>
        crate::types::Connect::inject_index_ptr(&mut self.#field_ident, ptr);
    });

    Ok(())
}

/// For one enum variant, build (connect-arm, collect-arm, inject-arm) tokens.
/// Bindings are named `field_<ident>` for named fields and `field_<n>` for unnamed.
fn emit_variant_arms(
    enum_name: &Ident,
    variant: &syn::Variant,
) -> Result<(TokenStream2, TokenStream2, TokenStream2), syn::Error> {
    // Reject `#[default_connection]` on enum variants — it's struct-only.
    for field in variant.fields.iter() {
        for attr in &field.attrs {
            if attr.path().is_ident("default_connection") {
                return Err(syn::Error::new(
                    attr.span(),
                    "#[default_connection] is only supported on struct fields, not enum variants",
                ));
            }
        }
    }

    let var_name = &variant.ident;
    match &variant.fields {
        Fields::Unit => {
            let pat = quote! { #enum_name::#var_name };
            Ok((
                quote! { #pat => {} },
                quote! { #pat => {} },
                quote! { #pat => {} },
            ))
        }
        Fields::Named(fields) => {
            let mut binds = Vec::new();
            let mut connect_calls = TokenStream2::new();
            let mut collect_calls = TokenStream2::new();
            let mut inject_calls = TokenStream2::new();
            for f in fields.named.iter() {
                let ident = f.ident.clone().expect("named field");
                binds.push(ident.clone());
                connect_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::connect(#ident, patch);
                });
                collect_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::collect_cables(#ident, sink);
                });
                inject_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::inject_index_ptr(#ident, ptr);
                });
            }
            let pat = quote! { #enum_name::#var_name { #(#binds),* } };
            Ok((
                quote! { #pat => { #connect_calls } },
                quote! { #pat => { #collect_calls } },
                quote! { #pat => { #inject_calls } },
            ))
        }
        Fields::Unnamed(fields) => {
            let mut binds = Vec::new();
            let mut connect_calls = TokenStream2::new();
            let mut collect_calls = TokenStream2::new();
            let mut inject_calls = TokenStream2::new();
            for (i, f) in fields.unnamed.iter().enumerate() {
                let ident = Ident::new(&format!("field_{}", i), Span::call_site());
                binds.push(ident.clone());
                connect_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::connect(#ident, patch);
                });
                collect_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::collect_cables(#ident, sink);
                });
                inject_calls.extend(quote_spanned! {f.span()=>
                    crate::types::Connect::inject_index_ptr(#ident, ptr);
                });
            }
            let pat = quote! { #enum_name::#var_name(#(#binds),*) };
            Ok((
                quote! { #pat => { #connect_calls } },
                quote! { #pat => { #collect_calls } },
                quote! { #pat => { #inject_calls } },
            ))
        }
    }
}

pub fn impl_connect_macro(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;

    let (default_connection_stmts, connect_body, collect_cables_body, inject_index_body) =
        match &ast.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let mut default_stmts = TokenStream2::new();
                let mut connect_stmts = TokenStream2::new();
                let mut collect_cables_stmts = TokenStream2::new();
                let mut inject_index_stmts = TokenStream2::new();

                for field in fields.named.iter() {
                    if let Err(e) = emit_field_named_struct(
                        field,
                        &mut default_stmts,
                        &mut connect_stmts,
                        &mut collect_cables_stmts,
                        &mut inject_index_stmts,
                    ) {
                        return e.to_compile_error().into();
                    }
                }

                (default_stmts, connect_stmts, collect_cables_stmts, inject_index_stmts)
            }
            Fields::Unnamed(_) | Fields::Unit => {
                return syn::Error::new(
                    ast.span(),
                    "#[derive(Connect)] on a struct requires named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        Data::Enum(data) => {
            let mut connect_arms = TokenStream2::new();
            let mut collect_arms = TokenStream2::new();
            let mut inject_arms = TokenStream2::new();
            for variant in data.variants.iter() {
                match emit_variant_arms(name, variant) {
                    Ok((c_arm, cc_arm, ii_arm)) => {
                        connect_arms.extend(c_arm);
                        connect_arms.extend(quote! { , });
                        collect_arms.extend(cc_arm);
                        collect_arms.extend(quote! { , });
                        inject_arms.extend(ii_arm);
                        inject_arms.extend(quote! { , });
                    }
                    Err(e) => return e.to_compile_error().into(),
                }
            }
            (
                TokenStream2::new(),
                quote! { match self { #connect_arms } },
                quote! { match self { #collect_arms } },
                quote! { match self { #inject_arms } },
            )
        }
        Data::Union(_) => {
            return syn::Error::new(ast.span(), "#[derive(Connect)] does not support unions")
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
            fn collect_cables(&self, sink: &mut Vec<String>) {
                #collect_cables_body
            }
            fn inject_index_ptr(&mut self, ptr: *const std::cell::Cell<usize>) {
                #inject_index_body
            }
        }
    };

    generated.into()
}
