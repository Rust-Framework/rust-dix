use proc_macro2::TokenStream;
use quote::quote;
use syn::{Field, Type};

/// Classify a field's type for injection strategy.
///
/// - `Arc`        → `Arc<T>`           : shared, resolve via `get_any` + downcast.
/// - `OptionArc`  → `Option<Arc<T>>`   : optional shared, returns `Option<Arc<T>>`.
/// - `Owned`      → bare `T`           : owned, resolve via `get_owned_any` + `try_unwrap`.
/// - `OptionOwned`→ `Option<T>`        : optional owned, returns `Option<T>`.
/// - `VecArc`     → `Vec<Arc<T>>`      : polymorphic, resolve via `get_all_any` + downcast each.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FieldKind {
    Arc,
    OptionArc,
    Owned,
    OptionOwned,
    VecArc,
}

pub(crate) fn classify_field(ty: &Type) -> (TokenStream, FieldKind) {
    if let Type::Path(p) = ty {
        if let Some(last) = p.path.segments.last() {
            match last.ident.to_string().as_str() {
                "Arc" => {
                    if let Some(inner) = single_generic_type(last) {
                        return (quote! {#inner}, FieldKind::Arc);
                    }
                }
                "Option" => {
                    if let Some(inner_ty) = single_generic_type(last) {
                        if let Type::Path(ip) = inner_ty {
                            if let Some(ilast) = ip.path.segments.last() {
                                if ilast.ident == "Arc" {
                                    if let Some(t) = single_generic_type(ilast) {
                                        return (quote! {#t}, FieldKind::OptionArc);
                                    }
                                } else {
                                    return (quote! {#inner_ty}, FieldKind::OptionOwned);
                                }
                            }
                        }
                    }
                }
                "Vec" => {
                    if let Some(inner) = single_generic_type(last) {
                        if let Type::Path(ip) = inner {
                            if let Some(ilast) = ip.path.segments.last() {
                                if ilast.ident == "Arc" {
                                    if let Some(t) = single_generic_type(ilast) {
                                        return (quote! {#t}, FieldKind::VecArc);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (quote! {#ty}, FieldKind::Owned)
}

pub(crate) fn single_generic_type(seg: &syn::PathSegment) -> Option<&Type> {
    if let syn::PathArguments::AngleBracketed(a) = &seg.arguments {
        if let Some(syn::GenericArgument::Type(t)) = a.args.first() {
            return Some(t);
        }
    }
    None
}

/// Generate the field initializer expression for a single struct field.
///
/// Shared between `#[derive(Inject)]` and `#[inject]` on a struct to avoid
/// duplicating the owned/Arc dispatch logic.
pub(crate) fn gen_field_init(field: &Field) -> syn::Result<TokenStream> {
    let attrs = parse_ia(field);
    let fnm = field.ident.as_ref().unwrap();
    let (inner, kind) = classify_field(&field.ty);

    if !attrs.inject && !attrs.provider {
        return Ok(quote! { #fnm: ::std::default::Default::default() });
    }
    if attrs.provider {
        return Ok(quote! {
            #fnm: resolver.provider_arc().expect(
                "ServiceProvider not available; use ServiceProvider::into_arc() to enable #[inject(provider)]"
            )
        });
    }

    let arc_extract = quote! {
        .and_then(|a| a.downcast::<::std::sync::Arc<#inner>>().ok())
        .map(|d| ::std::sync::Arc::clone(&*d))
    };
    let owned_extract = quote! {
        .and_then(|a| a.downcast::<::std::sync::Arc<#inner>>().ok())
        .map(|d| ::std::sync::Arc::clone(&*d))
        .and_then(|arc| ::std::sync::Arc::try_unwrap(arc).ok())
    };

    let key = &attrs.key;

    if !attrs.owned {
        match kind {
            FieldKind::OptionArc => {
                if let Some(k) = key {
                    Ok(quote! { #fnm: resolver.get_keyed_any(::std::any::type_name::<#inner>(), #k) #arc_extract })
                } else {
                    Ok(quote! { #fnm: resolver.get_any(::std::any::type_name::<#inner>()) #arc_extract })
                }
            }
            FieldKind::Arc => {
                if let Some(k) = key {
                    Ok(quote! { #fnm: resolver.get_keyed_any(::std::any::type_name::<#inner>(), #k) #arc_extract .unwrap_or_else(|| ::std::panic!("keyed not found")) })
                } else {
                    Ok(quote! { #fnm: resolver.get_any(::std::any::type_name::<#inner>()) #arc_extract .unwrap_or_else(|| ::std::panic!("svc not registered")) })
                }
            }
            FieldKind::VecArc => {
                if attrs.key.is_some() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "#[inject] on Vec<Arc<T>> does not support key; key is per-implementation",
                    ));
                }
                Ok(quote! {
                    #fnm: resolver.get_all_any(::std::any::type_name::<#inner>())
                        .into_iter()
                        .map(|a| a.downcast::<::std::sync::Arc<#inner>>()
                            .ok()
                            .unwrap_or_else(|| ::std::panic!(
                                "polymorphic inject: type mismatch for {}",
                                ::std::any::type_name::<#inner>()
                            )))
                        .map(|d| ::std::sync::Arc::clone(&*d))
                        .collect::<::std::vec::Vec<::std::sync::Arc<#inner>>>()
                })
            }
            _ => Err(syn::Error::new_spanned(
                field,
                "#[inject] requires Arc<T>, Option<Arc<T>>, or Vec<Arc<T>> field type; use #[inject(owned)] for bare T",
            )),
        }
    } else {
        match kind {
            FieldKind::OptionOwned => {
                if let Some(k) = key {
                    Ok(quote! { #fnm: resolver.get_keyed_owned_any(::std::any::type_name::<#inner>(), #k) #owned_extract })
                } else {
                    Ok(quote! { #fnm: resolver.get_owned_any(::std::any::type_name::<#inner>()) #owned_extract })
                }
            }
            FieldKind::Owned => {
                if let Some(k) = key {
                    Ok(quote! { #fnm: resolver.get_keyed_owned_any(::std::any::type_name::<#inner>(), #k) #owned_extract .unwrap_or_else(|| ::std::panic!("keyed owned not found")) })
                } else {
                    Ok(quote! { #fnm: resolver.get_owned_any(::std::any::type_name::<#inner>()) #owned_extract .unwrap_or_else(|| ::std::panic!("owned svc not registered or Singleton")) })
                }
            }
            _ => Err(syn::Error::new_spanned(
                field,
                "#[inject(owned)] requires bare T or Option<T> field type; use #[inject] for Arc<T>",
            )),
        }
    }
}

#[derive(Default)]
pub(crate) struct IA {
    pub inject: bool,
    pub owned: bool,
    pub provider: bool,
    pub key: Option<String>,
}

pub(crate) fn parse_ia(f: &Field) -> IA {
    let mut a = IA::default();
    for attr in &f.attrs {
        if !attr.path().is_ident("inject") {
            continue;
        }
        a.inject = true;
        let Ok(l) = attr.meta.require_list() else {
            continue;
        };
        l.parse_nested_meta(|m| {
            if m.path.is_ident("owned") {
                a.owned = true;
            } else if m.path.is_ident("provider") {
                a.provider = true;
                a.inject = false;
            } else if m.path.is_ident("key") {
                a.key = Some(m.value()?.parse::<syn::LitStr>()?.value());
            }
            Ok(())
        })
        .ok();
    }
    a
}