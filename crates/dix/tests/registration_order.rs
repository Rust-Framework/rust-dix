//! 0.8.0 MEDI registration-order semantics.
//!
//! - Single-service resolution: the **last** registration wins (MEDI
//!   `AddXxx` override semantics), for unkeyed and same-key keyed registrations.
//! - `get_all` returns unkeyed registrations in registration order.
//! - `try_add*` keeps "register only if absent" (MEDI `TryAdd`) semantics.

use rust_dix::*;
use std::sync::Arc;

#[derive(Debug, PartialEq)]
struct S(i32);

#[test]
fn single_registration_last_wins() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(S(1)))
        .singleton(|_| Arc::new(S(2)))
        .singleton(|_| Arc::new(S(3)))
        .build()
        .unwrap();
    assert_eq!(p.get::<S>().unwrap().0, 3);
    assert_eq!(p.get_optional::<S>().unwrap().0, 3);
}

#[test]
fn keyed_same_key_last_wins() {
    let p = ServiceCollection::new()
        .keyed_singleton("k", |_| Arc::new(S(1)))
        .keyed_singleton("k", |_| Arc::new(S(2)))
        .keyed_singleton("other", |_| Arc::new(S(9)))
        .build()
        .unwrap();
    assert_eq!(p.get_keyed::<S>("k").unwrap().0, 2);
    assert_eq!(p.get_keyed::<S>("other").unwrap().0, 9);
}

#[test]
fn registry_dependent_singleton_sees_last_registration() {
    // Validation/eager build must use the same "last wins" lookup as runtime.
    #[derive(Debug)]
    struct Dep(i32);
    #[derive(Debug)]
    struct Consumer(i32);

    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(Dep(1)))
        .singleton(|_| Arc::new(Dep(2)))
        .singleton(|r| {
            let dep = r
                .get_any(std::any::type_name::<Dep>())
                .and_then(|a| a.downcast::<Arc<Dep>>().ok().map(|d| Arc::clone(&*d)))
                .expect("Dep registered");
            Arc::new(Consumer(dep.0))
        })
        .build()
        .unwrap();

    assert_eq!(p.get::<Consumer>().unwrap().0, 2, "works at build time too");
}

#[test]
fn transient_owned_last_wins() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(S(1)))
        .transient(|_| Arc::new(S(2)))
        .build()
        .unwrap();
    assert_eq!(p.get_owned::<S>().unwrap().0, 2);
}

#[test]
fn keyed_owned_same_key_last_wins() {
    let p = ServiceCollection::new()
        .keyed_transient("k", |_| Arc::new(S(1)))
        .keyed_transient("k", |_| Arc::new(S(2)))
        .build()
        .unwrap();
    assert_eq!(p.get_keyed_owned::<S>("k").unwrap().0, 2);
}

#[test]
fn try_add_keeps_first_registration() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(S(1)))
        .try_add(|_| Arc::new(S(2)))
        .try_add_scoped(|_| Arc::new(S(3)))
        .build()
        .unwrap();
    assert_eq!(p.get::<S>().unwrap().0, 1, "TryAdd never overrides");
}

#[test]
fn get_all_is_registration_order_and_unkeyed_only() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(S(1)))
        .keyed_singleton("k", |_| Arc::new(S(99)))
        .singleton(|_| Arc::new(S(2)))
        .singleton(|_| Arc::new(S(3)))
        .build()
        .unwrap();

    let values: Vec<i32> = p.get_all::<S>().iter().map(|s| s.0).collect();
    assert_eq!(values, vec![1, 2, 3], "registration order, keyed excluded");

    let keyed: Vec<i32> = p.get_all_keyed::<S>().iter().map(|s| s.0).collect();
    assert_eq!(keyed, vec![99]);
}

#[test]
fn get_all_does_not_override_single_resolution() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(S(1)))
        .singleton(|_| Arc::new(S(2)))
        .build()
        .unwrap();
    assert_eq!(p.get::<S>().unwrap().0, 2);
    assert_eq!(p.get_all::<S>().len(), 2);
}

#[tokio::test]
async fn async_single_registration_last_wins() {
    let p = ServiceCollection::new()
        .async_singleton(|_| Box::pin(async { Arc::new(S(1)) }))
        .async_singleton(|_| Box::pin(async { Arc::new(S(2)) }))
        .build_async()
        .await
        .unwrap();
    let s: Arc<S> = p.get_async().await.unwrap();
    assert_eq!(s.0, 2);
}

#[test]
fn scope_uses_same_last_wins_rule() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(S(1)))
        .scoped(|_| Arc::new(S(2)))
        .build()
        .unwrap();
    let scope = p.scope();
    assert_eq!(scope.get::<S>().unwrap().0, 2);
}
