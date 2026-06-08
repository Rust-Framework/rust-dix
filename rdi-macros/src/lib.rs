//! RDI procedural macros.
#![allow(unsafe_code)]
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, DeriveInput, Field, Fields, ItemMod, Token,
};

#[proc_macro_derive(Inject, attributes(inject))]
pub fn inject_derive(input: TokenStream) -> TokenStream {
    expand_inject(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_inject(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let named = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(n) => n,
            _ => return Err(syn::Error::new_spanned(name, "named fields")),
        },
        _ => return Err(syn::Error::new_spanned(name, "struct only")),
    };
    let fn_name = format_ident!("__rdi_construct_{}", name);
    let mut inits = Vec::new();
    for field in named.named.iter() {
        let attrs = parse_ia(field);
        let fnm = field.ident.as_ref().unwrap();
        let (inner, _) = saw(&field.ty);
        let init = if attrs.skip {
            quote! {#fnm:Default::default()}
        } else if attrs.provider {
            quote! {#fnm:resolver.clone()}
        } else if attrs.optional {
            if let Some(k) = &attrs.key {
                quote! {#fnm:resolver.get_keyed_any(::std::any::type_name::<#inner>(),#k).and_then(|a|a.downcast::<::std::sync::Arc<#inner>>().ok()).map(|d|::std::sync::Arc::clone(&*d))}
            } else {
                quote! {#fnm:resolver.get_any(::std::any::type_name::<#inner>()).and_then(|a|a.downcast::<::std::sync::Arc<#inner>>().ok()).map(|d|::std::sync::Arc::clone(&*d))}
            }
        } else if let Some(k) = &attrs.key {
            quote! {#fnm:resolver.get_keyed_any(::std::any::type_name::<#inner>(),#k).and_then(|a|a.downcast::<::std::sync::Arc<#inner>>().ok()).map(|d|::std::sync::Arc::clone(&*d)).unwrap_or_else(||::std::panic!("keyed not found"))}
        } else {
            quote! {#fnm:resolver.get_any(::std::any::type_name::<#inner>()).and_then(|a|a.downcast::<::std::sync::Arc<#inner>>().ok()).map(|d|::std::sync::Arc::clone(&*d)).unwrap_or_else(||::std::panic!("svc not registered"))}
        };
        inits.push(init);
    }
    Ok(
        quote! {#[doc(hidden)]pub fn #fn_name(resolver:&dyn rdi::IServiceResolver)->::std::sync::Arc<#name>{::std::sync::Arc::new(#name{#(#inits),*})}},
    )
}

fn saw(ty: &syn::Type) -> (proc_macro2::TokenStream, bool) {
    if let syn::Type::Path(p) = ty {
        let l = p.path.segments.last().unwrap();
        if l.ident == "Arc" {
            if let syn::PathArguments::AngleBracketed(a) = &l.arguments {
                if let Some(syn::GenericArgument::Type(i)) = a.args.first() {
                    return (quote! {#i}, true);
                }
            }
        }
        if l.ident == "Option" {
            if let syn::PathArguments::AngleBracketed(a) = &l.arguments {
                if let Some(syn::GenericArgument::Type(i)) = a.args.first() {
                    if let syn::Type::Path(ip) = i {
                        if ip
                            .path
                            .segments
                            .last()
                            .map(|s| s.ident == "Arc")
                            .unwrap_or(false)
                        {
                            if let syn::PathArguments::AngleBracketed(ia) =
                                &ip.path.segments.last().unwrap().arguments
                            {
                                if let Some(syn::GenericArgument::Type(t)) = ia.args.first() {
                                    return (quote! {#t}, true);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (quote! {#ty}, false)
}

struct IA {
    skip: bool,
    optional: bool,
    provider: bool,
    key: Option<String>,
}
impl Default for IA {
    fn default() -> Self {
        Self {
            skip: false,
            optional: false,
            provider: false,
            key: None,
        }
    }
}
fn parse_ia(f: &Field) -> IA {
    let mut a = IA::default();
    for attr in &f.attrs {
        if !attr.path().is_ident("inject") {
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

#[proc_macro]
pub fn inject(_: TokenStream) -> TokenStream {
    quote! {}.into()
}

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
                if ps == "inject" || ps == "rdi::inject" {
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
                    ch.push(quote!{ .#mt::<#ty>(|_: &dyn rdi::IServiceResolver| ::std::sync::Arc::new(<#imp_ty as ::std::default::Default>::default())) });
                } else {
                    ch.push(quote!{ .#mt(|_: &dyn rdi::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
                }
            }
            IK::K { key, lt, ty } => {
                let mt = kmt(*lt);
                ch.push(quote!{ .#mt::<#ty>(#key,|_:&dyn rdi::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
            }
            IK::F { lt, f } => {
                let mt = lmt(*lt);
                ch.push(
                    quote! { .#mt(move |_: &dyn rdi::IServiceResolver| ::std::sync::Arc::new(#f)) },
                );
            }
        }
    }
    vd(&rs)?;
    let bi: syn::Item = syn::parse2(quote! {
        #[doc(hidden)]
        pub fn #fn_n() -> ::std::result::Result<::std::sync::Arc<rdi::ServiceProvider>, rdi::RdiError> {
            Ok(::std::sync::Arc::new(rdi::ServiceCollection::new() #(#ch)* .build()?))
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
        LT::S => quote! {keyed},
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
