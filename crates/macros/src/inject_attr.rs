use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    ItemImpl,
};

use crate::field_init::gen_field_init;
use crate::module::LT;

pub(crate) fn lt_to_token(lt: LT) -> TokenStream2 {
    match lt {
        LT::S => quote! { rust_dix::ServiceLifetime::Singleton },
        LT::Sc => quote! { rust_dix::ServiceLifetime::Scoped },
        LT::T => quote! { rust_dix::ServiceLifetime::Transient },
    }
}

pub(crate) struct InjectArgs {
    pub lifetime: Option<LT>,
}

impl Parse for InjectArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(InjectArgs { lifetime: None });
        }
        let lt: LT = input.parse()?;
        Ok(InjectArgs { lifetime: Some(lt) })
    }
}

pub(crate) fn expand_inject(
    args: InjectArgs,
    item: syn::Item,
) -> syn::Result<TokenStream2> {
    let lifetime = args.lifetime.unwrap_or(LT::S);
    match &item {
        syn::Item::Struct(s) => expand_inject_struct(s, lifetime),
        syn::Item::Impl(i) => expand_inject_impl(i, lifetime),
        _ => Err(syn::Error::new_spanned(
            &item,
            "#[inject] can only be placed on a struct or a trait impl block",
        )),
    }
}

fn expand_inject_struct(
    struct_item: &syn::ItemStruct,
    lifetime: LT,
) -> syn::Result<TokenStream2> {
    let name = &struct_item.ident;
    let fn_name = format_ident!("__rdi_construct_{}", name);
    let factory_name = format_ident!("__rdi_factory_{}", name);
    let type_name_fn_name = format_ident!("__rdi_type_name_{}", name);

    let constructor_body = match &struct_item.fields {
        syn::Fields::Named(n) => {
            let mut inits = Vec::new();
            for field in n.named.iter() {
                inits.push(gen_field_init(field)?);
            }
            quote! { #name { #(#inits),* } }
        }
        syn::Fields::Unit => quote! { #name },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "named struct or unit struct required",
            ))
        }
    };

    let constructor = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #fn_name(resolver: &dyn rust_dix::IServiceResolver) -> ::std::sync::Arc<#name> {
            ::std::sync::Arc::new(#constructor_body)
        }
    };

    let lt_ident = lt_to_token(lifetime);

    let factory = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #factory_name(resolver: &dyn rust_dix::IServiceResolver) -> ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync> {
            let v: ::std::sync::Arc<#name> = #fn_name(resolver);
            ::std::sync::Arc::new(v)
                as ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>
        }
    };

    let type_name_helper = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #type_name_fn_name() -> &'static str {
            ::std::any::type_name::<#name>()
        }
    };

    let submission = quote! {
        rust_dix::inventory::submit! {
            rust_dix::ServiceRegistration {
                lifetime: #lt_ident,
                type_id: ::std::any::TypeId::of::<#name>(),
                type_name_fn: #type_name_fn_name,
                factory: #factory_name,
            }
        }
    };

    let mut stripped = struct_item.clone();
    if let syn::Fields::Named(n) = &mut stripped.fields {
        for field in n.named.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("inject"));
        }
    }

    Ok(quote! {
        #stripped
        #constructor
        #type_name_helper
        #factory
        #submission
    })
}

fn expand_inject_impl(
    impl_item: &ItemImpl,
    lifetime: LT,
) -> syn::Result<TokenStream2> {
    let (not, trait_path, _) = impl_item.trait_.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(
            impl_item,
            "#[inject] on an impl block requires a trait impl: use `impl Trait for Type`",
        )
    })?;
    if not.is_some() {
        return Err(syn::Error::new_spanned(
            impl_item,
            "#[inject] does not support negative impls",
        ));
    }

    let impl_ty = &impl_item.self_ty;
    let type_ident = match &**impl_ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.clone()),
        _ => None,
    }
    .ok_or_else(|| syn::Error::new_spanned(impl_ty, "expected a path type"))?;

    let trait_segment = trait_path.segments.last().unwrap();
    let trait_suffix = &trait_segment.ident;

    let constructor_fn = format_ident!("__rdi_construct_{}", type_ident);
    let factory_name = format_ident!("__rdi_factory_{}_for_{}", type_ident, trait_suffix);
    let type_name_fn_name = format_ident!("__rdi_type_name_{}_for_{}", type_ident, trait_suffix);

    let lt_ident = lt_to_token(lifetime);
    let trait_ty = quote! { dyn #trait_path };

    let factory = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #factory_name(resolver: &dyn rust_dix::IServiceResolver) -> ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync> {
            let v: ::std::sync::Arc<#impl_ty> = #constructor_fn(resolver);
            let v2: ::std::sync::Arc<#trait_ty> = v;
            ::std::sync::Arc::new(v2)
                as ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>
        }
    };

    let type_name_helper = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #type_name_fn_name() -> &'static str {
            ::std::any::type_name::<#trait_ty>()
        }
    };

    let submission = quote! {
        rust_dix::inventory::submit! {
            rust_dix::ServiceRegistration {
                lifetime: #lt_ident,
                type_id: ::std::any::TypeId::of::<#trait_ty>(),
                type_name_fn: #type_name_fn_name,
                factory: #factory_name,
            }
        }
    };

    Ok(quote! {
        #impl_item
        #type_name_helper
        #factory
        #submission
    })
}