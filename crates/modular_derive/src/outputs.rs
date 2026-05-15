use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitStr, Token, Type};

use crate::utils::unwrap_attr;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reserved_output_names.rs"
));

/// Parsed output attribute data
struct OutputAttr {
    name: LitStr,
    description: Option<LitStr>,
    is_default: bool,
    range: Option<(f64, f64)>,
}

/// Precision type for output fields
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputPrecision {
    F32,
    PolySignal,
}

/// Parsed output field data
struct OutputField {
    field_name: Ident,
    output_name: LitStr,
    precision: OutputPrecision,
    description: TokenStream2,
    is_default: bool,
    range: Option<(f64, f64)>,
}

/// Parse output attribute tokens into OutputAttr
/// Supports:
/// - #[output("name", "description")]
/// - #[output("name", "description", default)]
/// - #[output("name", "description", range = (-1.0, 1.0))]
/// - #[output("name", "description", default, range = (-1.0, 1.0))]
fn parse_output_attr(tokens: TokenStream2) -> syn::Result<OutputAttr> {
    use syn::Result as SynResult;
    use syn::parse::{Parse, ParseStream};

    struct OutputAttrParser {
        name: LitStr,
        description: Option<LitStr>,
        is_default: bool,
        range: Option<(f64, f64)>,
    }

    impl Parse for OutputAttrParser {
        fn parse(input: ParseStream) -> SynResult<Self> {
            // Parse first string literal (name)
            let name: LitStr = input.parse()?;
            let name_value = name.value();

            // Build expanded reserved names including snake_case variants
            let reserved_with_snake: Vec<String> = RESERVED_OUTPUT_NAMES
                .iter()
                .flat_map(|&name| {
                    let snake = name.to_case(Case::Snake);
                    if snake == name {
                        vec![name.to_string()]
                    } else {
                        vec![name.to_string(), snake]
                    }
                })
                .collect();

            if reserved_with_snake.iter().any(|r| r == &name_value) {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "Output name '{}' is reserved. Reserved names are: {:?}",
                        name_value, reserved_with_snake
                    ),
                ));
            }

            input.parse::<Token![,]>()?;

            // Parse description string
            let description: LitStr = input.parse()?;

            // Parse optional attributes (default, range)
            let mut is_default = false;
            let mut range: Option<(f64, f64)> = None;
            while input.peek(Token![,]) {
                input.parse::<Token![,]>()?;

                if input.is_empty() {
                    break;
                }

                // Check for `default` keyword
                if input.peek(syn::Ident) {
                    let ident: Ident = input.parse()?;
                    if ident == "default" {
                        is_default = true;
                    } else if ident == "range" {
                        input.parse::<Token![=]>()?;
                        let content;
                        syn::parenthesized!(content in input);
                        let min: syn::LitFloat = content.parse()?;
                        content.parse::<Token![,]>()?;
                        let max: syn::LitFloat = content.parse()?;
                        range = Some((min.base10_parse()?, max.base10_parse()?));
                    } else {
                        return Err(syn::Error::new(
                            ident.span(),
                            format!(
                                "Unknown output attribute '{}'. Expected 'default' or 'range'",
                                ident
                            ),
                        ));
                    }
                }
            }

            Ok(OutputAttrParser {
                name,
                description: Some(description),
                is_default,
                range,
            })
        }
    }

    let parsed = syn::parse2::<OutputAttrParser>(tokens)?;

    Ok(OutputAttr {
        name: parsed.name,
        description: parsed.description,
        is_default: parsed.is_default,
        range: parsed.range,
    })
}

pub fn impl_outputs_macro(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;

    let outputs: Vec<OutputField> = match ast.data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let mut out = Vec::new();
                for f in fields.named.iter() {
                    let field_name = f
                        .ident
                        .clone()
                        .expect("Expected named field in Outputs struct");

                    let output_attr_tokens = match unwrap_attr(&f.attrs, "output") {
                        Some(t) => t,
                        None => {
                            return syn::Error::new(
                                f.span(),
                                "Every field in an Outputs struct must be annotated with #[output(...)]",
                            )
                            .to_compile_error()
                            .into();
                        }
                    };

                    // Detect field precision (f32 or PolySignal)
                    let precision = match &f.ty {
                        Type::Path(tp) => {
                            let type_name =
                                tp.path.segments.last().map(|seg| seg.ident.to_string());
                            match type_name.as_deref() {
                                Some("f32") => OutputPrecision::F32,
                                Some("PolyOutput") => OutputPrecision::PolySignal,
                                _ => {
                                    return syn::Error::new(
                                        f.ty.span(),
                                        "Output fields must have type f32 or PolyOutput",
                                    )
                                    .to_compile_error()
                                    .into();
                                }
                            }
                        }
                        _ => {
                            return syn::Error::new(
                                f.ty.span(),
                                "Output fields must have type f32, or PolyOutput",
                            )
                            .to_compile_error()
                            .into();
                        }
                    };

                    let output_attr = match parse_output_attr(output_attr_tokens) {
                        Ok(v) => v,
                        Err(e) => return e.to_compile_error().into(),
                    };
                    let output_name = output_attr.name;
                    let description = output_attr
                        .description
                        .as_ref()
                        .map(|d| quote!(#d.to_string()))
                        .unwrap_or(quote!("".to_string()));

                    out.push(OutputField {
                        field_name,
                        output_name,
                        precision,
                        description,
                        is_default: output_attr.is_default,
                        range: output_attr.range,
                    });
                }
                out
            }
            Fields::Unnamed(_) | Fields::Unit => {
                return syn::Error::new(
                    Span::call_site(),
                    "Outputs can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new(Span::call_site(), "Outputs can only be derived for structs")
                .to_compile_error()
                .into();
        }
    };

    // Check for duplicate output names
    {
        let mut seen: std::collections::HashMap<String, &LitStr> = std::collections::HashMap::new();
        for output in &outputs {
            let name_value = output.output_name.value();
            if let Some(first) = seen.get(&name_value) {
                let mut err = syn::Error::new(
                    output.output_name.span(),
                    format!("Duplicate output name '{}'", name_value),
                );
                err.combine(syn::Error::new(
                    first.span(),
                    format!("'{}' first defined here", name_value),
                ));
                return err.to_compile_error().into();
            }
            seen.insert(name_value, &output.output_name);
        }
    }

    // Validate that exactly one output is marked as default
    let default_outputs: Vec<_> = outputs.iter().filter(|o| o.is_default).collect();
    if default_outputs.is_empty() {
        return syn::Error::new(
            Span::call_site(),
            format!(
                "Outputs struct '{}' must have exactly one output marked as `default`. \
                 Add `default` to one of the #[output(...)] attributes.",
                name
            ),
        )
        .to_compile_error()
        .into();
    }
    if default_outputs.len() > 1 {
        let names: Vec<_> = default_outputs
            .iter()
            .map(|o| o.output_name.value())
            .collect();
        return syn::Error::new(
            Span::call_site(),
            format!(
                "Outputs struct '{}' has {} outputs marked as `default` ({:?}), but only one is allowed.",
                name,
                default_outputs.len(),
                names
            ),
        )
        .to_compile_error()
        .into();
    }

    // Generate default value expressions for each field type
    let field_defaults: Vec<_> = outputs
        .iter()
        .map(|o| {
            let field_name = &o.field_name;
            match o.precision {
                OutputPrecision::F32 => quote! { #field_name: 0.0 },
                OutputPrecision::PolySignal => {
                    quote! { #field_name: crate::poly::PolyOutput::default() }
                }
            }
        })
        .collect();

    // Generate get_poly_sample match arms (returns PolyOutput)
    let poly_sample_match_arms: Vec<_> = outputs
        .iter()
        .map(|o| {
            let output_name = &o.output_name;
            let field_name = &o.field_name;
            match o.precision {
                OutputPrecision::F32 => quote! {
                    #output_name => Some(crate::poly::PolyOutput::mono(self.#field_name)),
                },
                OutputPrecision::PolySignal => quote! {
                    #output_name => Some(self.#field_name),
                },
            }
        })
        .collect();

    let schema_exprs: Vec<_> = outputs
        .iter()
        .map(|o| {
            let output_name = &o.output_name;
            let description = &o.description;
            let is_polyphonic = o.precision == OutputPrecision::PolySignal;
            let is_default = o.is_default;
            let min_value = match o.range {
                Some((min, _)) => quote! { Some(#min) },
                None => quote! { None },
            };
            let max_value = match o.range {
                Some((_, max)) => quote! { Some(#max) },
                None => quote! { None },
            };
            quote! {
                crate::types::OutputSchema {
                    name: #output_name.to_string(),
                    description: #description,
                    polyphonic: #is_polyphonic,
                    default: #is_default,
                    min_value: #min_value,
                    max_value: #max_value,
                }
            }
        })
        .collect();

    let copy_stmts: Vec<_> = outputs
        .iter()
        .map(|o| {
            let field_name = &o.field_name;
            quote! {
                self.#field_name = other.#field_name;
            }
        })
        .collect();

    let set_channels_stmts: Vec<_> = outputs
        .iter()
        .filter(|o| o.precision == OutputPrecision::PolySignal)
        .map(|o| {
            let field_name = &o.field_name;
            quote! {
                self.#field_name.set_channels(channels);
            }
        })
        .collect();

    let generated = quote! {
        impl Default for #name {
            fn default() -> Self {
                Self {
                    #(#field_defaults,)*
                }
            }
        }

        impl crate::types::OutputStruct for #name {
            fn copy_from(&mut self, other: &Self) {
                #(#copy_stmts)*
            }

            fn get_poly_sample(&self, port: &str) -> Option<crate::poly::PolyOutput> {
                match port {
                    #(#poly_sample_match_arms)*
                    _ => None,
                }
            }

            fn set_all_channels(&mut self, channels: usize) {
                #(#set_channels_stmts)*
            }

            fn schemas() -> Vec<crate::types::OutputSchema> {
                vec![
                    #(#schema_exprs,)*
                ]
            }
        }
    };

    // -----------------------------------------------------------------------
    // Generate {Name}BlockOutputs
    // -----------------------------------------------------------------------
    let name_str = name.to_string();
    let block_outputs_name = if name_str.ends_with("Outputs") {
        format_ident!("{}BlockOutputs", &name_str[..name_str.len() - 7])
    } else {
        format_ident!("{}BlockOutputs", name_str)
    };

    let block_fields: Vec<_> = outputs
        .iter()
        .map(|o| {
            let field_name = &o.field_name;
            quote! { pub #field_name: crate::block_port::BlockPort }
        })
        .collect();

    let block_new_inits: Vec<_> = outputs
        .iter()
        .map(|o| {
            let field_name = &o.field_name;
            quote! { #field_name: crate::block_port::BlockPort::new(block_size) }
        })
        .collect();

    // String → index map (cold path: called once at connect time).
    let port_index_arms: Vec<_> = outputs
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let output_name = &o.output_name;
            quote! { #output_name => Some(#i), }
        })
        .collect();

    // Index → field dispatch (hot path: called per-sample on audio thread).
    // `match` on `usize` lowers to a jump table or dense branch tree, much
    // tighter than per-string compare on `&str`.
    let get_at_arms: Vec<_> = outputs
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let field_name = &o.field_name;
            quote! { #i => self.#field_name.get(index, ch), }
        })
        .collect();

    let copy_inner_stmts: Vec<_> = outputs
        .iter()
        .map(|o| {
            let field_name = &o.field_name;
            match o.precision {
                OutputPrecision::F32 => quote! {
                    self.#field_name.data[slot][0] = inner.#field_name;
                },
                OutputPrecision::PolySignal => quote! {
                    {
                        // `get_cycling` mirrors the old `PolyOutput::get_cycling`
                        // semantics that `get_poly_sample(...).get_cycling(ch)`
                        // used to provide: a mono producer broadcasts its single
                        // value to all consumer channels rather than producing
                        // silence on channels >= producer channel count.
                        let poly = &inner.#field_name;
                        for ch in 0..crate::poly::PORT_MAX_CHANNELS {
                            self.#field_name.data[slot][ch] = poly.get_cycling(ch);
                        }
                    }
                },
            }
        })
        .collect();

    let block_generated = quote! {
        /// Generated block-output buffer for #name.
        /// One `BlockPort` per output port; indexed `data[sample_index][channel]`.
        pub struct #block_outputs_name {
            #(#block_fields,)*
        }

        impl #block_outputs_name {
            /// Allocate all ports for the given block size. Call only on the main thread.
            pub fn new(block_size: usize) -> Self {
                Self {
                    #(#block_new_inits,)*
                }
            }

            /// Resolve a port name to its index. Cold path — call once at
            /// connect time and cache the result on the cable.
            ///
            /// Returns `None` for unknown ports.
            #[inline]
            pub fn port_index(name: &str) -> Option<usize> {
                match name {
                    #(#port_index_arms)*
                    _ => None,
                }
            }

            /// Hot-path read by port index. `port_idx` must come from
            /// [`port_index`] (a stale or out-of-range index returns 0.0).
            ///
            /// Lowers to a `match`-on-`usize`, which the optimiser turns into
            /// a jump table or dense branch tree — typically one branch per
            /// read regardless of port count.
            ///
            /// There is intentionally **no** `&str` overload: callers must
            /// resolve the port name once via [`port_index`] and cache the
            /// result. String lookup on the audio thread is exactly the cost
            /// this design avoids.
            #[inline]
            pub fn get_at(&self, port_idx: usize, ch: usize, index: usize) -> f32 {
                match port_idx {
                    #(#get_at_arms)*
                    _ => 0.0,
                }
            }

            /// Copy the inner module's scalar outputs into this block buffer at `slot`.
            pub fn copy_from_inner(&mut self, inner: &#name, slot: usize) {
                #(#copy_inner_stmts)*
            }

            /// Called once per CPAL callback to advance any stateful per-block fields.
            /// Default is a no-op; specialised impls (e.g. `BufferWrite`) override this.
            pub fn tick_buffers(&mut self, _block_size: usize) {}
        }
    };

    let mut all_generated = quote!(#generated);
    all_generated.extend(block_generated);
    all_generated.into()
}
