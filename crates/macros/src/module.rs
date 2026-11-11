use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    ItemMod, Token,
};

// ── LT enum ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LT {
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

// ── #[module] expansion ──

pub(crate) fn expand_md(mut m: ItemMod) -> syn::Result<TokenStream2> {
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
                if ps == "register" || ps == "rust_dix::register" {
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
                    ch.push(quote!{ .#mt::<#ty>(|_: &dyn rust_dix::IServiceResolver| ::std::sync::Arc::new(<#imp_ty as ::std::default::Default>::default())) });
                } else {
                    ch.push(quote!{ .#mt(|_: &dyn rust_dix::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
                }
            }
            IK::K { key, lt, ty, imp } => {
                let mt = kmt(*lt);
                if let Some(imp_ty) = imp {
                    ch.push(quote!{ .#mt::<#ty>(#key,|_:&dyn rust_dix::IServiceResolver| ::std::sync::Arc::new(<#imp_ty as ::std::default::Default>::default())) });
                } else {
                    ch.push(quote!{ .#mt::<#ty>(#key,|_:&dyn rust_dix::IServiceResolver| ::std::sync::Arc::new(<#ty as ::std::default::Default>::default())) });
                }
            }
            IK::F { lt, f } => {
                let mt = lmt(*lt);
                ch.push(
                    quote! { .#mt(move |_: &dyn rust_dix::IServiceResolver| ::std::sync::Arc::new(#f)) },
                );
            }
        }
    }
    vd(&rs)?;
    let bi: syn::Item = syn::parse2(quote! {
        #[doc(hidden)]
        pub fn #fn_n() -> ::std::result::Result<::std::sync::Arc<rust_dix::ServiceProvider>, rust_dix::RdiError> {
            rust_dix::ServiceCollection::new() #(#ch)* .build()
        }
    })
    .unwrap();
    cl.push(bi);
    m.content = Some((syn::token::Brace::default(), cl));
    Ok(quote! {#m})
}

fn lmt(lt: LT) -> TokenStream2 {
    match lt {
        LT::S => quote! {singleton},
        LT::Sc => quote! {scoped},
        LT::T => quote! {transient},
    }
}

fn kmt(lt: LT) -> TokenStream2 {
    match lt {
        LT::S => quote! {keyed_singleton},
        LT::Sc => quote! {keyed_scoped},
        LT::T => quote! {keyed_transient},
    }
}

fn vd(rs: &[ID]) -> syn::Result<()> {
    // Compile-time duplicate detection is only possible for string-literal keys
    // (expressions may be dynamic); runtime last-registration-wins semantics
    // still apply for same-key duplicates.
    let mut sn: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in rs {
        if let IK::K { key, .. } = &r.kind {
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            }) = key
            else {
                continue;
            };
            let e = sn.entry(lit.value()).or_default();
            *e += 1;
            if *e > 1 {
                return Err(syn::Error::new(
                    lit.span(),
                    format!("rdi-E004: duplicate key `{}`", lit.value()),
                ));
            }
        }
    }
    Ok(())
}

// ── register!() parse types ──

#[derive(Debug)]
enum IK {
    N {
        lt: LT,
        ty: syn::Type,
        imp: Option<syn::Type>,
    },
    K {
        /// Any expression convertible to `ServiceKey` (string literal, typed
        /// key expression, or downstream key value via `From`).
        key: syn::Expr,
        lt: LT,
        ty: syn::Type,
        /// Optional concrete implementation type for a trait-object registration
        /// (`keyed <key>: <lt>: dyn Trait => Concrete`).
        imp: Option<syn::Type>,
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
            let k: syn::Expr = i.parse()?;
            let _: Token![:] = i.parse()?;
            let lt: LT = i.parse()?;
            Some((k, lt))
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
            let imp: Option<syn::Type> = if !i.is_empty() && !i.peek(Token![|]) {
                Some(i.parse::<syn::Type>()?)
            } else {
                None
            };
            return Ok(ID {
                kind: IK::K {
                    key: k,
                    lt: l,
                    ty,
                    imp,
                },
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
