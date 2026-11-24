//! 0.10 typed-key / expression-key macro tests.
//!
//! Covers `#[inject(key = <expr>)]` for Arc / Option<Arc> / owned fields,
//! downstream key types via `impl From<LocalKey> for ServiceKey`, the
//! `register!(keyed <expr>: ...)` module syntax, AnyKey fallback resolution and
//! build-time validation with typed keys.

use rust_dix::*;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum Kind {
    A,
    B,
}

#[derive(Debug, PartialEq)]
struct Svc(&'static str);

#[derive(Debug, PartialEq)]
struct OwnedSvc(&'static str);

#[derive(Inject)]
struct TypedArcConsumer {
    #[inject(key = ServiceKey::new(Kind::A))]
    a: Arc<Svc>,
    #[inject(key = "main")]
    legacy: Arc<Svc>,
    #[inject(key = ServiceKey::new(Kind::B))]
    missing: Option<Arc<Svc>>,
}

#[test]
fn typed_and_string_expression_keys_in_derive() {
    let provider = ServiceCollection::new()
        .keyed_singleton(ServiceKey::new(Kind::A), |_| Arc::new(Svc("typed")))
        .keyed_singleton("main", |_| Arc::new(Svc("string")))
        .transient(|r| __rdi_construct_TypedArcConsumer(r))
        .build()
        .unwrap();

    let consumer: Arc<TypedArcConsumer> = provider.get().unwrap();
    assert_eq!(consumer.a.0, "typed");
    assert_eq!(consumer.legacy.0, "string");
    assert!(consumer.missing.is_none());
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct EfKey(&'static str);

impl From<EfKey> for ServiceKey {
    fn from(key: EfKey) -> Self {
        ServiceKey::new(key)
    }
}

#[derive(Inject)]
struct DownstreamKeyConsumer {
    #[inject(key = EfKey("main"))]
    svc: Arc<Svc>,
}

#[test]
fn downstream_key_type_via_from_impl() {
    // This is the official extension path for downstream frameworks: implement
    // `From<LocalKey> for ServiceKey` instead of duplicating injection macros.
    let provider = ServiceCollection::new()
        .keyed_singleton(ServiceKey::new(EfKey("main")), |_| Arc::new(Svc("ef")))
        .transient(|r| __rdi_construct_DownstreamKeyConsumer(r))
        .build()
        .unwrap();

    let consumer: Arc<DownstreamKeyConsumer> = provider.get().unwrap();
    assert_eq!(consumer.svc.0, "ef");
}

#[derive(Inject)]
struct TypedOwnedConsumer {
    #[inject(owned, key = ServiceKey::new(Kind::A))]
    svc: OwnedSvc,
}

#[test]
fn typed_keyed_owned_field() {
    let provider = ServiceCollection::new()
        .keyed_transient(ServiceKey::new(Kind::A), |_| Arc::new(OwnedSvc("owned")))
        .transient(|r| __rdi_construct_TypedOwnedConsumer(r))
        .build()
        .unwrap();

    let consumer: Arc<TypedOwnedConsumer> = provider.get().unwrap();
    assert_eq!(consumer.svc.0, "owned");
}

#[test]
fn typed_keyed_any_fallback_in_inject() {
    #[derive(Inject)]
    struct AnyConsumer {
        #[inject(key = ServiceKey::new(Kind::B))]
        svc: Arc<Svc>,
    }

    let provider = ServiceCollection::new()
        .keyed_any_singleton::<Svc>(|_, _| Arc::new(Svc("fallback")))
        .transient(|r| __rdi_construct_AnyConsumer(r))
        .build()
        .unwrap();
    let consumer: Arc<AnyConsumer> = provider.get().unwrap();
    assert_eq!(consumer.svc.0, "fallback");
}

#[test]
fn typed_keyed_scoped_dependency_is_captive_for_singleton() {
    #[derive(Inject)]
    struct CaptiveConsumer {
        #[inject(key = ServiceKey::new(Kind::A))]
        #[allow(dead_code)]
        svc: Arc<Svc>,
    }

    let result = ServiceCollection::new()
        .keyed_scoped(ServiceKey::new(Kind::A), |_| Arc::new(Svc("scoped")))
        .singleton(|r| __rdi_construct_CaptiveConsumer(r))
        .build();
    match result {
        Err(RdiError::SingletonDependsOnScoped { .. }) => {}
        Ok(_) => panic!("typed-key scoped dependency must be captive"),
        Err(other) => panic!("expected SingletonDependsOnScoped, got {other:?}"),
    }
}

// ── module macro: typed key expression in `register!` ──

#[derive(Default)]
struct ModuleJob;

#[rust_dix::module]
mod typed_key_module {
    rust_dix::register!(keyed rust_dix::ServiceKey::new(super::Kind::A): singleton: super::ModuleJob);
}

#[test]
fn module_macro_accepts_typed_key_expression() {
    let p = typed_key_module::__rdi_build_provider_typed_key_module().unwrap();
    let job: Arc<ModuleJob> = p.get_keyed(ServiceKey::new(Kind::A)).unwrap();
    let _ = job;
}

// Keyed trait-object registration with an explicit implementation type
// (`keyed <key>: <lifetime>: dyn Trait => Concrete`) must not silently drop
// the `=> Concrete` part.
trait ModulePlugin: Send + Sync {
    fn name(&self) -> &'static str;
}

#[derive(Default)]
struct ModulePluginImpl;

impl ModulePlugin for ModulePluginImpl {
    fn name(&self) -> &'static str {
        "plugin"
    }
}

#[rust_dix::module]
mod typed_trait_module {
    rust_dix::register!(keyed rust_dix::ServiceKey::new(super::Kind::B): singleton: dyn super::ModulePlugin => super::ModulePluginImpl);
}

#[test]
fn module_macro_keyed_trait_impl_registration() {
    let p = typed_trait_module::__rdi_build_provider_typed_trait_module().unwrap();
    let plugin: Arc<dyn ModulePlugin> = p.get_keyed(ServiceKey::new(Kind::B)).unwrap();
    assert_eq!(plugin.name(), "plugin");
}

#[rust_dix::module]
mod legacy_key_module {
    rust_dix::register!(keyed "legacy": transient: super::ModuleJob);
}

#[test]
fn module_macro_keeps_string_literal_keys() {
    let p = legacy_key_module::__rdi_build_provider_legacy_key_module().unwrap();
    let job: Arc<ModuleJob> = p.get_keyed("legacy").unwrap();
    let _ = job;
}
