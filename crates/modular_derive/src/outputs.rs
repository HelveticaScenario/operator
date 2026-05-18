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
    /// True when the `#[output(..., dynamic_range)]` flag is set. Triggers
    /// generation of virtual `<port>.rangeMin` / `<port>.rangeMax` ports
    /// backed by per-slot `BlockPort`s the inner module writes via
    /// `PolyOutput::set_range`.
    dynamic_range: bool,
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
    dynamic_range: bool,
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
        dynamic_range: bool,
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

            // Parse optional attributes (default, range, dynamic_range)
            let mut is_default = false;
            let mut range: Option<(f64, f64)> = None;
            let mut dynamic_range = false;
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
                    } else if ident == "dynamic_range" {
                        dynamic_range = true;
                    } else {
                        return Err(syn::Error::new(
                            ident.span(),
                            format!(
                                "Unknown output attribute '{}'. Expected 'default', 'range', or 'dynamic_range'",
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
                dynamic_range,
            })
        }
    }

    let parsed = syn::parse2::<OutputAttrParser>(tokens)?;

    if parsed.dynamic_range && parsed.range.is_none() {
        return Err(syn::Error::new(
            parsed.name.span(),
            "'dynamic_range' requires a fallback static 'range = (..)'. The static range is used \
             when no per-channel runtime range is recorded.",
        ));
    }

    Ok(OutputAttr {
        name: parsed.name,
        description: parsed.description,
        is_default: parsed.is_default,
        range: parsed.range,
        dynamic_range: parsed.dynamic_range,
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

                    if output_attr.dynamic_range && precision != OutputPrecision::PolySignal {
                        return syn::Error::new(
                            f.ty.span(),
                            "'dynamic_range' is only supported on PolyOutput fields. Per-channel \
                             runtime ranges require PolyOutput storage.",
                        )
                        .to_compile_error()
                        .into();
                    }

                    out.push(OutputField {
                        field_name,
                        output_name,
                        precision,
                        description,
                        is_default: output_attr.is_default,
                        range: output_attr.range,
                        dynamic_range: output_attr.dynamic_range,
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
            let dynamic_range = o.dynamic_range;
            quote! {
                crate::types::OutputSchema {
                    name: #output_name.to_string(),
                    description: #description,
                    polyphonic: #is_polyphonic,
                    default: #is_default,
                    min_value: #min_value,
                    max_value: #max_value,
                    dynamic_range: #dynamic_range,
                }
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

    // Walk every output and emit (in declaration order):
    //   1. The data BlockPort field, port_index entry, and get_at arm.
    //   2. If the output has a `range`, two virtual ports
    //      `<name>.rangeMin` / `<name>.rangeMax` for `.range()`-style chains.
    //      Static ranges return their constants directly from `get_at`;
    //      dynamic ranges store extra BlockPorts so the inner module can write
    //      per-channel per-slot bounds via `PolyOutput::set_range`.
    //   3. A `get_range(port, ch, index)` match arm so consumers can read the
    //      runtime range without composing virtual port names.
    let mut block_fields: Vec<TokenStream2> = Vec::new();
    let mut block_new_inits: Vec<TokenStream2> = Vec::new();
    let mut port_index_arms: Vec<TokenStream2> = Vec::new();
    let mut get_at_arms: Vec<TokenStream2> = Vec::new();
    let mut get_range_arms: Vec<TokenStream2> = Vec::new();
    let mut copy_inner_stmts: Vec<TokenStream2> = Vec::new();
    let mut next_idx: usize = 0;

    for o in &outputs {
        let field_name = &o.field_name;
        let output_name = &o.output_name;

        // Data port: regular BlockPort field + port_index + get_at.
        block_fields.push(quote! { pub #field_name: crate::block_port::BlockPort });
        block_new_inits.push(quote! {
            #field_name: crate::block_port::BlockPort::new(block_size)
        });
        port_index_arms.push(quote! { #output_name => Some(#next_idx), });
        get_at_arms.push(quote! { #next_idx => self.#field_name.get(index, ch), });
        next_idx += 1;

        // Copy data into the block buffer at `slot`.
        match o.precision {
            OutputPrecision::F32 => copy_inner_stmts.push(quote! {
                // Broadcast the mono f32 value to every channel slot.
                let v = inner.#field_name;
                for ch in 0..crate::poly::PORT_MAX_CHANNELS {
                    self.#field_name.data[slot][ch] = v;
                }
            }),
            OutputPrecision::PolySignal => copy_inner_stmts.push(quote! {
                {
                    // `get_cycling` mirrors the old `PolyOutput::get_cycling`
                    // semantics: a mono producer broadcasts its single value to
                    // all consumer channels rather than producing silence on
                    // channels >= producer channel count.
                    let poly = &inner.#field_name;
                    for ch in 0..crate::poly::PORT_MAX_CHANNELS {
                        self.#field_name.data[slot][ch] = poly.get_cycling(ch);
                    }
                }
            }),
        }

        // Virtual range ports for any output that declared a `range`.
        if let Some((min, max)) = o.range {
            let min_lit = min as f32;
            let max_lit = max as f32;
            let base_name = output_name.value();
            let range_min_name = LitStr::new(
                &format!("{}.rangeMin", base_name),
                output_name.span(),
            );
            let range_max_name = LitStr::new(
                &format!("{}.rangeMax", base_name),
                output_name.span(),
            );

            if o.dynamic_range {
                // Extra BlockPorts let the inner module write per-channel
                // per-slot bounds; only allocated for `dynamic_range` outputs.
                let field_min = format_ident!("{}_range_min", field_name);
                let field_max = format_ident!("{}_range_max", field_name);
                block_fields.push(quote! { pub #field_min: crate::block_port::BlockPort });
                block_fields.push(quote! { pub #field_max: crate::block_port::BlockPort });
                block_new_inits.push(quote! {
                    #field_min: crate::block_port::BlockPort::new(block_size)
                });
                block_new_inits.push(quote! {
                    #field_max: crate::block_port::BlockPort::new(block_size)
                });

                let min_idx = next_idx;
                port_index_arms.push(quote! { #range_min_name => Some(#min_idx), });
                get_at_arms.push(quote! { #min_idx => self.#field_min.get(index, ch), });
                next_idx += 1;
                let max_idx = next_idx;
                port_index_arms.push(quote! { #range_max_name => Some(#max_idx), });
                get_at_arms.push(quote! { #max_idx => self.#field_max.get(index, ch), });
                next_idx += 1;

                // Mirror per-channel range from the inner PolyOutput into the
                // virtual BlockPorts each slot. Fall back to the static range
                // when the inner module hasn't set bounds yet (NaN).
                copy_inner_stmts.push(quote! {
                    {
                        let poly = &inner.#field_name;
                        for ch in 0..crate::poly::PORT_MAX_CHANNELS {
                            let rmin = poly.get_range_min(ch);
                            let rmax = poly.get_range_max(ch);
                            self.#field_min.data[slot][ch] =
                                if rmin.is_nan() { #min_lit } else { rmin };
                            self.#field_max.data[slot][ch] =
                                if rmax.is_nan() { #max_lit } else { rmax };
                        }
                    }
                });

                // Runtime range query reads slot `index` so consumers stay
                // aligned with the producer's per-block index.
                get_range_arms.push(quote! {
                    #output_name => Some((
                        self.#field_min.get(index, ch),
                        self.#field_max.get(index, ch),
                    )),
                });
            } else {
                // Static range — no extra storage. `get_at` returns the
                // compile-time constants directly.
                let min_idx = next_idx;
                port_index_arms.push(quote! { #range_min_name => Some(#min_idx), });
                get_at_arms.push(quote! { #min_idx => #min_lit, });
                next_idx += 1;
                let max_idx = next_idx;
                port_index_arms.push(quote! { #range_max_name => Some(#max_idx), });
                get_at_arms.push(quote! { #max_idx => #max_lit, });
                next_idx += 1;

                get_range_arms.push(quote! {
                    #output_name => Some((#min_lit, #max_lit)),
                });
            }
        }
    }

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

            /// Copy the inner module's scalar outputs (and per-channel dynamic
            /// range bounds, if any) into this block buffer at `slot`.
            pub fn copy_from_inner(&mut self, inner: &#name, slot: usize) {
                #(#copy_inner_stmts)*
            }

            /// Read the per-channel range bounds of the data port `port` at
            /// sample slot `index`. Returns the compile-time constants for
            /// static-range outputs, the per-slot runtime values for
            /// dynamic-range outputs, and `None` for ports without range
            /// metadata.
            ///
            /// Zero-allocation. The match lowers to one branch per output.
            #[inline]
            pub fn get_range(&self, port: &str, ch: usize, index: usize) -> Option<(f32, f32)> {
                match port {
                    #(#get_range_arms)*
                    _ => None,
                }
            }

        }
    };

    let mut all_generated = quote!(#generated);
    all_generated.extend(block_generated);
    all_generated.into()
}
