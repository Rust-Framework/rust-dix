//! 0.8.0 scope validation (MEDI `ValidateScopes` equivalent).
//!
//! - Default: resolving a `Scoped` service from the **root** provider fails with
//!   `RdiError::ScopedFromRootProvider`; infallible lookups treat it as
//!   unresolvable; constructor dependencies propagate the typed error.
//! - Scopes resolve scoped services normally.
//! - `ServiceProviderOptions::lenient()` restores the 0.7 root-scope behavior.

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::sync::Arc;

#[derive(Debug)]
struct ScopedSvc(i32);

#[test]
fn root_scoped_resolution_is_rejected_by_default() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(1)))
        .build()
        .unwrap();

    match p.get::<ScopedSvc>() {
        Err(RdiError::ScopedFromRootProvider { service }) => {
            assert!(service.contains("ScopedSvc"));
        }
        other => panic!("expected ScopedFromRootProvider, got {other:?}"),
    }
    assert!(p.get_optional::<ScopedSvc>().is_none());
    assert!(
        p.get_all::<ScopedSvc>().is_empty(),
        "no scoped instances leak through enumeration"
    );
}

#[test]
fn transient_chain_from_root_propagates_scope_error() {
    // Transient is resolvable at the root, but its scoped dependency is not:
    // the typed error must survive the constructor/panic boundary.
    #[derive(Debug)]
    struct ScopedDep;
    #[derive(Debug)]
    struct TransientWithScopedDep {
        #[allow(dead_code)]
        dep: Arc<ScopedDep>,
    }

    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedDep))
        .transient(|r| {
            let dep = r
                .get_any(std::any::type_name::<ScopedDep>())
                .expect("scoped dep resolves through the resolver");
            let arc = dep.downcast::<Arc<ScopedDep>>().unwrap();
            Arc::new(TransientWithScopedDep {
                dep: Arc::clone(&*arc),
            })
        })
        .build()
        .unwrap();

    match p.get::<TransientWithScopedDep>() {
        Err(RdiError::ScopedFromRootProvider { service }) => {
            assert!(service.contains("ScopedDep"), "service={service}");
        }
        other => panic!("expected ScopedFromRootProvider, got {other:?}"),
    }
}

#[test]
fn injected_constructor_dependency_from_root_surfaces_scope_error() {
    #[derive(Debug)]
    struct ScopedDep;
    #[derive(Inject, Debug)]
    struct TransientConsumer {
        #[inject]
        #[allow(dead_code)]
        dep: Arc<ScopedDep>,
    }

    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedDep))
        .transient(|r| __rdi_construct_TransientConsumer(r))
        .build()
        .unwrap();

    match p.get::<TransientConsumer>() {
        Err(RdiError::ScopedFromRootProvider { .. }) => {}
        other => panic!("expected ScopedFromRootProvider, got {other:?}"),
    }
}

#[test]
fn scope_resolves_scoped_services() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(7)))
        .build()
        .unwrap();
    let scope = p.scope();
    let a = scope.get::<ScopedSvc>().unwrap();
    let b = scope.get::<ScopedSvc>().unwrap();
    assert_eq!(a.0, 7);
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn keyed_scoped_from_root_is_rejected() {
    let p = ServiceCollection::new()
        .keyed_scoped("k", |_| Arc::new(ScopedSvc(3)))
        .build()
        .unwrap();
    assert!(matches!(
        p.get_keyed::<ScopedSvc>("k"),
        Err(RdiError::ScopedFromRootProvider { .. })
    ));
    let scope = p.scope();
    assert_eq!(scope.get_keyed::<ScopedSvc>("k").unwrap().0, 3);
}

#[test]
fn lenient_options_restore_root_scope_caching() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(5)))
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();
    let a = p.get::<ScopedSvc>().unwrap();
    let b = p.get::<ScopedSvc>().unwrap();
    assert_eq!(a.0, 5);
    assert!(Arc::ptr_eq(&a, &b), "lenient root caches scoped instances");
}

#[test]
fn options_are_buildable_from_default_and_builder() {
    assert!(ServiceProviderOptions::new().validate_scopes);
    assert!(ServiceProviderOptions::default().validate_scopes);
    assert!(
        !ServiceProviderOptions::new()
            .with_validate_scopes(false)
            .validate_scopes
    );
    assert!(!ServiceProviderOptions::lenient().validate_scopes);
}

#[tokio::test]
async fn async_scoped_from_root_is_rejected() {
    let p = ServiceCollection::new()
        .async_scoped(|_| Box::pin(async { Arc::new(ScopedSvc(11)) }))
        .build_async()
        .await
        .unwrap();

    match p.get_async::<ScopedSvc>().await {
        Err(RdiError::ScopedFromRootProvider { .. }) => {}
        other => panic!("expected ScopedFromRootProvider, got {other:?}"),
    }

    let scope = p.scope();
    let scoped: Arc<ScopedSvc> = scope.get_async().await.unwrap();
    assert_eq!(scoped.0, 11);
}

#[test]
fn get_owned_is_exempt_from_root_scope_validation() {
    // Owned resolution returns a caller-owned value and never enters a scope
    // cache, so it is allowed at the root (its dependencies are still checked).
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(8)))
        .build()
        .unwrap();
    let owned: ScopedSvc = p.get_owned().unwrap();
    assert_eq!(owned.0, 8);
    // The root cache must stay untouched: a scope still gets its own instance.
    let scope = p.scope();
    let scoped = scope.get::<ScopedSvc>().unwrap();
    assert_eq!(scoped.0, 8);
}

#[test]
fn scope_factory_created_scopes_are_not_root() {
    // Background-worker pattern: singleton injects the built-in ScopeFactory and
    // resolves scoped services per tick (MEDI IServiceScopeFactory recipe).
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(2)))
        .build()
        .unwrap();
    let factory: Arc<dyn ScopeFactory> = p.get().unwrap();
    let scope = factory.create_scope();
    assert_eq!(scope.get::<ScopedSvc>().unwrap().0, 2);
}

#[test]
fn common_service_still_resolvable_at_root() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 4 }))
        .build()
        .unwrap();
    assert_eq!(p.get::<common::MyService>().unwrap().value, 4);
}
