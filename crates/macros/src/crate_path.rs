//! Resolve the real dependency name of the `rust-dix` crate.
//!
//! Before DIX-1 the macros hardcoded `::rust_dix`, so a downstream crate that
//! renamed the dependency (`dix = { package = "rust-dix" }`) had to add
//! `extern crate dix as rust_dix;` shims to its crate roots. Expansions now
//! address the crate through [`dix_crate_path`].

use proc_macro2::{Span, TokenStream};
use quote::quote;

/// Path to the `rust-dix` crate as seen from the crate the macro expands into.
///
/// - Dependency found (renamed or not) → `::<real name>`, e.g. `::dix`.
/// - Expanding inside `rust-dix` itself (unit tests) → `crate`.
/// - Doctests are compiled as a separate crate linked against the library, so
///   `crate` would point at the doctest wrapper; rustdoc flags them with
///   `UNSTABLE_RUSTDOC_TEST_PATH`, and the library is `::rust_dix` there.
/// - Manifest unreadable or dependency absent → keep the historical
///   `::rust_dix` so `rust-dix-macros` used directly behaves as before.
pub(crate) fn dix_crate_path() -> TokenStream {
    match proc_macro_crate::crate_name("rust-dix") {
        Ok(proc_macro_crate::FoundCrate::Itself) => {
            if std::env::var_os("UNSTABLE_RUSTDOC_TEST_PATH").is_some() {
                quote!(::rust_dix)
            } else {
                quote!(crate)
            }
        }
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
        Err(_) => quote!(::rust_dix),
    }
}
