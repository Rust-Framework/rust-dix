//! rust-di procedural macros.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    DeriveInput, Field, Fields, ItemImpl, ItemMod, Token,
};

// ───────────────────────────────────────────────────────────────────────────
// #[derive(Inject)] — auto-generate constructor from struct fields
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro_derive(Inject, attributes(rdi))]
pub fn inject_derive(input: TokenStream) -> TokenStream {
    expand_inject_derive(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_inject_derive(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let fn_name = format_ident!("__rdi_construct_{}", name);
    let constructor_body = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(named) => {
                let mut inits = Vec::new();
                for field in named.named.iter() {
                    inits.push(gen_field_init(field));
                }
                quote! { #name { #(#inits),* } }
            }
            Fields::Unit => quote! { #name },
            _ => return Err(syn::Error::new_spanned(name, "named fields or unit struct")),
        },
        _ => return Err(syn::Error::new_spanned(name, "struct only")),
    };
    Ok(
        quote! {#[doc(hidden)]pub fn #fn_name(resolver:&dyn rust_dicore::IServiceResolver)->::std::sync::Arc<#name>{::std::sync::Arc::new(#constructor_body)}},
    )
}

/// Classify a field's type for injection strategy.
///
/// - `Arc`        → `Arc<T>`           : shared, resolve via `get_any` + downcast.
/// - `OptionArc`  → `Option<Arc<T>>`   : optional shared, returns `Option<Arc<T>>`.
/// - `Owned`      → bare `T`           : owned, resolve via `get_owned_any` + `try_unwrap`.
/// - `OptionOwned`→ `Option<T>`        : optional owned, returns `Option<T>`.
#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    Arc,
    OptionArc,
    Owned,
    OptionOwned,
}

fn classify_field(ty: &syn::Type) -> (proc_macro2::TokenStream, FieldKind) {
    if let syn::Type::Path(p) = ty {
        if let Some(last) = p.path.segments.last() {
            match last.ident.to_string().as_str() {
                "Arc" => {
                    if let Some(inner) = single_generic_type(last) {
                        return (quote! {#inner}, FieldKind::Arc);
                    }
                }
                "Option" => {
                    if let Some(inner_ty) = single_generic_type(last) {
                        if let syn::Type::Path(ip) = inner_ty {
                            if let Some(ilast) = ip.path.segments.last() {
                                if ilast.ident == "Arc" {
                                    if let Some(t) = single_generic_type(ilast) {
                                        return (quote! {#t}, FieldKind::OptionArc);
                                    }
                                } else {
                                    // Option<T> where T is not Arc → OptionOwned
                                    return (quote! {#inner_ty}, FieldKind::OptionOwned);
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

fn single_generic_type(seg: &syn::PathSegment) -> Option<&syn::Type> {
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
fn gen_field_init(field: &Field) -> proc_macro2::TokenStream {
    let attrs = parse_ia(field);
    let fnm = field.ident.as_ref().unwrap();
    let (inner, kind) = classify_field(&field.ty);

    if attrs.skip {
        return quote! { #fnm: ::std::default::Default::default() };
    }
    if attrs.provider {
        return quote! { #fnm: resolver.clone() };
    }

    // Arc<T> extraction: downcast Arc<dyn Any> → Arc<Arc<T>> → clone inner Arc<T>.
    let arc_extract = quote! {
        .and_then(|a| a.downcast::<::std::sync::Arc<#inner>>().ok())
        .map(|d| ::std::sync::Arc::clone(&*d))
    };
    // Owned extraction: Arc<Arc<T>> → Arc<T> → try_unwrap → T.
    let owned_extract = quote! {
        .and_then(|a| a.downcast::<::std::sync::Arc<#inner>>().ok())
        .map(|d| ::std::sync::Arc::clone(&*d))
        .and_then(|arc| ::std::sync::Arc::try_unwrap(arc).ok())
    };

    let key = &attrs.key;
    match kind {
        // Option<Arc<T>>: field type already Option, always Option-returning path.
        FieldKind::OptionArc => {
            if let Some(k) = key {
                quote! { #fnm: resolver.get_keyed_any(::std::any::type_name::<#inner>(), #k) #arc_extract }
            } else {
                quote! { #fnm: resolver.get_any(::std::any::type_name::<#inner>()) #arc_extract }
            }
        }
        // Option<T>: field type already Option, always Option-returning owned path.
        FieldKind::OptionOwned => {
            if let Some(k) = key {
                quote! { #fnm: resolver.get_keyed_owned_any(::std::any::type_name::<#inner>(), #k) #owned_extract }
            } else {
                quote! { #fnm: resolver.get_owned_any(::std::any::type_name::<#inner>()) #owned_extract }
            }
        }
        // Arc<T>: attr-first (preserve existing behavior, incl. #[rdi(optional)]).
        FieldKind::Arc => {
            if attrs.optional {
                if let Some(k) = key {
                    quote! { #fnm: resolver.get_keyed_any(::std::any::type_name::<#inner>(), #k) #arc_extract }
                } else {
                    quote! { #fnm: resolver.get_any(::std::any::type_name::<#inner>()) #arc_extract }
                }
            } else if let Some(k) = key {
                quote! { #fnm: resolver.get_keyed_any(::std::any::type_name::<#inner>(), #k) #arc_extract .unwrap_or_else(|| ::std::panic!("keyed not found")) }
            } else {
                quote! { #fnm: resolver.get_any(::std::any::type_name::<#inner>()) #arc_extract .unwrap_or_else(|| ::std::panic!("svc not registered")) }
            }
        }
        // bare T: attr-first owned path (new).
        FieldKind::Owned => {
            if attrs.optional {
                if let Some(k) = key {
                    quote! { #fnm: resolver.get_keyed_owned_any(::std::any::type_name::<#inner>(), #k) #owned_extract }
                } else {
                    quote! { #fnm: resolver.get_owned_any(::std::any::type_name::<#inner>()) #owned_extract }
                }
            } else if let Some(k) = key {
                quote! { #fnm: resolver.get_keyed_owned_any(::std::any::type_name::<#inner>(), #k) #owned_extract .unwrap_or_else(|| ::std::panic!("keyed owned not found")) }
            } else {
                quote! { #fnm: resolver.get_owned_any(::std::any::type_name::<#inner>()) #owned_extract .unwrap_or_else(|| ::std::panic!("owned svc not registered or Singleton")) }
            }
        }
    }
}

#[derive(Default)]
struct IA {
    skip: bool,
    optional: bool,
    provider: bool,
    key: Option<String>,
}
fn parse_ia(f: &Field) -> IA {
    let mut a = IA::default();
    for attr in &f.attrs {
        if !attr.path().is_ident("rdi") {
            continue;
        }
        let Ok(l) = attr.meta.require_list() else {
            continue;
        };
        l.parse_nested_meta(|m| {
            if m.path.is_ident("skip") {
                a.skip = true;
            } else if m.path.is_ident("optional") {
                a.optional = true;
            } else if m.path.is_ident("provider") {
                a.provider = true;
            } else if m.path.is_ident("key") {
                a.key = Some(m.value()?.parse::<syn::LitStr>()?.value());
            }
            Ok(())
        })
        .ok();
    }
    a
}

// ───────────────────────────────────────────────────────────────────────────
// register!() — function-like placeholder macro for use inside #[module]
// ───────────────────────────────────────────────────────────────────────────

/// Function-like macro consumed by `#[rust_dicore::module]`.
///
/// Expands to nothing on its own; the `module` attribute macro detects
/// `register!(...)` calls at compile time and turns them into service
/// registrations.
#[proc_macro]
pub fn register(_: TokenStream) -> TokenStream {
    quote! {}.into()
}

// ───────────────────────────────────────────────────────────────────────────
// #[module] — compile-time module scanning for register!() declarations
// ───────────────────────────────────────────────────────────────────────────

#[proc_macro_attribute]
pub fn module(_: TokenStream, item: TokenStream) -> TokenStream {
    expand_md(parse_macro_input!(item as ItemMod))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_md(mut m: ItemMod) -> syn::Result<proc_macro2::TokenStream> {
    let mn = m.ident.clone();
    let fn_n = format_ident!("__rdi_build_provider_{}", mn);
    let is = match &m.content {
        Some((_, i)) => i.clone(),
        None => return Err(syn::Error::new_spanned(m, "body required")),
    };
    let mut rs = Vec::new();
    let mut cl = Vec::new();
    for i in &is {
        match i {
            syn::Item::Macro(mc) => {
                let ps = mc
                    .mac
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                if ps == "register" || ps == "rust_dicore::register" {
                    if let Ok(r) = syn::parse2::<ID>(mc.mac.tokens.clone()) {
                        rs.push(r);
                    }
                } else {
                    cl.push(i.clone());
                }
            }
            _ => cl.push(i.clone()),
        }
    }
    let mut ch = Vec::new();
    for r in &rs {
        match &r.kind {
            IK::N { lt, ty, imp } => {
                let mt = lmt(*lt);
                if let Some(imp_ty) = imp {
                    ch.push(quote!{ .#mt::<#ty>(|_: &dyn rust_dicore::IServiceResolver| ::std::sync::Arc::new(<#imp_ty as ::std::default::Default>::default())) });
                } else {
                    ch.push(quote!{ .#mt(|_: &dyn rust_dicore::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
                }
            }
            IK::K { key, lt, ty } => {
                let mt = kmt(*lt);
                ch.push(quote!{ .#mt::<#ty>(#key,|_:&dyn rust_dicore::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
            }
            IK::F { lt, f } => {
                let mt = lmt(*lt);
                ch.push(
                    quote! { .#mt(move |_: &dyn rust_dicore::IServiceResolver| ::std::sync::Arc::new(#f)) },
                );
            }
        }
    }
    vd(&rs)?;
    let bi: syn::Item = syn::parse2(quote! {
        #[doc(hidden)]
        pub fn #fn_n() -> ::std::result::Result<::std::sync::Arc<rust_dicore::ServiceProvider>, rust_dicore::RdiError> {
            Ok(::std::sync::Arc::new(rust_dicore::ServiceCollection::new() #(#ch)* .build()?))
        }
    })
    .unwrap();
    cl.push(bi);
    m.content = Some((syn::token::Brace::default(), cl));
    Ok(quote! {#m})
}
fn lmt(lt: LT) -> proc_macro2::TokenStream {
    match lt {
        LT::S => quote! {singleton},
        LT::Sc => quote! {scoped},
        LT::T => quote! {transient},
    }
}
fn kmt(lt: LT) -> proc_macro2::TokenStream {
    match lt {
        LT::S => quote! {keyed_singleton},
        LT::Sc => quote! {keyed_scoped},
        LT::T => quote! {keyed_transient},
    }
}
fn vd(rs: &[ID]) -> syn::Result<()> {
    let mut sn: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in rs {
        if let IK::K { key, .. } = &r.kind {
            let e = sn.entry(key.clone()).or_default();
            *e += 1;
            if *e > 1 {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("rdi-E004: duplicate key `{key}`"),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LT {
    S,
    Sc,
    T,
}
impl Parse for LT {
    fn parse(i: ParseStream) -> syn::Result<Self> {
        match i.parse::<syn::Ident>()?.to_string().as_str() {
            "singleton" => Ok(LT::S),
            "scoped" => Ok(LT::Sc),
            "transient" => Ok(LT::T),
            o => Err(syn::Error::new(i.span(), format!("unknown lifetime: {o}"))),
        }
    }
}
#[derive(Debug)]
enum IK {
    N {
        lt: LT,
        ty: syn::Type,
        imp: Option<syn::Type>,
    },
    K {
        key: String,
        lt: LT,
        ty: syn::Type,
    },
    F {
        lt: LT,
        f: syn::Expr,
    },
}
#[derive(Debug)]
struct ID {
    kind: IK,
}
impl Parse for ID {
    fn parse(i: ParseStream) -> syn::Result<Self> {
        let mk = if i.peek(syn::Ident) && i.fork().parse::<syn::Ident>()? == "keyed" {
            let _: syn::Ident = i.parse()?;
            let k: syn::LitStr = i.parse()?;
            let _: Token![:] = i.parse()?;
            let lt: LT = i.parse()?;
            Some((k.value(), lt))
        } else {
            None
        };
        if i.peek(syn::Ident) && i.fork().parse::<syn::Ident>()? == "factory" {
            let _: syn::Ident = i.parse()?;
            let lt: LT = i.parse()?;
            let _: Token![:] = i.parse()?;
            let _: syn::Type = i.parse()?;
            let _: Token![=>] = i.parse()?;
            let f: syn::Expr = i.parse()?;
            return Ok(ID {
                kind: IK::F { lt, f },
            });
        }
        if let Some((k, l)) = mk {
            let _: Token![:] = i.parse()?;
            let ty: syn::Type = i.parse()?;
            let _ = i.parse::<Token![=>]>();
            if !i.is_empty() && !i.peek(Token![|]) {
                let _: syn::Type = i.parse()?;
            }
            return Ok(ID {
                kind: IK::K { key: k, lt: l, ty },
            });
        }
        let lt: LT = i.parse()?;
        let _: Token![:] = i.parse()?;
        let ty: syn::Type = i.parse()?;
        let _ = i.parse::<Token![=>]>();
        let imp: Option<syn::Type> = if !i.is_empty() && !i.peek(Token![|]) {
            Some(i.parse::<syn::Type>()?)
        } else {
            None
        };
        Ok(ID {
            kind: IK::N { lt, ty, imp },
        })
    }
}

// ───────────────────────────────────────────────────────────────────────────
// #[inject] — attribute macro for auto-registration
//
// Supports two placement sites:
//   1. On a struct:  generates constructor + registers as concrete type
//   2. On a trait impl:  registers as the trait type (auto-detected from
//      `impl Trait for Type`), reusing the constructor generated by
//      `#[derive(Inject)]` or `#[inject]` on the struct.
//
// Syntax:  #[inject]  or  #[inject(scoped)]  or  #[inject(transient)]
// Lifetime defaults to `singleton` when omitted.
// ───────────────────────────────────────────────────────────────────────────

struct InjectArgs {
    lifetime: Option<LT>,
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

#[proc_macro_attribute]
pub fn inject(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_inject(
        parse_macro_input!(attr as InjectArgs),
        parse_macro_input!(item as syn::Item),
    )
    .unwrap_or_else(|e| e.to_compile_error())
    .into()
}

fn expand_inject(args: InjectArgs, item: syn::Item) -> syn::Result<proc_macro2::TokenStream> {
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

/// `#[inject]` on a struct: generate constructor + register as concrete type.
fn expand_inject_struct(
    struct_item: &syn::ItemStruct,
    lifetime: LT,
) -> syn::Result<proc_macro2::TokenStream> {
    let name = &struct_item.ident;
    let fn_name = format_ident!("__rdi_construct_{}", name);
    let factory_name = format_ident!("__rdi_factory_{}", name);
    let type_name_fn_name = format_ident!("__rdi_type_name_{}", name);

    let constructor_body = match &struct_item.fields {
        syn::Fields::Named(n) => {
            let mut inits = Vec::new();
            for field in n.named.iter() {
                inits.push(gen_field_init(field));
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
        pub fn #fn_name(resolver: &dyn rust_dicore::IServiceResolver) -> ::std::sync::Arc<#name> {
            ::std::sync::Arc::new(#constructor_body)
        }
    };

    let lt_ident = lt_to_token(lifetime);

    let factory = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #factory_name(resolver: &dyn rust_dicore::IServiceResolver) -> ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync> {
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
        rust_dicore::inventory::submit! {
            rust_dicore::ServiceRegistration {
                lifetime: #lt_ident,
                type_id: ::std::any::TypeId::of::<#name>(),
                type_name_fn: #type_name_fn_name,
                factory: #factory_name,
            }
        }
    };

    // Strip #[rdi(...)] helper attributes from fields before emitting the
    // struct. Without #[derive(Inject)] on the struct, the compiler doesn't
    // know `rdi` is a valid helper attribute and would reject it.
    let mut stripped = struct_item.clone();
    if let syn::Fields::Named(n) = &mut stripped.fields {
        for field in n.named.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("rdi"));
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

/// `#[inject]` on a trait impl: register as the trait type.
///
/// Extracts `Trait` and `Type` from `impl Trait for Type`, then generates a
/// factory that calls `__rdi_construct_{Type}` (which must be provided by
/// `#[derive(Inject)]` or `#[inject]` on the struct) and upcasts to
/// `dyn Trait`.
fn expand_inject_impl(
    impl_item: &ItemImpl,
    lifetime: LT,
) -> syn::Result<proc_macro2::TokenStream> {
    // Must be a trait impl: `impl Trait for Type`
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

    // Extract the implementing type's identifier for function naming
    let impl_ty = &impl_item.self_ty;
    let type_ident = match &**impl_ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.clone()),
        _ => None,
    }
    .ok_or_else(|| syn::Error::new_spanned(impl_ty, "expected a path type"))?;

    // Build a unique suffix from the trait's last segment to avoid collisions
    // when the same struct implements multiple traits.
    let trait_segment = trait_path.segments.last().unwrap();
    let trait_suffix = &trait_segment.ident;

    let constructor_fn = format_ident!("__rdi_construct_{}", type_ident);
    let factory_name = format_ident!("__rdi_factory_{}_for_{}", type_ident, trait_suffix);
    let type_name_fn_name =
        format_ident!("__rdi_type_name_{}_for_{}", type_ident, trait_suffix);

    let lt_ident = lt_to_token(lifetime);

    // `dyn Trait` — register key
    let trait_ty = quote! { dyn #trait_path };

    let factory = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #factory_name(resolver: &dyn rust_dicore::IServiceResolver) -> ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync> {
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
        rust_dicore::inventory::submit! {
            rust_dicore::ServiceRegistration {
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

fn lt_to_token(lt: LT) -> proc_macro2::TokenStream {
    match lt {
        LT::S => quote! { rust_dicore::ServiceLifetime::Singleton },
        LT::Sc => quote! { rust_dicore::ServiceLifetime::Scoped },
        LT::T => quote! { rust_dicore::ServiceLifetime::Transient },
    }
}
