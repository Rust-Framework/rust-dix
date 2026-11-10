use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

pub(crate) fn expand_inject_derive(input: syn::DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let try_fn = quote::format_ident!("__rdi_try_construct_{}", name);
    let fn_name = quote::format_ident!("__rdi_construct_{}", name);
    let constructor_body = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(named) => {
                let mut inits = Vec::new();
                for field in named.named.iter() {
                    inits.push(crate::field_init::gen_field_init(field, name)?);
                }
                quote! { #name { #(#inits),* } }
            }
            syn::Fields::Unit => quote! { #name },
            _ => return Err(syn::Error::new_spanned(name, "named fields or unit struct")),
        },
        _ => return Err(syn::Error::new_spanned(name, "struct only")),
    };
    Ok(quote! {
            #[doc(hidden)]
            pub fn #try_fn(
                resolver: &dyn rust_dix::IServiceResolver,
            ) -> ::std::result::Result<::std::sync::Arc<#name>, rust_dix::RdiError> {
                ::std::result::Result::Ok(::std::sync::Arc::new(#constructor_body))
            }

    /// Infallible inject constructor for `ServiceCollection` factories.
    ///
    /// Prefer `__rdi_try_construct_*` when you need typed [`rust_dix::RdiError`].
    /// Failures panic with `RdiError` as the payload so `ServiceProvider::get`
    /// can recover a typed error via `catch_unwind`.
    #[doc(hidden)]
    pub fn #fn_name(resolver: &dyn rust_dix::IServiceResolver) -> ::std::sync::Arc<#name> {
        rust_dix::inject_arc(#try_fn(resolver))
    }
        })
}
