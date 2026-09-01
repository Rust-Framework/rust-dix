//! Diagnostics & quality-of-service tests for rust-dix 0.7.

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::sync::Arc;

#[derive(Debug, Inject)]
struct NeedsLogger {
    #[inject]
    log: Arc<common::Logger>,
}

#[test]
fn try_construct_returns_typed_not_found_with_field_context() {
    let p = ServiceCollection::new().build().unwrap();
    let err = __rdi_try_construct_NeedsLogger(&*p).unwrap_err();
    match err {
        RdiError::ServiceNotFound { ty, context } => {
            assert!(ty.contains("Logger"), "ty={ty}");
            let ctx = context.expect("context should be set");
            assert!(
                ctx.contains("NeedsLogger") && ctx.contains("log"),
                "context={ctx}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn get_maps_inject_panic_any_to_typed_error() {
    let p = ServiceCollection::new()
        .transient(|r| __rdi_construct_NeedsLogger(r))
        .build()
        .unwrap();

    let err = p.get::<NeedsLogger>().unwrap_err();
    match err {
        RdiError::ServiceNotFound { context, .. } => {
            let ctx = context.expect("context");
            assert!(ctx.contains("log"), "context={ctx}");
        }
        other => panic!("expected ServiceNotFound, got {other:?}"),
    }
}

#[test]
fn scope_factory_available_without_manual_registration() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 7 }))
        .build()
        .unwrap();

    let sf: Arc<dyn ScopeFactory> = p.get().unwrap();
    let scope = sf.create_scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 7);
}

#[test]
fn try_add_scoped_skips_when_present() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 1 }))
        .try_add_scoped(|_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();
    assert_eq!(p.get::<common::MyService>().unwrap().value, 1);
}

#[test]
fn runtime_transient_cycle_returns_error() {
    #[derive(Debug)]
    struct A;
    #[derive(Debug)]
    struct B;

    let p = ServiceCollection::new()
        .transient(|r| {
            let _ = r.get_any(std::any::type_name::<B>());
            Arc::new(A)
        })
        .transient(|r| {
            let _ = r.get_any(std::any::type_name::<A>());
            Arc::new(B)
        })
        .build()
        .unwrap();

    let err = p.get::<A>().unwrap_err();
    assert!(
        matches!(err, RdiError::CircularDependency(_)),
        "got {err:?}"
    );
}
