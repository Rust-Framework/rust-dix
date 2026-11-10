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
//! the `get_any` string path used by the inject macros. Constructors may store
//! the injected handles but must not resolve services through them — those calls
//! are deterministically rejected at build time (see the last tests).

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::any::TypeId;
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
    let from_get: Arc<dyn ScopeFactory> = provider.get().unwrap();
    assert!(
        Arc::ptr_eq(&worker.scope_factory, &from_get),
        "eager injection must reuse the provider's cached ScopeFactory singleton"
    );

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
fn build_time_provider_resolution_is_rejected() {
    // Constructors may store the injected provider but must not resolve
    // services through it: the attempt fails with a typed FactoryPanic, so no
    // service can be built dynamically and bypass validation.
    #[derive(Debug)]
    struct SelfReferential;

    let result = ServiceCollection::new()
        .singleton(|r| {
            let provider = r
                .provider_arc()
                .expect("provider must be available during validation");
            let error = provider
                .get::<SelfReferential>()
                .expect_err("resolving through the provider during build must fail");
            match error {
                RdiError::FactoryPanic { payload, .. } => assert!(
                    payload.contains("resolve lazily"),
                    "expected a constructor-resolution rejection, got: {payload}"
                ),
                other => panic!("expected FactoryPanic, got {other:?}"),
            }
            Arc::new(SelfReferential)
        })
        .build();

    match result {
        Ok(_) => {}
        Err(error) => {
            panic!("the constructor handled the rejection, build should succeed: {error:?}")
        }
    }
}

#[test]
fn build_time_provider_resolution_is_order_independent() {
    // Before the constructor guard, dynamic resolution during validation could
    // build a captive singleton before its static turn (order-dependent:
    // 84 Ok / 116 Err over 200 identical builds). With the guard, no dynamic
    // build happens and the captive dependency is always reported.
    static REJECTIONS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug)]
    struct A;
    #[derive(Debug)]
    struct B;
    #[derive(Debug)]
    struct Captive;

    fn a_factory(r: &dyn IServiceResolver) -> Arc<A> {
        let provider = r
            .provider_arc()
            .expect("provider must be available during validation");
        if let Err(RdiError::FactoryPanic { payload, .. }) = provider.get::<B>() {
            assert!(
                payload.contains("resolve lazily"),
                "unexpected payload: {payload}"
            );
            REJECTIONS.fetch_add(1, Ordering::SeqCst);
        }
        Arc::new(A)
    }

    // B is captive: it (transitively) requires the Scoped service.
    fn b_factory(r: &dyn IServiceResolver) -> Arc<B> {
        let any = r
            .get_any(std::any::type_name::<Captive>())
            .expect("Captive must resolve through the validation resolver");
        let arc = any.downcast::<Arc<Captive>>().expect("Captive downcast");
        let captive: Arc<Captive> = Arc::clone(&*arc);
        std::hint::black_box(captive);
        Arc::new(B)
    }

    for b_first in [false, true] {
        for _ in 0..20 {
            let collection = ServiceCollection::new().scoped(|_| Arc::new(Captive));
            let collection = if b_first {
                collection.singleton(b_factory).singleton(a_factory)
            } else {
                collection.singleton(a_factory).singleton(b_factory)
            };
            match collection.build() {
                Err(RdiError::SingletonDependsOnScoped { .. }) => {}
                Err(other) => panic!("expected SingletonDependsOnScoped, got {other:?}"),
                Ok(_) => {
                    panic!("build must reject provider-mediated resolution (b_first={b_first})")
                }
            }
        }
    }

    // The guard must have fired before B was built (both registration orders).
    assert!(REJECTIONS.load(Ordering::SeqCst) > 0);
}

#[test]
fn builtin_services_resolve_through_resolver_paths() {
    let provider = ServiceCollection::new().build().unwrap();

    let by_type_id = IServiceResolver::get_by_type_id(&*provider, TypeId::of::<dyn ScopeFactory>())
        .expect("get_by_type_id must expose the built-in ScopeFactory");
    assert!(by_type_id.downcast::<Arc<dyn ScopeFactory>>().is_ok());

    let by_name = IServiceResolver::get_any(&*provider, std::any::type_name::<dyn ScopeFactory>())
        .expect("get_any must expose the built-in ScopeFactory");
    assert!(by_name.downcast::<Arc<dyn ScopeFactory>>().is_ok());

    let provider_any =
        IServiceResolver::get_by_type_id(&*provider, TypeId::of::<ServiceProvider>())
            .expect("get_by_type_id must expose the built-in ServiceProvider");
    assert!(provider_any.downcast::<Arc<ServiceProvider>>().is_ok());
}

#[test]
fn builtin_scope_factory_is_a_singleton() {
    let provider = ServiceCollection::new().build().unwrap();
    let first: Arc<dyn ScopeFactory> = provider.get().unwrap();
    let second: Arc<dyn ScopeFactory> = provider.get().unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "the built-in ScopeFactory is a singleton (MEDI parity)"
    );
}

#[test]
fn explicit_scope_factory_registration_takes_precedence() {
    struct CountingFactory {
        provider: Arc<ServiceProvider>,
        calls: AtomicUsize,
    }

    impl ScopeFactory for CountingFactory {
        fn create_scope(&self) -> Scope {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.provider.scope()
        }
    }

    // Auxiliary container: the explicit factory must create scopes over it, not
    // over the container that resolves it (which is what the built-in would do).
    let auxiliary = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 99 }))
        .build()
        .unwrap();
    let explicit = Arc::new(CountingFactory {
        provider: Arc::clone(&auxiliary),
        calls: AtomicUsize::new(0),
    });
    let explicit_for_registration = Arc::clone(&explicit);

    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 1 }))
        .singleton::<dyn ScopeFactory>(move |_| {
            Arc::clone(&explicit_for_registration) as Arc<dyn ScopeFactory>
        })
        .singleton(|r| __rdi_construct_EagerScopeFactoryWorker(r))
        .build()
        .expect("explicit ScopeFactory registration must build");

    // `provider.get` honours the explicit registration.
    let from_get: Arc<dyn ScopeFactory> = provider.get().unwrap();
    assert!(Arc::ptr_eq(
        &from_get,
        &(explicit.clone() as Arc<dyn ScopeFactory>)
    ));

    // Field injection in an eager singleton does too.
    let worker: Arc<EagerScopeFactoryWorker> = provider.get().unwrap();
    let scope = worker.scope_factory.create_scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 99, "explicit factory must win over the built-in");
    assert_eq!(explicit.calls.load(Ordering::SeqCst), 1);
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
