use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

pub(crate) fn expand_inject_derive(input: syn::DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let fn_name = quote::format_ident!("__rdi_construct_{}", name);
    let constructor_body = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(named) => {
                let mut inits = Vec::new();
                for field in named.named.iter() {
                    inits.push(crate::field_init::gen_field_init(field)?);
                }
                quote! { #name { #(#inits),* } }
            }
            syn::Fields::Unit => quote! { #name },
            _ => return Err(syn::Error::new_spanned(name, "named fields or unit struct")),
        },
        _ => return Err(syn::Error::new_spanned(name, "struct only")),
    };
    Ok(quote! {#[doc(hidden)]pub fn #fn_name(resolver:&dyn rust_dicore::IServiceResolver)->::std::sync::Arc<#name>{::std::sync::Arc::new(#constructor_body)}})
}