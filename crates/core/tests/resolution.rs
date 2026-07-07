mod common;

use rust_dicore::*;
use std::sync::Arc;

#[test]
fn get_panics_on_missing() {
    let p = ServiceCollection::new().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<common::MyService> = p.get::<common::MyService>().unwrap();
    }));
    assert!(result.is_err());
}

#[test]
fn get_optional_returns_none_on_missing() {
    let p = ServiceCollection::new().build().unwrap();
    assert!(p.get_optional::<common::MyService>().is_none());
}

#[test]
fn get_returns_service() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 42 }))
        .build()
        .unwrap();
    let svc: Arc<common::MyService> = p.get().unwrap();
    assert_eq!(svc.value, 42);
}

#[test]
fn get_optional_returns_service() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 99 }))
        .build()
        .unwrap();
    let svc: Option<Arc<common::MyService>> = p.get_optional();
    assert_eq!(svc.unwrap().value, 99);
}

#[test]
fn get_all_returns_empty_when_none() {
    let p = ServiceCollection::new().build().unwrap();
    let all: Vec<Arc<common::MyService>> = p.get_all();
    assert!(all.is_empty());
}

#[test]
fn get_all_returns_all_keyed() {
    let p = ServiceCollection::new()
        .keyed_singleton("a", |_| Arc::new(common::MyService { value: 1 }))
        .keyed_singleton("b", |_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();
    let all: Vec<Arc<common::MyService>> = p.get_all();
    assert_eq!(all.len(), 2);
}

#[test]
fn resolver_default_try_get_returns_none() {
    let p = ServiceCollection::new().build().unwrap();
    let result: Option<Arc<common::MyService>> = p.try_get();
    assert!(result.is_none());
}

#[test]
fn resolver_default_try_get_keyed_returns_none() {
    let p = ServiceCollection::new().build().unwrap();
    let result: Option<Arc<common::MyService>> = p.try_get_keyed("x");
    assert!(result.is_none());
}

#[test]
fn resolver_default_get_keyed_panics_on_missing() {
    let p = ServiceCollection::new().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<common::MyService> = p.get_keyed("nonexistent").unwrap();
    }));
    assert!(result.is_err());
}

#[test]
fn resolver_default_get_panics_on_missing() {
    let p = ServiceCollection::new().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<common::MyService> = p.get().unwrap();
    }));
    assert!(result.is_err());
}

#[test]
fn provider_get_all_empty() {
    let p = ServiceCollection::new().build().unwrap();
    let all: Vec<Arc<common::MyService>> = p.get_all();
    assert!(all.is_empty());
}

#[test]
fn provider_has_correct_keyed_count() {
    let p = ServiceCollection::new()
        .keyed_singleton("a", |_| Arc::new(common::MyService { value: 1 }))
        .keyed_singleton("b", |_| Arc::new(common::MyService { value: 2 }))
        .keyed_singleton("c", |_| Arc::new(common::MyService { value: 3 }))
        .build()
        .unwrap();
    assert_eq!(p.get_keyed::<common::MyService>("a").unwrap().value, 1);
    assert_eq!(p.get_keyed::<common::MyService>("b").unwrap().value, 2);
    assert_eq!(p.get_keyed::<common::MyService>("c").unwrap().value, 3);
    assert_eq!(p.get_all::<common::MyService>().len(), 3);
}

#[test]
fn multiple_singletons_independent() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 10 }))
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "log".into(),
            })
        })
        .build()
        .unwrap();
    assert_eq!(p.get::<common::MyService>().unwrap().value, 10);
    assert_eq!(p.get::<common::Logger>().unwrap().prefix, "log");
}

#[test]
fn trait_object_multiple_impls() {
    use common::{AltPlugin, IPlugin, TestPlugin};
    let p = ServiceCollection::new()
        .singleton::<dyn IPlugin>(|_| Arc::new(TestPlugin))
        .keyed_singleton::<dyn IPlugin>("alt", |_| Arc::new(AltPlugin))
        .build()
        .unwrap();
    let default: Arc<dyn IPlugin> = p.get().unwrap();
    assert_eq!(default.name(), "test_plugin");
    let alt: Arc<dyn IPlugin> = p.get_keyed("alt").unwrap();
    assert_eq!(alt.name(), "alt_plugin");
}

#[test]
fn try_get_keyed_returns_correct_value() {
    let p = ServiceCollection::new()
        .keyed_singleton("exists", |_| Arc::new(common::MyService { value: 42 }))
        .build()
        .unwrap();
    let present: Option<Arc<common::MyService>> = p.try_get_keyed("exists");
    assert_eq!(present.unwrap().value, 42);
    let absent: Option<Arc<common::MyService>> = p.try_get_keyed("nope");
    assert!(absent.is_none());
}
