use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;
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
/// - #[output("name", "description", dynamic_range)]
/// - #[output("name", "description", default, range = (-1.0, 1.0))]
fn parse_output_attr(tokens: TokenStream2) -> syn::Result<OutputAttr> {
    use syn::parse::{Parse, ParseStream};
    use syn::Result as SynResult;

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

    // Check that dynamic_range is only used on PolyOutput fields
    for output in &outputs {
        if output.dynamic_range && output.precision != OutputPrecision::PolySignal {
            return syn::Error::new(
                output.field_name.span(),
                "dynamic_range can only be used on PolyOutput fields, not f32",
            )
            .to_compile_error()
            .into();
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

    // Generate get_sample match arms (returns single f32, no PolyOutput copy)
    let sample_match_arms: Vec<_> = outputs
        .iter()
        .map(|o| {
            let output_name = &o.output_name;
            let field_name = &o.field_name;
            match o.precision {
                OutputPrecision::F32 => quote! {
                    #output_name => Some(self.#field_name),
                },
                OutputPrecision::PolySignal => quote! {
                    #output_name => Some(self.#field_name.get_cycling(channel)),
                },
            }
        })
        .collect();

    // Generate virtual range port match arms for dynamic_range outputs
    let virtual_range_arms: Vec<_> = outputs
        .iter()
        .filter(|o| o.dynamic_range && o.precision == OutputPrecision::PolySignal)
        .flat_map(|o| {
            let field_name = &o.field_name;
            let output_name_str = o.output_name.value();
            let range_min_name = format!("{}.rangeMin", output_name_str);
            let range_max_name = format!("{}.rangeMax", output_name_str);
            vec![
                quote! {
                    #range_min_name => {
                        let mut po = crate::poly::PolyOutput::default();
                        po.set_channels(self.#field_name.channels());
                        for ch in 0..self.#field_name.channels() {
                            po.set(ch, self.#field_name.range_min_value(ch));
                        }
                        Some(po)
                    },
                },
                quote! {
                    #range_max_name => {
                        let mut po = crate::poly::PolyOutput::default();
                        po.set_channels(self.#field_name.channels());
                        for ch in 0..self.#field_name.channels() {
                            po.set(ch, self.#field_name.range_max_value(ch));
                        }
                        Some(po)
                    },
                },
            ]
        })
        .collect();

    // Generate virtual range port get_sample arms (single f32, no PolyOutput copy)
    let virtual_range_sample_arms: Vec<_> = outputs
        .iter()
        .filter(|o| o.dynamic_range && o.precision == OutputPrecision::PolySignal)
        .flat_map(|o| {
            let field_name = &o.field_name;
            let output_name_str = o.output_name.value();
            let range_min_name = format!("{}.rangeMin", output_name_str);
            let range_max_name = format!("{}.rangeMax", output_name_str);
            vec![
                quote! {
                    #range_min_name => Some(self.#field_name.range_min_value(channel)),
                },
                quote! {
                    #range_max_name => Some(self.#field_name.range_max_value(channel)),
                },
            ]
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
                    #(#virtual_range_arms)*
                    _ => None,
                }
            }

            fn get_sample(&self, port: &str, channel: usize) -> Option<f32> {
                match port {
                    #(#sample_match_arms)*
                    #(#virtual_range_sample_arms)*
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

    generated.into()
}
