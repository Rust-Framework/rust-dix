//! Build-time singleton injection of the container itself.
//!
//! Regression coverage for the 0.7.x gap where an eagerly built singleton could
//! not receive `ServiceProvider` / `dyn ScopeFactory`:
//!
//! - `#[inject(provider)]` on an eager singleton used to fail at `build()` with
//!   `FactoryPanic("ServiceProvider not available")`.
//! - `#[inject] Arc<dyn ScopeFactory>` on an eager singleton (and on lazy
//!   services) used to fail with `ServiceNotFound`.
//!
//! Both now resolve during build: the provider `Arc` is created before
//! dependency validation and the built-in `ScopeFactory` is reachable through
//! the `get_any` string path used by the inject macros.

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Inject)]
struct EagerScopeFactoryWorker {
    #[inject]
    scope_factory: Arc<dyn ScopeFactory>,
}

#[derive(Inject)]
struct EagerProviderWorker {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,
}

#[derive(Inject)]
struct EagerPlainProviderWorker {
    #[inject]
    provider: Arc<ServiceProvider>,
}

#[test]
fn provider_is_resolvable_like_a_builtin_service() {
    let provider = ServiceCollection::new().build().unwrap();
    let from_get: Arc<ServiceProvider> = provider.get().expect("provider is built-in");
    assert!(
        Arc::ptr_eq(&provider, &from_get),
        "get::<ServiceProvider>() must return the container itself"
    );
}

#[test]
fn eager_singleton_injects_provider_through_plain_inject() {
    let provider = ServiceCollection::new()
        .singleton(|r| __rdi_construct_EagerPlainProviderWorker(r))
        .build()
        .expect("plain #[inject] Arc<ServiceProvider> must work in eager singletons");

    let worker: Arc<EagerPlainProviderWorker> = provider.get().unwrap();
    let resolved: Arc<ServiceProvider> = worker.provider.get().unwrap();
    assert!(Arc::ptr_eq(&provider, &resolved));
}

#[test]
fn eager_singleton_injects_builtin_scope_factory() {
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 1 }))
        .singleton(|r| __rdi_construct_EagerScopeFactoryWorker(r))
        .build()
        .expect("eager singleton with ScopeFactory must build");

    let worker: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();

    let scope1 = worker.scope_factory.create_scope();
    let a: Arc<common::MyService> = scope1.get().unwrap();
    let b: Arc<common::MyService> = scope1.get().unwrap();
    assert!(
        Arc::ptr_eq(&a, &b),
        "same scope must reuse the scoped instance"
    );

    let scope2 = worker.scope_factory.create_scope();
    let c: Arc<common::MyService> = scope2.get().unwrap();
    assert!(
        !Arc::ptr_eq(&a, &c),
        "different scopes must own independent scoped instances"
    );
    assert_eq!((a.value, c.value), (1, 1));
}

#[test]
fn eager_singleton_injects_service_provider() {
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 42 }))
        .singleton(|r| __rdi_construct_EagerProviderWorker(r))
        .build()
        .expect("eager singleton with ServiceProvider must build");

    let worker: Arc<EagerProviderWorker> = provider.get().unwrap();

    // The injected provider is the root provider: resolving a scoped service
    // through it caches in the root scope (MEDI root-scope semantics).
    let from_root: Arc<common::MyService> = worker.provider.get().unwrap();
    assert_eq!(from_root.value, 42);

    // Per-task isolation still comes from explicit scopes, as documented.
    let scope = worker.provider.scope();
    let scoped: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(scoped.value, 42);
    assert!(
        !Arc::ptr_eq(&from_root, &scoped),
        "scope must not reuse the root-scope instance"
    );
}

#[test]
fn lazy_transient_injects_builtin_scope_factory() {
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 7 }))
        .transient(|r| __rdi_construct_EagerScopeFactoryWorker(r))
        .build()
        .expect("transient with ScopeFactory must build");

    let worker: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();
    let scope = worker.scope_factory.create_scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 7);
}

#[test]
fn eager_singleton_factory_runs_exactly_once() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);

    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 0 }))
        .singleton(|r| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            __rdi_construct_EagerScopeFactoryWorker(r)
        })
        .build()
        .unwrap();

    assert_eq!(
        BUILDS.load(Ordering::SeqCst),
        1,
        "eager init must not double-build"
    );
    let _: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();
    assert_eq!(
        BUILDS.load(Ordering::SeqCst),
        1,
        "resolution must reuse the cache"
    );
}

#[test]
fn scope_factory_injection_works_through_build_async() {
    let provider = futures::executor::block_on(
        ServiceCollection::new()
            .scoped(|_| Arc::new(common::MyService { value: 3 }))
            .singleton(|r| __rdi_construct_EagerScopeFactoryWorker(r))
            .build_async(),
    )
    .expect("build_async must support eager ScopeFactory injection");

    let worker: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();
    let scope = worker.scope_factory.create_scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 3);
}

#[test]
fn provider_mediated_self_resolution_is_rejected_as_cycle() {
    // A factory that uses the injected provider to resolve its own service must
    // surface CircularDependency (not recurse until stack overflow).
    #[derive(Debug)]
    struct SelfReferential;

    let result = ServiceCollection::new()
        .singleton(|r| {
            let provider = r
                .provider_arc()
                .expect("provider must be available during validation");
            let error = provider
                .get::<SelfReferential>()
                .expect_err("resolving the in-flight singleton must fail");
            assert!(
                matches!(error, RdiError::CircularDependency(_)),
                "expected CircularDependency, got {error:?}"
            );
            Arc::new(SelfReferential)
        })
        .build();

    match result {
        Ok(_) => {}
        Err(error) => panic!("build failed: {error:?}"),
    }
}

#[test]
#[should_panic(expected = "has been dropped")]
fn builtin_scope_factory_does_not_keep_the_provider_alive() {
    // The built-in ScopeFactory holds a Weak reference: a singleton can hold the
    // factory without creating a `container -> singleton -> container` Arc cycle.
    let provider = ServiceCollection::new()
        .singleton(|r| __rdi_construct_EagerScopeFactoryWorker(r))
        .build()
        .unwrap();
    let worker: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();

    drop(provider);

    // Provider was the only non-weak owner and is now dropped; using the factory
    // afterwards is a contract violation and panics instead of leaking the
    // container.
    let _ = worker.scope_factory.create_scope();
}
