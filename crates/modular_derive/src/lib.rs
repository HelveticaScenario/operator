extern crate proc_macro;
extern crate quote;
extern crate syn;

mod block_inputs;
mod channel_count;
mod connect;
mod enum_tag;
mod message_handlers;
mod module_attr;
mod outputs;
mod signal_params;
mod utils;

use proc_macro::TokenStream;
use syn::DeriveInput;

#[proc_macro]
pub fn message_handlers(input: TokenStream) -> TokenStream {
    message_handlers::message_handlers_impl(input)
}

#[proc_macro_derive(Outputs, attributes(output))]
pub fn outputs_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    outputs::impl_outputs_macro(&ast)
}

#[proc_macro_derive(EnumTag, attributes(enum_tag))]
pub fn enum_tag_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    enum_tag::impl_enum_tag_macro(&ast)
}

#[proc_macro_derive(Connect, attributes(default_connection))]
pub fn connect_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    connect::impl_connect_macro(&ast)
}

#[proc_macro_derive(ChannelCount)]
pub fn channel_count_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    channel_count::impl_channel_count_macro(&ast)
}

#[proc_macro_derive(SignalParams, attributes(signal))]
pub fn signal_params_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    signal_params::impl_signal_params_macro(&ast)
}

#[proc_macro_derive(BlockInputs, attributes(signal, block_input))]
pub fn block_inputs_macro_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = match syn::parse(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    block_inputs::impl_block_inputs_macro(&ast)
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    module_attr::module_impl(attr, item)
}
