//! Regression tests for the 0.8.0 release-verification findings:
//!
//! 1. `get_async` must fall back to the synchronous path for sync-registered
//!    Scoped services (not `ServiceNotFound`).
//! 2. `IServiceResolver::try_get` / `try_get_keyed` must return `None` (not
//!    panic) for scoped services under root scope validation.
//! 3. Async Scoped self/cross recursion must return `CircularDependency`
//!    instead of dead-locking on the per-key async cache lock.
//! 4. Async singleton resolution during `build_async` must produce an
//!    actionable error when a dependency is registered later.
//! 5. Disposal keeps releasing instances/config when a user `Drop` panics.

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct SyncScoped(i32);

#[test]
fn get_async_falls_back_to_sync_scoped() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(SyncScoped(5)))
        .build()
        .unwrap();
    let scope = p.scope();
    let svc: Arc<SyncScoped> = futures::executor::block_on(scope.get_async()).unwrap();
    assert_eq!(svc.0, 5);
    // Cached like a normal scoped service.
    let again: Arc<SyncScoped> = futures::executor::block_on(scope.get_async()).unwrap();
    assert!(Arc::ptr_eq(&svc, &again));
}

#[test]
fn get_async_keyed_falls_back_to_sync_scoped() {
    let p = ServiceCollection::new()
        .keyed_scoped("k", |_| Arc::new(SyncScoped(9)))
        .build()
        .unwrap();
    let scope = p.scope();
    let svc: Arc<SyncScoped> = futures::executor::block_on(scope.get_keyed_async("k")).unwrap();
    assert_eq!(svc.0, 9);
}

#[test]
fn get_async_still_rejects_scoped_at_root() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(SyncScoped(1)))
        .build()
        .unwrap();
    match futures::executor::block_on(p.get_async::<SyncScoped>()) {
        Err(RdiError::ScopedFromRootProvider { .. }) => {}
        other => panic!("expected ScopedFromRootProvider, got {other:?}"),
    }
}

#[test]
fn try_get_returns_none_for_strict_root_scoped() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(SyncScoped(1)))
        .keyed_scoped("k", |_| Arc::new(SyncScoped(2)))
        .build()
        .unwrap();

    assert!(IServiceResolver::try_get::<SyncScoped>(&*p).is_none());
    assert!(IServiceResolver::try_get_keyed::<SyncScoped>(&*p, "k").is_none());
    // Same contract on a scope for a disposed/missing lookup.
    let scope = p.scope();
    assert!(IServiceResolver::try_get::<SyncScoped>(&scope).is_some());
    scope.dispose();
    assert!(IServiceResolver::try_get::<SyncScoped>(&scope).is_none());
}

#[test]
fn async_scoped_self_recursion_is_detected() {
    struct Recursive;
    let cycle_detected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = Arc::clone(&cycle_detected);

    let p = futures::executor::block_on(
        ServiceCollection::new()
            .async_scoped(move |sp| {
                let seen = Arc::clone(&seen);
                Box::pin(async move {
                    // Re-enter the service currently being resolved: must return
                    // an error instead of waiting forever on its own cache lock.
                    let nested = sp.get_async::<Recursive>().await;
                    if matches!(nested, Err(RdiError::CircularDependency(_))) {
                        seen.store(true, Ordering::SeqCst);
                    }
                    Arc::new(Recursive)
                })
            })
            .build_async(),
    )
    .unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let scope = p.scope();
    let done = Arc::clone(&cycle_detected);
    std::thread::spawn(move || {
        let _ = futures::executor::block_on(scope.get_async::<Recursive>());
        let _ = tx.send(done.load(Ordering::SeqCst));
    });
    let detected = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("async recursion must not hang");
    assert!(detected, "nested resolution must report CircularDependency");
}

#[test]
fn async_scoped_cross_recursion_is_detected() {
    struct A;
    struct B;
    let cycle_detected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen_a = Arc::clone(&cycle_detected);
    let seen_b = Arc::clone(&cycle_detected);

    let p = futures::executor::block_on(
        ServiceCollection::new()
            .async_scoped(move |sp| {
                let seen = Arc::clone(&seen_a);
                Box::pin(async move {
                    let _ = sp.get_async::<B>().await;
                    // A→B→A: the inner A resolution is the cycle.
                    if seen.load(Ordering::SeqCst) {
                        std::hint::black_box(&seen);
                    }
                    Arc::new(A)
                })
            })
            .async_scoped(move |sp| {
                let seen = Arc::clone(&seen_b);
                Box::pin(async move {
                    let nested = sp.get_async::<A>().await;
                    if matches!(nested, Err(RdiError::CircularDependency(_))) {
                        seen.store(true, Ordering::SeqCst);
                    }
                    Arc::new(B)
                })
            })
            .build_async(),
    )
    .unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let scope = p.scope();
    let done = Arc::clone(&cycle_detected);
    std::thread::spawn(move || {
        let _ = futures::executor::block_on(scope.get_async::<A>());
        let _ = tx.send(done.load(Ordering::SeqCst));
    });
    let detected = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("async recursion must not hang");
    assert!(detected, "cross recursion must report CircularDependency");
}

#[test]
fn async_singleton_forward_dependency_reports_actionable_error() {
    #[derive(Debug)]
    struct Consumer(#[allow(dead_code)] Arc<LateDep>);
    #[derive(Debug)]
    struct LateDep;

    // Consumer is registered BEFORE its async singleton dependency; async
    // singletons initialize in registration order, so this must fail with a
    // clear message rather than a misleading sync-build panic.
    let result = futures::executor::block_on(
        ServiceCollection::new()
            .async_singleton(|sp| {
                Box::pin(async move {
                    let dep = sp.get_async::<LateDep>().await;
                    Arc::new(Consumer(dep.expect("dependency")))
                })
            })
            .async_singleton(|_| Box::pin(async { Arc::new(LateDep) }))
            .build_async(),
    );
    match result {
        Err(RdiError::FactoryPanic { payload, .. }) => {
            assert!(payload.contains("registration order"), "payload={payload}");
        }
        Ok(_) => panic!("forward async-singleton dependency must fail"),
        Err(other) => panic!("expected FactoryPanic, got {other:?}"),
    }
}

#[test]
fn dispose_continues_after_instance_drop_panic() {
    static NORMAL_DROPS: AtomicUsize = AtomicUsize::new(0);
    struct PanicsOnDrop;
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("instance drop boom");
        }
    }
    struct Normal;
    impl Drop for Normal {
        fn drop(&mut self) {
            NORMAL_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(PanicsOnDrop))
        .singleton(|_| Arc::new(Normal))
        .build()
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.dispose()));
    assert!(result.is_err(), "first instance Drop panic is re-raised");
    assert_eq!(
        NORMAL_DROPS.load(Ordering::SeqCst),
        1,
        "remaining instances are still released"
    );
}

/// Legitimate concurrent resolution of the same async Scoped service (single
/// flight via the per-key async lock) must not be reported as a cycle. Guards
/// are scoped to a single poll, so interleaved tasks cannot see each other.
#[tokio::test(flavor = "current_thread")]
async fn concurrent_same_async_scoped_resolution_is_not_a_cycle() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    struct Slow;

    let p = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(Slow)
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = p.scope();
    let (a, b) = tokio::join!(scope.get_async::<Slow>(), scope.get_async::<Slow>());
    assert!(a.is_ok(), "first resolution failed: {:?}", a.err());
    assert!(b.is_ok(), "second resolution failed: {:?}", b.err());
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "the factory must run exactly once (single flight)"
    );
    assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
}

/// Concurrent resolution of a DAG where two async Scoped services share one
/// async Scoped dependency must succeed (shared in-flight dependency is not a
/// cycle).
#[tokio::test(flavor = "current_thread")]
async fn concurrent_async_dag_with_shared_dependency_is_not_a_cycle() {
    static DEP_CALLS: AtomicUsize = AtomicUsize::new(0);
    struct Dep;
    struct Left(#[allow(dead_code)] Arc<Dep>);
    struct Right(#[allow(dead_code)] Arc<Dep>);

    let p = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(15)).await;
                DEP_CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(Dep)
            })
        })
        .async_scoped(|sp| {
            Box::pin(async move {
                let dep = sp.get_async::<Dep>().await.unwrap();
                Arc::new(Left(dep))
            })
        })
        .async_scoped(|sp| {
            Box::pin(async move {
                let dep = sp.get_async::<Dep>().await.unwrap();
                Arc::new(Right(dep))
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = p.scope();
    let (l, r) = tokio::join!(scope.get_async::<Left>(), scope.get_async::<Right>());
    assert!(l.is_ok(), "Left failed: {:?}", l.err());
    assert!(r.is_ok(), "Right failed: {:?}", r.err());
    assert_eq!(DEP_CALLS.load(Ordering::SeqCst), 1, "shared dep built once");
}

/// Multi-threaded runtime: tasks may be polled on different worker threads.
/// Per-poll guards must not leave cross-task/thread state behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_async_resolution_across_tasks_is_not_a_cycle() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    struct S;

    let p = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(S)
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = Arc::new(p.scope());
    let mut handles = Vec::new();
    for _ in 0..16 {
        let scope = Arc::clone(&scope);
        handles.push(tokio::spawn(async move {
            scope
                .get_async::<S>()
                .await
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        }));
    }
    for handle in handles {
        let result = handle.await.expect("join");
        assert!(result.is_ok(), "task failed: {:?}", result.err());
    }
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "factory runs once across worker threads"
    );
}
