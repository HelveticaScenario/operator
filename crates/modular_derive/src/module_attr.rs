use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, LitStr, Token, Type, punctuated::Punctuated};

use crate::utils::{extract_doc_comments, unwrap_attr};

/// Parsed module attribute data
struct ModuleAttr {
    name: LitStr,
    channels: Option<u8>,
    channels_param: Option<LitStr>,
    channels_param_default: Option<u8>,
    /// Custom function to derive channel count from params struct.
    /// The function must have signature: fn(&ParamsStruct) -> Option<usize>
    channels_derive: Option<syn::Path>,
}

struct ArgAttr {
    name: Ident,
}

impl syn::parse::Parse for ArgAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        Ok(ArgAttr { name })
    }
}

// ---------------------------------------------------------------------------
// Attribute-macro argument parser
// ---------------------------------------------------------------------------

/// All configuration parsed from `#[module(...)]` attribute arguments.
///
/// Idiomatic key=value syntax:
/// ```text
/// #[module(
///     name = "$sine",
///     channels = 2,
///     args(freq, engine),
///     stateful,
///     patch_update,
///     has_init,
/// )]
/// ```
pub struct ModuleAttrArgs {
    module: ModuleAttr,
    args: Vec<ArgAttr>,
    /// Whether the `args(...)` keyword was present at all (even if empty).
    has_args: bool,
    stateful: bool,
    patch_update: bool,
    has_init: bool,
    has_prepare_resources: bool,
    clock_sync: bool,
}

impl syn::parse::Parse for ModuleAttrArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name: Option<LitStr> = None;
        let mut channels: Option<u8> = None;
        let mut channels_param: Option<LitStr> = None;
        let mut channels_param_default: Option<u8> = None;
        let mut channels_derive: Option<syn::Path> = None;
        let mut args: Vec<ArgAttr> = Vec::new();
        let mut has_args = false;
        let mut stateful = false;
        let mut patch_update = false;
        let mut has_init = false;
        let mut has_prepare_resources = false;
        let mut clock_sync = false;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "name" => {
                    input.parse::<Token![=]>()?;
                    name = Some(input.parse()?);
                }
                "channels" => {
                    input.parse::<Token![=]>()?;
                    let lit: syn::LitInt = input.parse()?;
                    let n: u8 = lit.base10_parse()?;
                    // Must match modular_core::poly::PORT_MAX_CHANNELS
                    const MAX: u8 = 16;
                    if n < 1 || n > MAX {
                        return Err(syn::Error::new(
                            lit.span(),
                            format!("channels must be between 1 and {MAX}, got {n}"),
                        ));
                    }
                    channels = Some(n);
                }
                "channels_param" => {
                    input.parse::<Token![=]>()?;
                    channels_param = Some(input.parse()?);
                }
                "channels_param_default" => {
                    input.parse::<Token![=]>()?;
                    let lit: syn::LitInt = input.parse()?;
                    channels_param_default = Some(lit.base10_parse()?);
                }
                "channels_derive" => {
                    input.parse::<Token![=]>()?;
                    channels_derive = Some(input.parse()?);
                }
                "args" => {
                    has_args = true;
                    let content;
                    syn::parenthesized!(content in input);
                    let parsed: Punctuated<ArgAttr, Token![,]> =
                        Punctuated::parse_terminated(&content)?;
                    args = parsed.into_iter().collect();
                }
                "stateful" => {
                    stateful = true;
                }
                "patch_update" => {
                    patch_update = true;
                }
                "has_init" => {
                    has_init = true;
                }
                "has_prepare_resources" => {
                    has_prepare_resources = true;
                }
                "clock_sync" => {
                    clock_sync = true;
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "Unknown module attribute '{other}'. Expected one of: \
                             name, channels, channels_param, \
                             channels_param_default, channels_derive, args, \
                             stateful, patch_update, has_init, has_prepare_resources, clock_sync"
                        ),
                    ));
                }
            }

            // Consume trailing comma if present
            let _ = input.parse::<Token![,]>();
        }

        let name = name.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "missing `name` in #[module(...)]",
            )
        })?;

        Ok(ModuleAttrArgs {
            module: ModuleAttr {
                name,
                channels,
                channels_param,
                channels_param_default,
                channels_derive,
            },
            args,
            has_args,
            stateful,
            patch_update,
            has_init,
            has_prepare_resources,
            clock_sync,
        })
    }
}

/// Attribute-style proc macro for declaring audio modules.
///
/// # Syntax
///
/// ```rust,ignore
/// #[module(
///     name = "$sine",
///     description = "A sine wave oscillator",
///     // Channel count configuration (at most one):
///     // channels = 2,                         // hardcoded
///     // channels_param = "channels",           // read from param field
///     // channels_param_default = 1,            // default when param absent
///     // channels_derive = my_derive_fn,        // custom function
///     //
///     // Positional DSL arguments (optional):
///     // args(freq, engine),
///     //
///     // Flags (optional):
///     // stateful,      // implements StatefulModule
///     // patch_update,  // implements PatchUpdateHandler
///     // has_init,      // has fn init(&mut self, sample_rate: f32)
/// )]
/// pub struct MyModule { ... }
/// ```
///
/// The struct **must** have a field named `outputs` whose type derives `Outputs`,
/// and a field named `params` whose type derives `Deserialize`, `JsonSchema`,
/// `Connect`, and `ChannelCount`.
///
/// Module structs do **not** need to derive `Default`. The proc macro generates
/// per-field initialization in the constructor: `params` comes from deserialization,
/// `_channel_count` from the computed channel count, and all other fields use
/// `Default::default()` on their individual types.
pub fn module_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = syn::parse_macro_input!(attr as ModuleAttrArgs);
    let mut ast: DeriveInput = syn::parse_macro_input!(item as DeriveInput);

    // Strip any leftover helper attributes that we've absorbed (safety net for migration)
    ast.attrs.retain(|a| {
        !a.path().is_ident("module")
            && !a.path().is_ident("args")
            && !a.path().is_ident("stateful")
            && !a.path().is_ident("patch_update")
            && !a.path().is_ident("has_init")
            && !a.path().is_ident("has_prepare_resources")
    });

    // Inject `_channel_count: usize` field into the struct so that
    // `self.channel_count()` can return a precomputed value set by the
    // main thread via the constructor.
    if let Data::Struct(ref mut data_struct) = ast.data {
        if let Fields::Named(ref mut fields) = data_struct.fields {
            let field: syn::Field = syn::parse_quote! {
                pub _channel_count: usize
            };
            fields.named.push(field);
        }
    }

    match impl_module_macro_attr(&ast, &attr_args) {
        Ok(generated) => {
            let mut output = quote!(#ast);
            output.extend(generated);
            output.into()
        }
        Err(e) => e.to_compile_error().into(),
    }
}

fn impl_module_macro_attr(
    ast: &DeriveInput,
    attr_args: &ModuleAttrArgs,
) -> syn::Result<TokenStream2> {
    let name = &ast.ident;
    let module_name = &attr_args.module.name;

    // Extract /// doc comments from the module struct for documentation (required)
    let module_documentation = extract_doc_comments(&ast.attrs).ok_or_else(|| {
        syn::Error::new(
            name.span(),
            format!(
                "Module struct `{}` must have `///` doc comments for documentation",
                name
            ),
        )
    })?;
    let module_documentation_token = quote! { #module_documentation.to_string() };

    // Store channels info for channel_count generation
    let hardcoded_channels = attr_args.module.channels;
    let channels_param_name = attr_args.module.channels_param.clone();
    let channels_param_default_val = attr_args.module.channels_param_default;
    let channels_derive_fn = &attr_args.module.channels_derive;

    let module_channels = match attr_args.module.channels {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    };
    let module_channels_param = match &attr_args.module.channels_param {
        Some(s) => quote! { Some(#s.to_string()) },
        None => quote! { None },
    };
    let module_channels_param_default = match attr_args.module.channels_param_default {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    };

    let has_args = attr_args.has_args;
    let positional_args_exprs: Vec<TokenStream2> = attr_args
        .args
        .iter()
        .map(|arg| {
            let arg_name = arg.name.to_string();
            quote! {
                crate::types::PositionalArg {
                    name: #arg_name.to_string(),
                }
            }
        })
        .collect();

    // The module struct must contain a field named `outputs`.
    // Also collect all fields for per-field initialization in the constructor.
    let (outputs_ty, module_field_inits, has_state_field): (Type, Vec<TokenStream2>, bool) =
        match ast.data {
            Data::Struct(ref data) => match data.fields {
                Fields::Named(ref fields) => {
                    // Disallow legacy per-field #[output] annotations on the module struct.
                    if fields
                        .named
                        .iter()
                        .any(|f| unwrap_attr(&f.attrs, "output").is_some())
                    {
                        return Err(syn::Error::new(
                            Span::call_site(),
                            "#[module] expects an `outputs` field (a struct that derives Outputs); do not annotate module fields with #[output(...)]",
                        ));
                    }

                    let outputs_field = fields
                        .named
                        .iter()
                        .find(|f| f.ident.as_ref().map(|i| i == "outputs").unwrap_or(false));

                    let outputs_ty = match outputs_field {
                        Some(f) => f.ty.clone(),
                        None => {
                            return Err(syn::Error::new(
                                Span::call_site(),
                                "#[module] requires a field named `outputs` whose type derives Outputs",
                            ));
                        }
                    };

                    let has_state = fields
                        .named
                        .iter()
                        .any(|f| f.ident.as_ref().map(|i| i == "state").unwrap_or(false));

                    // Generate per-field initialization for the inner module struct.
                    // - `params` → use deserialized params
                    // - `_channel_count` → use deserialized channel count
                    // - `outputs` and `state` → use Default::default()
                    // - other fields → error
                    let field_inits: Vec<TokenStream2> = fields
                        .named
                        .iter()
                        .map(|f| {
                            let field_name = f.ident.as_ref().unwrap();
                            let field_name_str = field_name.to_string();
                            match field_name_str.as_str() {
                                "params" => Ok(quote! { params: *concrete_params }),
                                "_channel_count" => {
                                    Ok(quote! { _channel_count: deserialized.channel_count })
                                }
                                "outputs" | "state" => {
                                    Ok(quote! { #field_name: Default::default() })
                                }
                                other => Err(syn::Error::new(
                                    field_name.span(),
                                    format!(
                                        "Module struct field `{other}` is not allowed. \
                                     Only `state`, `outputs`, and `params` fields are permitted.",
                                    ),
                                )),
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .collect();

                    (outputs_ty, field_inits, has_state)
                }
                Fields::Unnamed(_) | Fields::Unit => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "#[module] can only be applied to structs with named fields",
                    ));
                }
            },
            Data::Enum(_) | Data::Union(_) => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "#[module] can only be applied to structs",
                ));
            }
        };

    let struct_name = format_ident!("{}Sampleable", name);
    let constructor_name = format_ident!("{}Constructor", name)
        .to_string()
        .to_case(Case::Snake);
    let constructor_name = Ident::new(&constructor_name, Span::call_site());
    let params_struct_name = format_ident!("{}Params", name);

    // Extract generics for proper impl blocks
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();

    // For the wrapper struct, we need to replace all lifetime parameters with 'static
    // since Sampleable requires 'static. Build a static version of ty_generics.
    let static_ty_generics = {
        let params = ast
            .generics
            .params
            .iter()
            .map(|p| match p {
                syn::GenericParam::Lifetime(_) => quote!('static),
                syn::GenericParam::Type(t) => {
                    let ident = &t.ident;
                    quote!(#ident)
                }
                syn::GenericParam::Const(c) => {
                    let ident = &c.ident;
                    quote!(#ident)
                }
            })
            .collect::<Vec<_>>();
        if params.is_empty() {
            quote!()
        } else {
            quote!(<#(#params),*>)
        }
    };

    let is_stateful = attr_args.stateful;

    let get_state_impl = if is_stateful {
        if has_args {
            // Stateful module with positional args - merge argument_spans into state
            quote! {
                use crate::types::StatefulModule;
                // SAFETY: Audio thread has exclusive access. See crate::types module documentation.
                let module = unsafe { &*self.module.get() };
                let argument_spans = unsafe { &*self.argument_spans.get() };

                // Get base state from module's StatefulModule impl
                let state = module.get_state();

                // If we have argument spans, merge them into the state
                if argument_spans.is_empty() {
                    state
                } else {
                    match (state, serde_json::to_value(argument_spans).ok()) {
                        (Some(serde_json::Value::Object(mut obj)), Some(spans)) => {
                            obj.insert("argument_spans".to_string(), spans);
                            Some(serde_json::Value::Object(obj))
                        }
                        (Some(state_val), Some(spans)) => {
                            // State exists but isn't an object - wrap it
                            Some(serde_json::json!({
                                "_state": state_val,
                                "argument_spans": spans
                            }))
                        }
                        (None, Some(spans)) => {
                            // No base state, create one with just argument_spans
                            Some(serde_json::json!({
                                "argument_spans": spans
                            }))
                        }
                        (state, None) => state,
                    }
                }
            }
        } else {
            // Stateful module without args - just return module state
            quote! {
                use crate::types::StatefulModule;
                // SAFETY: Audio thread has exclusive access. See crate::types module documentation.
                let module = unsafe { &*self.module.get() };
                module.get_state()
            }
        }
    } else if has_args {
        // Non-stateful module with args - return argument_spans only if present
        quote! {
            let argument_spans = unsafe { &*self.argument_spans.get() };
            if !argument_spans.is_empty() {
                serde_json::to_value(std::collections::HashMap::from([
                    ("argument_spans".to_string(), argument_spans.clone())
                ])).ok()
            } else {
                None
            }
        }
    } else {
        quote! { None }
    };

    // Check for has_init flag
    let has_init_call = if attr_args.has_init {
        quote! {
            // SAFETY: We just created sampleable, no one else has access yet.
            unsafe { (*sampleable.module.get()).init(sample_rate); }
        }
    } else {
        quote! {}
    };

    // Check for has_prepare_resources flag
    let prepare_resources_impl = if attr_args.has_prepare_resources {
        quote! {
            fn prepare_resources(
                &self,
                wav_data: &std::collections::HashMap<String, std::sync::Arc<crate::types::WavData>>,
            ) {
                // SAFETY: Called on the main thread between construction and
                // queueing the module for the audio thread. No other
                // references to `self.module` exist at this point.
                let module = unsafe { &mut *self.module.get() };
                module.prepare_resources_impl(wav_data, self.sample_rate);
            }
        }
    } else {
        quote! {}
    };

    // Check for patch_update flag
    let on_patch_update_impl = if attr_args.patch_update {
        quote! {
            fn on_patch_update(&self) {
                use crate::types::PatchUpdateHandler;
                // SAFETY: Audio thread has exclusive access. See crate::types module documentation.
                let module = unsafe { &mut *self.module.get() };
                PatchUpdateHandler::on_patch_update(module);
            }
        }
    } else {
        quote! {
            fn on_patch_update(&self) {}
        }
    };

    // Check for clock_sync flag
    let clock_sync_impl = if attr_args.clock_sync {
        quote! {
            fn sync_external_clock(&self, bar_phase: f64, bpm: f64) {
                let module = unsafe { &mut *self.module.get() };
                module.sync_external_clock(bar_phase, bpm);
            }

            fn clear_external_sync(&self) {
                let module = unsafe { &mut *self.module.get() };
                module.clear_external_sync();
            }
        }
    } else {
        quote! {}
    };

    // Generate transfer_state_from body - only swap state if the module has a `state` field
    let transfer_state_body = if has_state_field {
        quote! {
            std::mem::swap(&mut new_inner.state, &mut old_inner.state);
        }
    } else {
        // No state field, nothing to transfer (buffer transfer handled below)
        quote! {}
    };

    // Generate the channel count derivation function name
    let channel_count_fn_name = format_ident!(
        "__{}_derive_channel_count",
        name.to_string().to_case(Case::Snake)
    );

    // Generate the core channel count implementation that works with typed params.
    let channel_count_fn_impl = if let Some(custom_fn) = channels_derive_fn {
        quote! {
            #[inline]
            fn #channel_count_fn_name(params: &#params_struct_name) -> usize {
                #custom_fn(params)
            }
        }
    } else {
        match (
            hardcoded_channels,
            &channels_param_name,
            channels_param_default_val,
        ) {
            (Some(n), _, _) => {
                let n = n as usize;
                quote! {
                    #[inline]
                    fn #channel_count_fn_name(_params: &#params_struct_name) -> usize {
                        #n
                    }
                }
            }
            (None, Some(param_name), default_val) => {
                let param_ident = Ident::new(&param_name.value(), param_name.span());
                match default_val {
                    Some(default) => {
                        let default = default as usize;
                        quote! {
                            #[inline]
                            fn #channel_count_fn_name(params: &#params_struct_name) -> usize {
                                let param_value = params.#param_ident;
                                if param_value > 0 {
                                    param_value.clamp(1, crate::poly::PORT_MAX_CHANNELS)
                                } else {
                                    #default
                                }
                            }
                        }
                    }
                    None => {
                        quote! {
                            #[inline]
                            fn #channel_count_fn_name(params: &#params_struct_name) -> usize {
                                params.#param_ident.clamp(1, crate::poly::PORT_MAX_CHANNELS)
                            }
                        }
                    }
                }
            }
            (None, None, _) => {
                quote! {
                    #[inline]
                    fn #channel_count_fn_name(params: &#params_struct_name) -> usize {
                        use crate::types::PolySignalFields;
                        let fields = params.poly_signal_fields();
                        let refs: Vec<&crate::poly::PolySignal> = fields.into_iter().collect();
                        crate::poly::PolySignal::max_channels(&refs).max(1) as usize
                    }
                }
            }
        }
    };

    let generated = quote! {
        // Generated core channel count function (used by derive_channel_count and initial default)
        // IMPORTANT: This function should never be called within the audio thread.
        // It may be computationally expensive. It should only be called in non-audio-thread contexts.
        #channel_count_fn_impl

        impl #impl_generics #name #ty_generics #where_clause {
            /// Returns the precomputed channel count set during construction.
            #[inline]
            pub fn channel_count(&self) -> usize {
                self._channel_count
            }
        }

        /// Generated wrapper struct for audio-thread-only module access.
        ///
        /// # Safety Model (UnsafeCell)
        ///
        /// This struct uses `UnsafeCell` instead of `Mutex`/`RwLock` for interior mutability.
        /// This is safe because:
        ///
        /// 1. **Exclusive Audio Thread Ownership**: After construction, all modules live in
        ///    `AudioProcessor::patch` which is owned exclusively by the audio thread closure.
        ///    See `crates/modular/src/audio.rs` `make_stream()`.
        ///
        /// 2. **Command Queue Isolation**: The main thread communicates via `PatchUpdate`
        ///    commands through an `rtrb` SPSC queue. It never directly accesses module state.
        ///
        /// 3. **No Escaping References**: Owned modules are stored in `Patch::sampleables` and
        ///    are never aliased across threads after being added to the patch.
        ///
        /// ## Invariants (DO NOT VIOLATE)
        ///
        /// - **NEVER** call Sampleable trait methods from the main thread
        /// - **NEVER** share module ownership across threads
        /// - **NEVER** access Patch::sampleables from outside AudioProcessor
        /// - **ALWAYS** use the command queue for main→audio communication
        ///
        /// Violating these invariants will cause undefined behavior (data races).
        struct #struct_name {
            id: String,
            outputs: std::cell::UnsafeCell<#outputs_ty>,
            module: std::cell::UnsafeCell<#name #static_ty_generics>,
            processed: core::sync::atomic::AtomicBool,
            sample_rate: f32,
            argument_spans: std::cell::UnsafeCell<std::collections::HashMap<String, crate::params::ArgumentSpan>>,
        }

        // SAFETY: This type is only accessed from the audio thread after construction.
        unsafe impl Send for #struct_name {}
        unsafe impl Sync for #struct_name {}

        impl crate::types::Sampleable for #struct_name {
            fn tick(&self) {
                self.processed.store(false, core::sync::atomic::Ordering::Release);
            }

            fn update(&self) {
                if let Ok(_) = self.processed.compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::Acquire,
                    core::sync::atomic::Ordering::Relaxed,
                ) {
                    unsafe {
                        let module = &mut *self.module.get();
                        module.update(self.sample_rate);
                        let outputs = &mut *self.outputs.get();
                        crate::types::OutputStruct::copy_from(outputs, &module.outputs);
                    }
                }
            }

            fn get_poly_sample(&self, port: &str) -> napi::Result<crate::poly::PolyOutput> {
                self.update();
                let outputs = unsafe { &*self.outputs.get() };
                crate::types::OutputStruct::get_poly_sample(outputs, port).ok_or_else(|| {
                    napi::Error::from_reason(
                        format!(
                            "{} with id {} does not have port {}",
                            #module_name,
                            &self.id,
                            port
                        )
                    )
                })
            }

            fn get_module_type(&self) -> &str {
                #module_name
            }

            fn get_id(&self) -> &str {
                &self.id
            }

            fn connect(&self, patch: &crate::Patch) {
                let module = unsafe { &mut *self.module.get() };
                crate::types::Connect::connect(&mut module.params, patch);
            }

            #on_patch_update_impl

            #prepare_resources_impl

            #clock_sync_impl

            fn get_state(&self) -> Option<serde_json::Value> {
                #get_state_impl
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn get_buffer_output(&self, port: &str) -> Option<&std::sync::Arc<crate::BufferData>> {
                let module = unsafe { &*self.module.get() };
                crate::types::OutputStruct::get_buffer_output(&module.outputs, port)
            }

            fn transfer_state_from(&self, old: &dyn crate::types::Sampleable) {
                if let Some(old_typed) = old.as_any().downcast_ref::<Self>() {
                    // Guard against self-aliasing: if old and new are the same box,
                    // creating two &mut refs to the same UnsafeCell contents is UB.
                    if std::ptr::eq(self as *const Self, old_typed as *const Self) {
                        return;
                    }
                    let new_inner = unsafe { &mut *self.module.get() };
                    let old_inner = unsafe { &mut *old_typed.module.get() };
                    #transfer_state_body
                    // Transfer buffer data (no-op for modules without buffer outputs)
                    crate::types::OutputStruct::transfer_buffers_from(
                        &mut new_inner.outputs,
                        &mut old_inner.outputs,
                    );
                    // Transfer wrapper outputs so feedback cycles read previous-frame
                    // values instead of zeros on the patch-update frame. Without this,
                    // the module that is "second" in a cycle reads Default outputs
                    // (all zeros) from the "first" module's wrapper, injecting a
                    // one-sample discontinuity into the feedback loop.
                    unsafe {
                        let new_outputs = &mut *self.outputs.get();
                        let old_outputs = &mut *old_typed.outputs.get();
                        std::mem::swap(new_outputs, old_outputs);
                    }
                }
            }
        }

        fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<Box<dyn crate::types::Sampleable>> {
            let concrete_params = deserialized.params.into_any()
                .downcast::<#params_struct_name>()
                .map_err(|_| napi::Error::from_reason(
                    format!("Failed to downcast params for module type {}", #module_name)
                ))?;

            // Construct inner module with per-field initialization.
            // `params` comes from deserialization, `_channel_count` from computed channel count,
            // all other fields use Default::default().
            let mut inner = #name #static_ty_generics {
                #(#module_field_inits),*
            };
            crate::types::OutputStruct::set_all_channels(&mut inner.outputs, deserialized.channel_count);

            let sampleable = #struct_name {
                id: id.clone(),
                sample_rate,
                outputs: std::cell::UnsafeCell::new(Default::default()),
                module: std::cell::UnsafeCell::new(inner),
                processed: core::sync::atomic::AtomicBool::new(false),
                argument_spans: std::cell::UnsafeCell::new(deserialized.argument_spans),
            };

            #has_init_call
            Ok(Box::new(sampleable))
        }

        impl #impl_generics crate::types::Module for #name #ty_generics #where_clause {
            fn install_constructor(map: &mut std::collections::HashMap<String, crate::types::SampleableConstructor>) {
                map.insert(#module_name.into(), Box::new(#constructor_name));
            }

            fn install_params_deserializer(map: &mut std::collections::HashMap<String, crate::params::ParamsDeserializer>) {
                fn deserializer(params: serde_json::Value) -> std::result::Result<crate::params::CachedParams, crate::param_errors::ModuleParamErrors> {
                    let parsed: #params_struct_name = deserr::deserialize::<_, _, crate::param_errors::ModuleParamErrors>(params)?;
                    let channel_count = #channel_count_fn_name(&parsed);
                    Ok(crate::params::CachedParams {
                        params: Box::new(parsed),
                        channel_count,
                    })
                }
                map.insert(#module_name.into(), deserializer as crate::params::ParamsDeserializer);
            }

            fn get_schema() -> crate::types::ModuleSchema {
                let params_schema = schemars::schema_for!(#params_struct_name);

                let param_names: std::collections::HashSet<String> = params_schema
                    .pointer("/properties")
                    .or_else(|| params_schema.pointer("/schema/properties"))
                    .and_then(serde_json::Value::as_object)
                    .map(|props| props.keys().cloned().collect())
                    .unwrap_or_default();

                let outputs = <#outputs_ty as crate::types::OutputStruct>::schemas();
                let output_names: std::collections::HashSet<String> = outputs.iter().map(|o| o.name.clone()).collect();
                let overlap: Vec<&String> = param_names.intersection(&output_names).collect();
                if !overlap.is_empty() {
                    panic!(
                        "Parameters and outputs must have unique names for module '{}'. Overlapping: {:?}",
                        #module_name,
                        overlap,
                    );
                }

                crate::types::ModuleSchema {
                    name: #module_name.to_string(),
                    documentation: #module_documentation_token,
                    params_schema: crate::types::SchemaContainer {
                        schema: params_schema,
                    },
                    outputs,
                    signal_params: <#params_struct_name as crate::types::SignalParamMeta>::signal_param_schemas(),
                    positional_args: vec![
                        #(#positional_args_exprs),*
                    ],
                    channels: #module_channels,
                    channels_param: #module_channels_param,
                    channels_param_default: #module_channels_param_default,
                }
            }
        }
    };
    Ok(generated)
}
