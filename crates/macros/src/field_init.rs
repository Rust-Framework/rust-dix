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
                    if let Some(Type::Path(ip)) = single_generic_type(last) {
                        if let Some(ilast) = ip.path.segments.last() {
                            if ilast.ident == "Arc" {
                                if let Some(t) = single_generic_type(ilast) {
                                    return (quote! {#t}, FieldKind::VecArc);
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
/// Expressions are written for a `Result`-returning constructor body (`?` ok).
/// Shared between `#[derive(Inject)]` and `#[inject]` on a struct.
pub(crate) fn gen_field_init(field: &Field, consumer: &syn::Ident) -> syn::Result<TokenStream> {
    let attrs = parse_ia(field);
    let fnm = field.ident.as_ref().unwrap();
    let (inner, kind) = classify_field(&field.ty);
    let field_name = fnm.to_string();
    let consumer_ty = quote! { ::std::any::type_name::<#consumer>() };

    if !attrs.inject && !attrs.provider {
        return Ok(quote! { #fnm: ::std::default::Default::default() });
    }
    if attrs.provider {
        if attrs.key.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "#[inject(provider)] cannot be combined with key; inject Arc<ServiceProvider> and resolve keyed services on demand",
            ));
        }
        return Ok(quote! {
            #fnm: ::rust_dix::macros_support::resolve_provider(
                resolver,
                #consumer_ty,
                #field_name,
            )?
        });
    }

    let key = &attrs.key;

    if !attrs.owned {
        match kind {
            FieldKind::OptionArc => {
                if let Some(k) = key {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_option_keyed_arc::<#inner>(
                            resolver,
                            #k,
                        )
                    })
                } else {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_option_arc::<#inner>(resolver)
                    })
                }
            }
            FieldKind::Arc => {
                if let Some(k) = key {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_keyed_arc::<#inner>(
                            resolver,
                            #k,
                            #consumer_ty,
                            #field_name,
                        )?
                    })
                } else {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_arc::<#inner>(
                            resolver,
                            #consumer_ty,
                            #field_name,
                        )?
                    })
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
                    #fnm: ::rust_dix::macros_support::resolve_vec_arc::<#inner>(
                        resolver,
                        #consumer_ty,
                        #field_name,
                    )?
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
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_option_keyed_owned::<#inner>(
                            resolver,
                            #k,
                        )
                    })
                } else {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_option_owned::<#inner>(resolver)
                    })
                }
            }
            FieldKind::Owned => {
                if let Some(k) = key {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_keyed_owned::<#inner>(
                            resolver,
                            #k,
                            #consumer_ty,
                            #field_name,
                        )?
                    })
                } else {
                    Ok(quote! {
                        #fnm: ::rust_dix::macros_support::resolve_owned::<#inner>(
                            resolver,
                            #consumer_ty,
                            #field_name,
                        )?
                    })
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
    pub key: Option<syn::Expr>,
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
                // Accept any expression convertible to `ServiceKey`:
                // string literals, typed key expressions, or downstream key
                // values implementing `From<LocalKey> for ServiceKey`.
                a.key = Some(m.value()?.parse::<syn::Expr>()?);
            }
            Ok(())
        })
        .ok();
    }
    a
}
