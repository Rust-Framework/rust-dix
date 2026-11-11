//! Procedural macros for [`rust-dix`](https://docs.rs/rust-dix)
//! (`rust-dix-macros`).
//!
//! This crate is an implementation detail of `rust-dix`; depend on
//! `rust-dix`, which re-exports these macros (do not depend on this crate
//! directly unless you have a specific reason).
//!
//! # Macros
//!
//! - [`macro@Inject`] — `#[derive(Inject)]` generates constructor injection
//!   code from struct fields (fields marked `#[inject]`,
//!   `#[inject(owned)]`, `#[inject(provider)]`, `#[inject(key = "...")]` or
//!   `#[inject(owned, key = "...")]`).
//! - [`macro@inject`] — attribute macro placed on structs or trait impls to
//!   auto-register services with the container.
//! - [`macro@module`] — compile-time module scan that collects
//!   `register!()` declarations.
//! - [`macro@register`] — function-like placeholder used inside
//!   `#[rust_dix::module]`.
//!
//! # Example
//!
//! Illustrative only (the macros generate code that references `rust_dix`, so
//! this proc-macro crate's doctests cannot link the core crate; the same
//! patterns are compiled in `rust-dix`' test suite):
//!
//! ```rust,ignore
//! use rust_dix::{inject, Inject};
//! use std::sync::Arc;
//!
//! struct Config;
//! trait IWorker: Send + Sync {}
//!
//! #[derive(Inject)]
//! struct Worker {
//!     #[inject]
//!     config: Arc<Config>,
//! }
//!
//! #[inject(singleton)]
//! impl IWorker for Worker {}
//! ```
//!
//! # Engineering baseline
//!
//! MSRV **Rust 1.95**; `#![forbid(unsafe_code)]`; errors are emitted as
//! compile errors instead of panics where possible. See the
//! [repository security policy](https://github.com/Rust-Framework/rust-dix/blob/main/SECURITY.md).

#![forbid(unsafe_code)]

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
