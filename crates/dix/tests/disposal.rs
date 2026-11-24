//! 0.8.0 deterministic disposal / lifecycle ownership.
//!
//! - `Scope::dispose` (and `Drop`) releases scoped instances in **reverse
//!   creation order**, then runs `register_disposable` cleanups in reverse
//!   registration order.
//! - `ServiceProvider::dispose` does the same for singletons + root-scoped
//!   instances, clears named services, and marks the tree disposed.
//! - Disposed containers/scopes reject resolution (`ProviderDisposed` /
//!   `ScopeDisposed`); infallible lookups return `None`/empty.
//! - `get_owned` instances are caller-owned and are not tracked.

use rust_dix::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[test]
fn scope_dispose_releases_scoped_instances_in_reverse_creation_order() {
    static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Tracked(&'static str);
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.lock().unwrap().push(self.0);
        }
    }
    struct ScopedA(#[allow(dead_code)] Arc<Tracked>);
    struct ScopedB(#[allow(dead_code)] Arc<Tracked>);

    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedA(Arc::new(Tracked("a")))))
        .scoped(|_| Arc::new(ScopedB(Arc::new(Tracked("b")))))
        .build()
        .unwrap();

    let scope = p.scope();
    // Resolve in creation order A, B; then drop the caller handles so the scope
    // cache owns the instances.
    let a = scope.get::<ScopedA>().unwrap();
    let b = scope.get::<ScopedB>().unwrap();
    drop(a);
    drop(b);
    assert!(DROPS.lock().unwrap().is_empty(), "instances still cached");

    scope.dispose();
    assert_eq!(
        *DROPS.lock().unwrap(),
        vec!["b", "a"],
        "scoped instances released LIFO"
    );
}

#[test]
fn scope_drop_releases_instances() {
    static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Tracked(&'static str);
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.lock().unwrap().push(self.0);
        }
    }
    struct ScopedA(#[allow(dead_code)] Arc<Tracked>);

    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedA(Arc::new(Tracked("a")))))
        .build()
        .unwrap();
    {
        let scope = p.scope();
        let _ = scope.get::<ScopedA>().unwrap();
    }
    assert_eq!(*DROPS.lock().unwrap(), vec!["a"], "Drop disposes the scope");
}

#[test]
fn cleanups_run_in_reverse_registration_order() {
    static ORDER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    let p = ServiceCollection::new().build().unwrap();
    let scope = p.scope();
    scope
        .scope_provider()
        .register_disposable(Box::new(|| ORDER.lock().unwrap().push(1)));
    scope
        .scope_provider()
        .register_disposable(Box::new(|| ORDER.lock().unwrap().push(2)));
    scope
        .scope_provider()
        .register_disposable(Box::new(|| ORDER.lock().unwrap().push(3)));
    scope.dispose();
    assert_eq!(*ORDER.lock().unwrap(), vec![3, 2, 1]);
}

#[test]
fn dispose_is_idempotent() {
    static CLEANUPS: AtomicUsize = AtomicUsize::new(0);
    let p = ServiceCollection::new().build().unwrap();
    let scope = p.scope();
    scope.scope_provider().register_disposable(Box::new(|| {
        CLEANUPS.fetch_add(1, Ordering::SeqCst);
    }));
    scope.dispose();
    scope.dispose();
    drop(scope);
    assert_eq!(CLEANUPS.load(Ordering::SeqCst), 1, "cleanups run once");
}

#[test]
fn disposed_scope_rejects_resolution() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(1)))
        .build()
        .unwrap();
    let scope = p.scope();
    assert!(scope.get::<ScopedSvc>().is_ok());
    scope.dispose();
    assert!(matches!(
        scope.get::<ScopedSvc>(),
        Err(RdiError::ScopeDisposed)
    ));
    assert!(scope.get_optional::<ScopedSvc>().is_none());
    assert!(scope.get_all::<ScopedSvc>().is_empty());
}

#[test]
fn provider_dispose_blocks_existing_scopes() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedSvc(1)))
        .build()
        .unwrap();
    let scope = p.scope();
    p.dispose();
    assert!(matches!(
        scope.get::<ScopedSvc>(),
        Err(RdiError::ProviderDisposed)
    ));
    assert!(matches!(
        p.get::<ScopedSvc>(),
        Err(RdiError::ProviderDisposed)
    ));
    assert!(p.get_optional::<ScopedSvc>().is_none());
    assert!(p.get_all::<ScopedSvc>().is_empty());
}

#[test]
fn provider_dispose_releases_singletons_reverse_order() {
    static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Tracked(&'static str);
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.lock().unwrap().push(self.0);
        }
    }
    struct First(#[allow(dead_code)] Arc<Tracked>);
    struct Second(#[allow(dead_code)] Arc<Tracked>);

    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(First(Arc::new(Tracked("first")))))
        .singleton(|_| Arc::new(Second(Arc::new(Tracked("second")))))
        .build()
        .unwrap();
    p.dispose();
    assert_eq!(*DROPS.lock().unwrap(), vec!["second", "first"]);
}

#[test]
fn lenient_root_scoped_instances_are_released_on_provider_dispose() {
    static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Tracked(&'static str);
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.lock().unwrap().push(self.0);
        }
    }
    struct ScopedA(#[allow(dead_code)] Arc<Tracked>);
    struct ScopedB(#[allow(dead_code)] Arc<Tracked>);

    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedA(Arc::new(Tracked("a")))))
        .scoped(|_| Arc::new(ScopedB(Arc::new(Tracked("b")))))
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();
    let a = p.get::<ScopedA>().unwrap();
    let b = p.get::<ScopedB>().unwrap();
    drop(a);
    drop(b);
    p.dispose();
    assert_eq!(*DROPS.lock().unwrap(), vec!["b", "a"]);
}

#[test]
fn owned_instances_are_not_tracked_or_disposed() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Owned;
    impl Drop for Owned {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(Owned))
        .build()
        .unwrap();
    let scope = p.scope();
    let owned: Owned = scope.get_owned().unwrap();
    scope.dispose();
    // Still alive after scope disposal: caller-owned.
    drop(owned);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn named_services_are_cleared_on_provider_dispose() {
    let p = ServiceCollection::new().build().unwrap();
    p.register_named("plugin", Arc::new(ScopedSvc(1)));
    assert!(p.get_named_any("plugin").is_some());
    p.dispose();
    assert!(p.get_named_any("plugin").is_none());
}

#[test]
fn is_registered_remains_a_registration_query_after_dispose() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ScopedSvc(1)))
        .build()
        .unwrap();
    p.dispose();
    assert!(p.is_registered::<ScopedSvc>());
    assert!(!p.is_registered::<String>());
}

#[tokio::test]
async fn async_scoped_instances_are_disposed_by_scope() {
    static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Tracked(&'static str);
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.lock().unwrap().push(self.0);
        }
    }
    struct AsyncScoped(#[allow(dead_code)] Arc<Tracked>);

    let p = ServiceCollection::new()
        .async_scoped(|_| Box::pin(async { Arc::new(AsyncScoped(Arc::new(Tracked("async")))) }))
        .build_async()
        .await
        .unwrap();
    {
        let scope = p.scope();
        let svc: Arc<AsyncScoped> = scope.get_async().await.unwrap();
        drop(svc);
        scope.dispose();
    }
    assert_eq!(*DROPS.lock().unwrap(), vec!["async"]);
}

#[test]
fn panicking_instance_drop_during_unwind_does_not_abort() {
    struct PanicsOnDrop;
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("instance drop during unwind");
        }
    }
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(PanicsOnDrop))
        .build()
        .unwrap();

    // Unwinding through `Scope::drop` must not double-panic (abort): the
    // disposal path catches instance `Drop` panics while the thread unwinds.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let scope = p.scope();
        let _svc: Arc<PanicsOnDrop> = scope.get().unwrap();
        panic!("outer panic");
    }));
    assert!(result.is_err());
}

#[derive(Debug)]
#[allow(dead_code)]
struct ScopedSvc(i32);
