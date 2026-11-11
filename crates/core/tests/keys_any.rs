//! 0.9 keyed-service tests: typed keys and `AnyKey` fallbacks.

use rust_dix::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Hash)]
enum DbKey {
    Read,
    Write,
}

#[derive(Debug, PartialEq)]
struct Conn(&'static str);

#[test]
fn typed_keys_register_and_resolve() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Conn>(ServiceKey::new(DbKey::Read), |_| Arc::new(Conn("read")))
        .keyed_singleton::<Conn>(ServiceKey::new(DbKey::Write), |_| Arc::new(Conn("write")))
        .build()
        .unwrap();

    assert_eq!(
        p.get_keyed::<Conn>(ServiceKey::new(DbKey::Read)).unwrap().0,
        "read"
    );
    assert_eq!(
        p.get_keyed::<Conn>(ServiceKey::new(DbKey::Write))
            .unwrap()
            .0,
        "write"
    );
    assert!(p.get_keyed::<Conn>(ServiceKey::new(DbKey::Read)).is_ok());
    assert!(p.is_registered_keyed::<Conn>(ServiceKey::new(DbKey::Read)));
    assert!(!p.is_registered_keyed::<Conn>(ServiceKey::new("read")));
    assert_eq!(p.get_all_keyed::<Conn>().len(), 2);
}

#[test]
fn typed_and_string_keys_do_not_collide() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Conn>("1", |_| Arc::new(Conn("string-1")))
        .keyed_singleton::<Conn>(ServiceKey::new(1_u32), |_| Arc::new(Conn("u32-1")))
        .build()
        .unwrap();

    assert_eq!(p.get_keyed::<Conn>("1").unwrap().0, "string-1");
    assert_eq!(
        p.get_keyed::<Conn>(ServiceKey::new(1_u32)).unwrap().0,
        "u32-1"
    );
}

#[test]
fn typed_key_last_registration_wins() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Conn>(ServiceKey::new(DbKey::Read), |_| Arc::new(Conn("first")))
        .keyed_singleton::<Conn>(ServiceKey::new(DbKey::Read), |_| Arc::new(Conn("second")))
        .build()
        .unwrap();
    assert_eq!(
        p.get_keyed::<Conn>(ServiceKey::new(DbKey::Read)).unwrap().0,
        "second"
    );
}

#[test]
fn typed_key_owned_and_scope_resolution() {
    let p = ServiceCollection::new()
        .keyed_transient::<Conn>(ServiceKey::new(DbKey::Write), |_| Arc::new(Conn("owned")))
        .keyed_scoped::<Conn>(ServiceKey::new(DbKey::Read), |_| Arc::new(Conn("scoped")))
        .build()
        .unwrap();

    let owned: Conn = p.get_keyed_owned(ServiceKey::new(DbKey::Write)).unwrap();
    assert_eq!(owned.0, "owned");

    let scope = p.scope();
    let a = scope
        .get_keyed::<Conn>(ServiceKey::new(DbKey::Read))
        .unwrap();
    let b = scope
        .get_keyed::<Conn>(ServiceKey::new(DbKey::Read))
        .unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn any_key_fallback_resolves_unregistered_keys() {
    let p = ServiceCollection::new()
        .keyed_any_singleton::<Conn>(|_, key| {
            Arc::new(Conn(if key.is_any() { "any" } else { "fallback" }))
        })
        .build()
        .unwrap();

    assert_eq!(p.get_keyed::<Conn>("alpha").unwrap().0, "fallback");
    assert_eq!(
        p.get_keyed::<Conn>(ServiceKey::new(DbKey::Read)).unwrap().0,
        "fallback"
    );
    assert!(p.is_registered_keyed::<Conn>("alpha"));
    assert!(p.is_registered_keyed::<Conn>(ServiceKey::new(DbKey::Read)));
}

#[test]
fn any_key_singleton_caches_per_requested_key() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    let p = ServiceCollection::new()
        .keyed_any_singleton::<Conn>(|_, key| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Arc::new(Conn(if key.is_any() { "?" } else { "x" }))
        })
        .build()
        .unwrap();

    let a1 = p.get_keyed::<Conn>("a").unwrap();
    let a2 = p.get_keyed::<Conn>("a").unwrap();
    let b = p.get_keyed::<Conn>("b").unwrap();
    assert!(Arc::ptr_eq(&a1, &a2), "same requested key reuses instance");
    assert!(
        !Arc::ptr_eq(&a1, &b),
        "different requested keys get distinct instances"
    );
    assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
}

#[test]
fn any_key_scoped_caches_per_scope_and_key() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    let p = ServiceCollection::new()
        .keyed_any_scoped::<Conn>(|_, _| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Arc::new(Conn("scoped"))
        })
        .build()
        .unwrap();

    let s1 = p.scope();
    let a1 = s1.get_keyed::<Conn>("a").unwrap();
    let a2 = s1.get_keyed::<Conn>("a").unwrap();
    let b1 = s1.get_keyed::<Conn>("b").unwrap();
    assert!(Arc::ptr_eq(&a1, &a2));
    assert!(!Arc::ptr_eq(&a1, &b1));

    let s2 = p.scope();
    let a3 = s2.get_keyed::<Conn>("a").unwrap();
    assert!(
        !Arc::ptr_eq(&a1, &a3),
        "different scopes get distinct instances"
    );
}

#[test]
fn any_key_transient_is_fresh_and_receives_key() {
    use std::sync::Mutex;
    static KEYS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let p = ServiceCollection::new()
        .keyed_any_transient::<Conn>(|_, key| {
            KEYS.lock().unwrap().push(key.to_string());
            Arc::new(Conn("fresh"))
        })
        .build()
        .unwrap();

    let a = p.get_keyed::<Conn>("a").unwrap();
    let b = p.get_keyed::<Conn>("b").unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    let keys = KEYS.lock().unwrap().clone();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().any(|k| k.contains("a")));
    assert!(keys.iter().any(|k| k.contains("b")));
}

#[test]
fn exact_key_registration_wins_over_any_key_fallback() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Conn>("special", |_| Arc::new(Conn("exact")))
        .keyed_any_singleton::<Conn>(|_, _| Arc::new(Conn("fallback")))
        .build()
        .unwrap();

    assert_eq!(p.get_keyed::<Conn>("special").unwrap().0, "exact");
    assert_eq!(p.get_keyed::<Conn>("other").unwrap().0, "fallback");
}

#[test]
fn get_all_keyed_excludes_any_key_fallback() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Conn>("a", |_| Arc::new(Conn("a")))
        .keyed_any_singleton::<Conn>(|_, _| Arc::new(Conn("fallback")))
        .build()
        .unwrap();
    let names: Vec<&str> = p.get_all_keyed::<Conn>().iter().map(|c| c.0).collect();
    assert_eq!(names, vec!["a"]);
}

#[test]
fn resolving_the_any_key_sentinel_itself_is_not_allowed() {
    let p = ServiceCollection::new()
        .keyed_any_singleton::<Conn>(|_, _| Arc::new(Conn("fallback")))
        .build()
        .unwrap();
    assert!(p.get_keyed::<Conn>(ServiceKey::any_key()).is_err());
    assert!(!p.is_registered_keyed::<Conn>(ServiceKey::any_key()));
}

#[test]
fn any_key_async_resolution_falls_back_to_sync() {
    let p = futures::executor::block_on(
        ServiceCollection::new()
            .keyed_any_singleton::<Conn>(|_, _| Arc::new(Conn("sync-fallback")))
            .build_async(),
    )
    .unwrap();
    let value: Arc<Conn> = futures::executor::block_on(p.get_keyed_async("k")).unwrap();
    assert_eq!(value.0, "sync-fallback");
}

#[test]
fn any_key_singletons_are_released_on_dispose() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Tracked;
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let p = ServiceCollection::new()
        .keyed_any_singleton::<Tracked>(|_, _| Arc::new(Tracked))
        .build()
        .unwrap();
    let _a = p.get_keyed::<Tracked>("a").unwrap();
    let _b = p.get_keyed::<Tracked>("b").unwrap();
    drop(_a);
    drop(_b);
    p.dispose();
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}

#[test]
fn any_key_singleton_consumed_during_validation_keeps_identity() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    struct AnyS(#[allow(dead_code)] usize);
    struct Consumer(#[allow(dead_code)] Arc<AnyS>);

    let provider = ServiceCollection::new()
        .keyed_any_singleton::<AnyS>(|_, _| {
            let n = BUILDS.fetch_add(1, Ordering::SeqCst);
            Arc::new(AnyS(n))
        })
        .singleton(|r| {
            let any = r
                .get_keyed_any(std::any::type_name::<AnyS>(), "buildtime")
                .expect("AnyKey fallback resolves during validation");
            let arc = any.downcast::<Arc<AnyS>>().unwrap();
            Arc::new(Consumer(Arc::clone(&*arc)))
        })
        .build()
        .unwrap();

    assert_eq!(BUILDS.load(Ordering::SeqCst), 1, "singleton built once");
    let runtime = provider.get_keyed::<AnyS>("buildtime").unwrap();
    let consumer: Arc<Consumer> = provider.get().unwrap();
    assert!(
        Arc::ptr_eq(&consumer.0, &runtime),
        "validation-created AnyKey singleton must be reused at runtime"
    );
}

#[test]
fn any_key_singleton_shared_by_multiple_validated_consumers_is_single_instance() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    struct AnyS(#[allow(dead_code)] usize);
    struct C1(#[allow(dead_code)] Arc<AnyS>);
    struct C2(#[allow(dead_code)] Arc<AnyS>);

    fn resolve_any(r: &dyn IServiceResolver) -> Arc<AnyS> {
        let any = r
            .get_keyed_any(std::any::type_name::<AnyS>(), "shared")
            .expect("AnyKey fallback resolves during validation");
        let arc = any.downcast::<Arc<AnyS>>().unwrap();
        Arc::clone(&*arc)
    }

    let provider = ServiceCollection::new()
        .keyed_any_singleton::<AnyS>(|_, _| {
            let n = BUILDS.fetch_add(1, Ordering::SeqCst);
            Arc::new(AnyS(n))
        })
        .singleton(|r| Arc::new(C1(resolve_any(r))))
        .singleton(|r| Arc::new(C2(resolve_any(r))))
        .build()
        .unwrap();

    assert_eq!(BUILDS.load(Ordering::SeqCst), 1, "single initialization");
    let c1: Arc<C1> = provider.get().unwrap();
    let c2: Arc<C2> = provider.get().unwrap();
    let runtime = provider.get_keyed::<AnyS>("shared").unwrap();
    assert!(
        Arc::ptr_eq(&c1.0, &c2.0),
        "both consumers see the same instance"
    );
    assert!(
        Arc::ptr_eq(&c1.0, &runtime),
        "runtime reuses validation instance"
    );
}

#[derive(Debug)]
struct TypeA(usize);

#[derive(Debug)]
struct TypeB(usize);

#[test]
fn any_key_singleton_caches_per_service_and_key() {
    let p = ServiceCollection::new()
        .keyed_any_singleton::<TypeA>(|_, _| Arc::new(TypeA(1)))
        .keyed_any_singleton::<TypeB>(|_, _| Arc::new(TypeB(2)))
        .build()
        .unwrap();
    // Same requested key, different services: independent cache slots.
    assert_eq!(p.get_keyed::<TypeA>("tenant").unwrap().0, 1);
    assert_eq!(p.get_keyed::<TypeB>("tenant").unwrap().0, 2);
    assert!(Arc::ptr_eq(
        &p.get_keyed::<TypeA>("tenant").unwrap(),
        &p.get_keyed::<TypeA>("tenant").unwrap()
    ));
}

#[test]
fn any_key_scoped_caches_per_service_and_key() {
    let p = ServiceCollection::new()
        .keyed_any_scoped::<TypeA>(|_, _| Arc::new(TypeA(1)))
        .keyed_any_scoped::<TypeB>(|_, _| Arc::new(TypeB(2)))
        .build()
        .unwrap();
    let scope = p.scope();
    assert_eq!(scope.get_keyed::<TypeA>("tenant").unwrap().0, 1);
    assert_eq!(scope.get_keyed::<TypeB>("tenant").unwrap().0, 2);
}

#[test]
fn any_key_validation_consumers_of_different_services_same_key() {
    struct C1(#[allow(dead_code)] Arc<TypeA>);
    struct C2(#[allow(dead_code)] Arc<TypeB>);

    let provider = ServiceCollection::new()
        .keyed_any_singleton::<TypeA>(|_, _| Arc::new(TypeA(1)))
        .keyed_any_singleton::<TypeB>(|_, _| Arc::new(TypeB(2)))
        .singleton(|r| {
            let any = r
                .get_keyed_any(std::any::type_name::<TypeA>(), "tenant")
                .expect("TypeA fallback");
            let arc = any.downcast::<Arc<TypeA>>().unwrap();
            Arc::new(C1(Arc::clone(&*arc)))
        })
        .singleton(|r| {
            let any = r
                .get_keyed_any(std::any::type_name::<TypeB>(), "tenant")
                .expect("TypeB fallback");
            let arc = any.downcast::<Arc<TypeB>>().unwrap();
            Arc::new(C2(Arc::clone(&*arc)))
        })
        .build()
        .unwrap();
    assert!(Arc::ptr_eq(
        &provider.get::<C1>().unwrap().0,
        &provider.get_keyed::<TypeA>("tenant").unwrap()
    ));
    assert!(Arc::ptr_eq(
        &provider.get::<C2>().unwrap().0,
        &provider.get_keyed::<TypeB>("tenant").unwrap()
    ));
}

#[test]
fn any_key_scoped_owned_is_allowed_at_root() {
    // `get_owned`/`get_keyed_owned` are the documented strict-scope exception.
    struct ScopedConn;
    let p = ServiceCollection::new()
        .keyed_any_scoped::<ScopedConn>(|_, _| Arc::new(ScopedConn))
        .build()
        .unwrap();
    let owned: ScopedConn = p.get_keyed_owned("x").unwrap();
    let _ = owned;
    // Exact-key scoped owned is also allowed at the root.
    let p2 = ServiceCollection::new()
        .keyed_scoped::<ScopedConn>("x", |_| Arc::new(ScopedConn))
        .build()
        .unwrap();
    let _owned2: ScopedConn = p2.get_keyed_owned("x").unwrap();
}

#[test]
fn any_key_nested_resolution_of_different_key_is_not_a_cycle() {
    struct Wrap(#[allow(dead_code)] Option<Arc<Wrap>>);
    let p = ServiceCollection::new()
        .keyed_any_transient::<Wrap>(|r, key| {
            if key.to_string() == "outer" {
                let inner = r
                    .get_keyed_any(std::any::type_name::<Wrap>(), "inner")
                    .expect("different requested key is not a cycle");
                let arc = inner.downcast::<Arc<Wrap>>().unwrap();
                Arc::new(Wrap(Some(Arc::clone(&*arc))))
            } else {
                Arc::new(Wrap(None))
            }
        })
        .build()
        .unwrap();
    assert!(p.get_keyed::<Wrap>("outer").is_ok());
}

#[derive(rust_dix_macros::Inject)]
struct VecAsyncConsumer {
    #[inject]
    #[allow(dead_code)]
    deps: Vec<Arc<AsyncS>>,
}

#[derive(Debug)]
struct AsyncS;

#[test]
fn vec_injection_includes_initialized_async_singleton() {
    let provider = futures::executor::block_on(
        ServiceCollection::new()
            .async_singleton(|_| Box::pin(async { Arc::new(AsyncS) }))
            .transient(|r| __rdi_construct_VecAsyncConsumer(r))
            .build_async(),
    )
    .unwrap();
    let consumer: Arc<VecAsyncConsumer> = provider.get().unwrap();
    assert_eq!(
        consumer.deps.len(),
        1,
        "initialized async singleton is included"
    );
}

#[test]
fn any_key_dependency_is_resolved_during_validation() {
    struct Consumer(#[allow(dead_code)] Arc<Conn>);

    let provider = ServiceCollection::new()
        .keyed_any_transient::<Conn>(|_, _| Arc::new(Conn("validated")))
        .singleton(|r| {
            let any = r
                .get_keyed_any(std::any::type_name::<Conn>(), "dynamic")
                .expect("AnyKey fallback must resolve during validation");
            let arc = any.downcast::<Arc<Conn>>().unwrap();
            Arc::new(Consumer(Arc::clone(&*arc)))
        })
        .build()
        .unwrap();
    assert!(provider.get::<Consumer>().is_ok());
}

#[test]
fn singleton_depending_on_any_key_scoped_service_is_captive() {
    // A singleton must not capture a scoped AnyKey fallback: build-time
    // validation reports the captive dependency instead of silently building it.
    struct ScopedConn;
    struct SingletonConsumer(#[allow(dead_code)] Arc<ScopedConn>);

    let result = ServiceCollection::new()
        .keyed_any_scoped::<ScopedConn>(|_, _| Arc::new(ScopedConn))
        .singleton(|r| {
            let any = r
                .get_keyed_any(std::any::type_name::<ScopedConn>(), "dynamic")
                .expect("scoped AnyKey resolves during validation");
            let arc = any.downcast::<Arc<ScopedConn>>().unwrap();
            Arc::new(SingletonConsumer(Arc::clone(&*arc)))
        })
        .build();
    match result {
        Err(RdiError::SingletonDependsOnScoped { .. }) => {}
        Ok(_) => panic!("captive dependency must be rejected"),
        Err(other) => panic!("expected SingletonDependsOnScoped, got {other:?}"),
    }
}
