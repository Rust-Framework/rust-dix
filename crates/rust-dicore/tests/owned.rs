//! Tests for owned service resolution (`get_owned::<T>() -> T`).
//!
//! Covers: Transient/Scoped/Singleton owned semantics, scope dependency
//! binding, keyed owned, #[derive(Inject)] owned/Option/keyed fields,
//! mixed handler &mut self e2e, captive dependency rejection.

#![allow(non_snake_case)]
#![allow(dead_code)]

mod common;

use rust_dicore::*;
use rust_dicore_macros::Inject;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────
// Test 1: Transient get_owned returns fresh owned instances
// ─────────────────────────────────────────────────────────────────

#[test]
fn transient_get_owned_returns_fresh_owned() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 0 }))
        .build()
        .unwrap();

    let mut a = p.get_owned::<common::MyService>();
    let b = p.get_owned::<common::MyService>();

    // Two owned instances are independent: mutating one doesn't affect the other.
    a.value = 42;
    assert_eq!(b.value, 0, "second owned instance must be unaffected");
    assert_eq!(a.value, 42);
}

// ─────────────────────────────────────────────────────────────────
// Test 2: Scoped get_owned bypasses cache (fresh each call)
// ─────────────────────────────────────────────────────────────────

#[test]
fn scoped_get_owned_bypasses_cache() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 0 }))
        .build()
        .unwrap();

    // get() caches in root_scoped_cache.
    let a = p.get::<common::MyService>();
    let b = p.get::<common::MyService>();
    assert!(Arc::ptr_eq(&a, &b), "get() must cache scoped in root");

    // get_owned() bypasses cache → fresh each call.
    let mut o1 = p.get_owned::<common::MyService>();
    let o2 = p.get_owned::<common::MyService>();
    o1.value = 99;
    assert_eq!(o1.value, 99);
    assert_eq!(o2.value, 0, "get_owned must bypass cache (fresh instances)");

    // get_owned did not pollute the cache: get() still returns the cached Arc.
    let c = p.get::<common::MyService>();
    assert!(Arc::ptr_eq(&a, &c), "get_owned must not pollute root_scoped_cache");
    assert_eq!(c.value, 0, "owned mutation must not leak into cached instance");
}

// ─────────────────────────────────────────────────────────────────
// Test 3 & 4: Singleton owned is rejected
// ─────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "get_owned: not registered or Singleton")]
fn singleton_get_owned_panics() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    let _ = p.get_owned::<common::MyService>();
}

#[test]
fn singleton_try_get_owned_returns_none() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    assert!(p.try_get_owned::<common::MyService>().is_none());
}

// ─────────────────────────────────────────────────────────────────
// Test 5: try_get_owned returns None when unregistered
// ─────────────────────────────────────────────────────────────────

#[test]
fn try_get_owned_none_when_unregistered() {
    let p = ServiceCollection::new().build().unwrap();
    assert!(p.try_get_owned::<common::MyService>().is_none());
}

// ─────────────────────────────────────────────────────────────────
// Test 6: Owned transient handler's Arc<Scoped> dep binds to child scope
//
// owned handler is fresh each get_owned, but its Arc<ScopedCore> field
// (Arc path → get_any) is cached in the child scope. Across scopes the
// ScopedCore is independent.
// ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ScopedCore(u64);

#[derive(Inject)]
struct OwnedHandler {
    core: Arc<ScopedCore>,
}

#[test]
fn scope_owned_transient_arc_scoped_dep_binds_to_child() {
    static CORE_CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = Arc::new(
        ServiceCollection::new()
            .scoped(|_| {
                let n = CORE_CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(ScopedCore(n))
            })
            .transient(|r| __rdi_construct_OwnedHandler(r))
            .build()
            .unwrap(),
    );

    let scope1 = provider.scope();
    let h1 = scope1.get_owned::<OwnedHandler>();
    let h2 = scope1.get_owned::<OwnedHandler>();

    // Handler itself is fresh (owned), but its Arc<ScopedCore> field is
    // cached in scope1 → both handlers share the same core.
    assert!(
        Arc::ptr_eq(&h1.core, &h2.core),
        "Arc<ScopedCore> must be cached in child scope"
    );

    let scope2 = provider.scope();
    let h3 = scope2.get_owned::<OwnedHandler>();
    assert!(
        !Arc::ptr_eq(&h1.core, &h3.core),
        "different scopes must have independent ScopedCore"
    );

    // ScopedCore factory ran once per scope (owned bypass does not trigger it).
    assert_eq!(
        CORE_CALLS.load(Ordering::SeqCst),
        2,
        "ScopedCore factory should run once per scope"
    );
}

// ─────────────────────────────────────────────────────────────────
// Test 7 & 8: Keyed owned
// ─────────────────────────────────────────────────────────────────

#[test]
fn keyed_transient_get_owned() {
    let p = ServiceCollection::new()
        .keyed_transient("primary", |_| Arc::new(common::MyService { value: 7 }))
        .build()
        .unwrap();
    let svc = p.get_keyed_owned::<common::MyService>("primary");
    assert_eq!(svc.value, 7);
}

#[test]
#[should_panic(expected = "get_keyed_owned: not registered or Singleton")]
fn keyed_singleton_get_owned_panics() {
    let p = ServiceCollection::new()
        .keyed_singleton("primary", |_| Arc::new(common::MyService { value: 7 }))
        .build()
        .unwrap();
    let _ = p.get_keyed_owned::<common::MyService>("primary");
}

// ─────────────────────────────────────────────────────────────────
// Test 9: #[derive(Inject)] with bare T (owned) + Arc<T> (shared) fields
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct MixedHandler {
    svc: common::MyService,
    logger: Arc<common::Logger>,
}

#[test]
fn derive_inject_owned_field() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 0 }))
        .singleton(|_| Arc::new(common::Logger { prefix: "log".into() }))
        .transient(|r| __rdi_construct_MixedHandler(r))
        .build()
        .unwrap();

    let mut h = p.get_owned::<MixedHandler>();
    // svc is owned (bare T) → mutable.
    h.svc.value = 123;
    assert_eq!(h.svc.value, 123, "owned field must be mutable");
    // logger is shared (Arc<T>).
    assert_eq!(h.logger.prefix, "log");
}

// ─────────────────────────────────────────────────────────────────
// Test 10: #[derive(Inject)] with Option<T> (owned optional) field
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct OptHandler {
    maybe: Option<common::MyService>,
}

#[test]
fn derive_inject_option_owned_field_present() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 5 }))
        .transient(|r| __rdi_construct_OptHandler(r))
        .build()
        .unwrap();
    let h = p.get_owned::<OptHandler>();
    assert!(h.maybe.is_some(), "registered dep → Some(owned)");
    assert_eq!(h.maybe.as_ref().unwrap().value, 5);
}

#[test]
fn derive_inject_option_owned_field_absent() {
    let p = ServiceCollection::new()
        .transient(|r| __rdi_construct_OptHandler(r))
        .build()
        .unwrap();
    let h = p.get_owned::<OptHandler>();
    assert!(h.maybe.is_none(), "unregistered dep → None (no panic)");
}

// ─────────────────────────────────────────────────────────────────
// Test 11: #[derive(Inject)] with keyed owned field
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct KeyedOwnedHandler {
    #[rdi(key = "primary")]
    svc: common::MyService,
}

#[test]
fn derive_inject_keyed_owned_field() {
    let p = ServiceCollection::new()
        .keyed_transient("primary", |_| Arc::new(common::MyService { value: 9 }))
        .transient(|r| __rdi_construct_KeyedOwnedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<KeyedOwnedHandler>();
    assert_eq!(h.svc.value, 9, "keyed owned field must resolve by key");
}

// ─────────────────────────────────────────────────────────────────
// Test 12: End-to-end &mut self (simulates rust-ef DbContext use case)
//
// No interior mutability, no unsafe: owned DbContext + &mut self methods.
// ─────────────────────────────────────────────────────────────────

#[derive(Default)]
struct DbContext {
    count: u32,
}

impl DbContext {
    fn add(&mut self) {
        self.count += 1;
    }
    fn total(&self) -> u32 {
        self.count
    }
}

#[derive(Inject)]
struct BlogHandler {
    ctx: DbContext,
    logger: Arc<common::Logger>,
}

#[test]
fn mixed_handler_mutable_self_e2e() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(DbContext::default()))
        .singleton(|_| Arc::new(common::Logger { prefix: "ef".into() }))
        .transient(|r| __rdi_construct_BlogHandler(r))
        .build()
        .unwrap();

    let mut h = p.get_owned::<BlogHandler>();
    assert_eq!(h.ctx.total(), 0, "fresh DbContext starts at 0");

    // &mut self methods work directly — no RwLock, no unsafe.
    h.ctx.add();
    h.ctx.add();
    assert_eq!(h.ctx.total(), 2, "&mut self mutation succeeds on owned DbContext");
    assert_eq!(h.logger.prefix, "ef");
}

// ─────────────────────────────────────────────────────────────────
// Test 13: Captive dependency — Singleton with owned Scoped field rejected
//
// ValidationResolver.get_owned_any still runs captive detection even though
// owned bypasses caching: the dependency graph traversal is not bypassed.
// ─────────────────────────────────────────────────────────────────

#[derive(Default)]
struct CaptiveScoped;

#[derive(Inject)]
struct CaptiveSingleton {
    ctx: CaptiveScoped,
}

#[test]
fn captive_singleton_depends_on_scoped_owned_rejected() {
    let result = ServiceCollection::new()
        .scoped(|_| Arc::new(CaptiveScoped))
        .singleton(|r| __rdi_construct_CaptiveSingleton(r))
        .build();

    assert!(
        matches!(result, Err(RdiError::SingletonDependsOnScoped { .. })),
        "Singleton with owned Scoped field must be rejected at build"
    );
}
