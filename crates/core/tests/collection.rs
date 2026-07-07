mod common;

use rust_dix::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn collection_singleton_value() {
    let p = ServiceCollection::new()
        .singleton_value(common::MyService { value: 42 })
        .build()
        .unwrap();
    assert_eq!(p.get::<common::MyService>().unwrap().value, 42);
}

#[test]
fn collection_scoped_builds() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 10 }))
        .build()
        .unwrap();
    let svc: Arc<common::MyService> = p.get().unwrap();
    assert_eq!(svc.value, 10);
}

#[test]
fn collection_keyed_transient() {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let p = ServiceCollection::new()
        .keyed_transient("t", |_| {
            COUNT.fetch_add(1, Ordering::SeqCst);
            Arc::new(common::MyService { value: 1 })
        })
        .build()
        .unwrap();
    let _a: Arc<common::MyService> = p.get_keyed("t").unwrap();
    let _b: Arc<common::MyService> = p.get_keyed("t").unwrap();
    assert_eq!(COUNT.load(Ordering::SeqCst), 2);
}

#[test]
fn collection_keyed_scoped() {
    let p = ServiceCollection::new()
        .keyed_scoped("s", |_| Arc::new(common::MyService { value: 5 }))
        .build()
        .unwrap();
    let scope = p.scope();
    let a: Arc<common::MyService> = scope.get_keyed("s").unwrap();
    let b: Arc<common::MyService> = scope.get_keyed("s").unwrap();
    assert_eq!(a.value, 5);
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn collection_add_with_explicit_lifetime() {
    let p = ServiceCollection::new()
        .add(ServiceLifetime::Singleton, |_| {
            Arc::new(common::Logger {
                prefix: "explicit".into(),
            })
        })
        .build()
        .unwrap();
    assert_eq!(p.get::<common::Logger>().unwrap().prefix, "explicit");
}

#[test]
fn collection_default_is_empty() {
    let c = ServiceCollection::default();
    let p = c.build().unwrap();
    assert!(p.get_optional::<common::MyService>().is_none());
}

#[test]
fn composite_resolution() {
    let p = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "comp".into(),
            })
        })
        .transient(|_| {
            Arc::new(common::CompositeService {
                logger: Arc::new(common::Logger {
                    prefix: "comp".into(),
                }),
            })
        })
        .build()
        .unwrap();
    let composite: Arc<common::CompositeService> = p.get().unwrap();
    assert_eq!(composite.logger.prefix, "comp");
}
