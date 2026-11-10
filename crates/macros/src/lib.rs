//! rust-di procedural macros.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

mod field_init;
mod inject_attr;
mod inject_derive;
mod module;

// ───────────────────────────────────────────────────────────────────────────
// #[derive(Inject)] — auto-generate constructor from struct fields
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro_derive(Inject, attributes(inject))]
pub fn inject_derive(input: TokenStream) -> TokenStream {
    inject_derive::expand_inject_derive(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// ───────────────────────────────────────────────────────────────────────────
// register!() — function-like placeholder macro for use inside #[module]
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro]
pub fn register(_: TokenStream) -> TokenStream {
    quote! {}.into()
}

// ───────────────────────────────────────────────────────────────────────────
// #[module] — compile-time module scanning for register!() declarations
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro_attribute]
pub fn module(_: TokenStream, item: TokenStream) -> TokenStream {
    module::expand_md(parse_macro_input!(item as syn::ItemMod))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// ───────────────────────────────────────────────────────────────────────────
// #[inject] — attribute macro for auto-registration
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro_attribute]
pub fn inject(attr: TokenStream, item: TokenStream) -> TokenStream {
    inject_attr::expand_inject(
        parse_macro_input!(attr as inject_attr::InjectArgs),
        parse_macro_input!(item as syn::Item),
    )
    .unwrap_or_else(|e| e.to_compile_error())
    .into()
}
