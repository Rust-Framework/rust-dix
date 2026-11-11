//! Documentation smoke tests.
//!
//! These mirror the canonical examples in the crate README / crate rustdoc so a
//! documentation drift that breaks a "copy-paste" example fails CI. Keep them in
//! sync with the docs (only stable, documented API).

use rust_dix::*;
use std::sync::Arc;

// ── README quick start: attribute auto-registration ──

#[rust_dix::inject(singleton)]
struct Config;

#[rust_dix::inject(transient)]
struct Worker {
    #[inject]
    config: Arc<Config>,
}

#[test]
fn readme_quick_start_compiles_and_resolves() {
    let provider = ServiceCollection::from_injected().build().unwrap();
    let worker: Arc<Worker> = provider.get().unwrap();
    let _ = &worker.config;
}

// ── #[derive(Inject)] with owned (bare T) + shared (Arc<T>) fields ──

#[derive(Inject)]
struct Mixed {
    #[inject(owned)]
    owned: OwnedService,
    #[inject]
    shared: Arc<SharedService>,
}

struct OwnedService;
struct SharedService;

#[test]
fn derive_inject_owned_and_shared_fields_compile() {
    let provider = ServiceCollection::new()
        .transient(|_| Arc::new(OwnedService))
        .singleton(|_| Arc::new(SharedService))
        .transient(|r| __rdi_construct_Mixed(r))
        .build()
        .unwrap();
    let mixed: Arc<Mixed> = provider.get().unwrap();
    let _ = (&mixed.owned, &mixed.shared);
}

// ── Typed keys + AnyKey fallback (canonical rustdoc example) ──

#[derive(Debug, PartialEq, Eq, Hash)]
enum Kind {
    Fast,
}

trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
}

struct Fast;
impl Strategy for Fast {
    fn name(&self) -> &'static str {
        "fast"
    }
}

struct Fallback;
impl Strategy for Fallback {
    fn name(&self) -> &'static str {
        "fallback"
    }
}

#[test]
fn typed_keys_and_any_key_fallback() {
    let provider = ServiceCollection::new()
        .keyed_singleton::<dyn Strategy>(ServiceKey::new(Kind::Fast), |_| Arc::new(Fast))
        .keyed_any_singleton::<dyn Strategy>(|_, _| Arc::new(Fallback))
        .build()
        .unwrap();

    let fast = provider
        .get_keyed::<dyn Strategy>(ServiceKey::new(Kind::Fast))
        .unwrap();
    assert_eq!(fast.name(), "fast");
    let fallback = provider.get_keyed::<dyn Strategy>("unknown").unwrap();
    assert_eq!(fallback.name(), "fallback");
}

// ── Typed keys in injection macros (0.10 canonical example) ──

#[derive(Debug, PartialEq, Eq, Hash)]
enum Channel {
    Primary,
}

#[derive(Inject)]
struct TypedKeyConsumer {
    #[inject(key = ServiceKey::new(Channel::Primary))]
    #[allow(dead_code)]
    svc: Arc<SharedService>,
}

#[test]
fn typed_key_expression_in_inject_macro() {
    let provider = ServiceCollection::new()
        .keyed_singleton(ServiceKey::new(Channel::Primary), |_| {
            Arc::new(SharedService)
        })
        .transient(|r| __rdi_construct_TypedKeyConsumer(r))
        .build()
        .unwrap();
    let consumer: Arc<TypedKeyConsumer> = provider.get().unwrap();
    let _ = &consumer.svc;
}

// ── Async resolution (build_async + get_async) ──

struct DbPool {
    url: String,
}

#[test]
fn async_quick_start_compiles() {
    let provider = futures::executor::block_on(
        ServiceCollection::new()
            .async_singleton(|_| {
                Box::pin(async {
                    Arc::new(DbPool {
                        url: "sqlite::memory:".into(),
                    })
                })
            })
            .build_async(),
    )
    .unwrap();
    let pool: Arc<DbPool> = futures::executor::block_on(provider.get_async()).unwrap();
    assert_eq!(pool.url, "sqlite::memory:");
}

// Silence unused-import warnings for macro-only imports.
