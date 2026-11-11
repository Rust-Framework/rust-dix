//! MEDI parity checks for enumeration / registry-query APIs.

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::sync::Arc;

#[test]
fn is_registered_matches_container_builtins() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .keyed_singleton("k", |_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();

    assert!(p.is_registered::<common::MyService>());
    assert!(p.is_registered::<ServiceProvider>());
    assert!(p.is_registered::<dyn ScopeFactory>());
    assert!(!p.is_registered::<String>());
    assert!(p.is_registered_keyed::<common::MyService>("k"));
    assert!(!p.is_registered_keyed::<common::MyService>("missing"));

    let scope = p.scope();
    assert!(scope.is_registered::<common::MyService>());
    assert!(scope.is_registered_keyed::<common::MyService>("k"));
    assert!(!scope.is_registered::<String>());
}

#[test]
fn wrapper_reports_child_and_root_registrations() {
    let root = ServiceCollection::new()
        .singleton(|_| Arc::new(common::Logger { prefix: "r".into() }))
        .build()
        .unwrap();
    let child = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    assert!(w.is_registered::<common::MyService>());
    assert!(w.is_registered::<common::Logger>());
    assert!(!w.is_registered::<String>());
}

#[derive(Inject)]
struct FooVectorConsumer {
    #[inject]
    values: Vec<Arc<common::MyService>>,
}

#[test]
fn vec_injection_uses_unkeyed_registration_order() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .keyed_singleton("k", |_| Arc::new(common::MyService { value: 99 }))
        .singleton(|_| Arc::new(common::MyService { value: 2 }))
        .transient(|r| __rdi_construct_FooVectorConsumer(r))
        .build()
        .unwrap();
    let consumer: Arc<FooVectorConsumer> = p.get().unwrap();
    let values: Vec<i32> = consumer.values.iter().map(|s| s.value).collect();
    assert_eq!(values, vec![1, 2], "unkeyed only, registration order");
}

#[test]
fn get_all_keyed_preserves_registration_order() {
    let p = ServiceCollection::new()
        .keyed_singleton("a", |_| Arc::new(common::MyService { value: 10 }))
        .keyed_singleton("b", |_| Arc::new(common::MyService { value: 20 }))
        .keyed_singleton("c", |_| Arc::new(common::MyService { value: 30 }))
        .build()
        .unwrap();
    let values: Vec<i32> = p
        .get_all_keyed::<common::MyService>()
        .iter()
        .map(|s| s.value)
        .collect();
    assert_eq!(values, vec![10, 20, 30]);
}

#[test]
fn wrapper_get_all_keyed_merges_child_then_root() {
    let root = ServiceCollection::new()
        .keyed_singleton("r", |_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    let child = ServiceCollection::new()
        .keyed_singleton("c", |_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let values: Vec<i32> = w
        .get_all_keyed::<common::MyService>()
        .iter()
        .map(|s| s.value)
        .collect();
    assert_eq!(values, vec![2, 1], "child first, then root");
}

#[test]
fn injector_scope_has_same_registration_view() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .keyed_singleton("k", |_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();
    let scope = p.scope();
    assert_eq!(scope.get_all::<common::MyService>().len(), 1);
    assert_eq!(scope.get_all_keyed::<common::MyService>().len(), 1);
}
