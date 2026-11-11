//! 0.9 async context & diagnostics tests.

use rust_dix::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_context_detects_cycle_across_spawned_task() {
    struct SelfRef;
    let detected = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&detected);

    let build = async move {
        ServiceCollection::new()
            .async_singleton(move |sp| {
                let seen = Arc::clone(&seen);
                Box::pin(async move {
                    let ctx = sp.async_context().expect("built provider");
                    let handle = tokio::spawn(async move {
                        let nested = ctx.get_async::<SelfRef>().await;
                        if matches!(nested, Err(RdiError::CircularDependency(_))) {
                            seen.store(true, Ordering::SeqCst);
                        }
                    });
                    handle.await.expect("spawned task");
                    Arc::new(SelfRef)
                })
            })
            .build_async()
            .await
    };
    let result = tokio::time::timeout(TIMEOUT, build)
        .await
        .expect("cross-task cycle detection must not dead-lock");
    assert!(result.is_ok(), "factory handled the cycle error");
    assert!(
        detected.load(Ordering::SeqCst),
        "nested resolution must report CircularDependency"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn async_context_resolves_keyed_and_typed_keys() {
    #[derive(Debug, PartialEq, Eq, Hash)]
    enum Key {
        A,
    }
    struct Dep;
    struct Consumer(#[allow(dead_code)] Arc<Dep>);

    let provider = ServiceCollection::new()
        .async_keyed_singleton(ServiceKey::new(Key::A), |_| {
            Box::pin(async { Arc::new(Dep) })
        })
        .async_singleton(|_| {
            Box::pin(async {
                // Context is available for users; resolution through it works.
                Arc::new(Consumer(Arc::new(Dep)))
            })
        })
        .build_async()
        .await
        .unwrap();

    let ctx = provider.async_context().expect("provider");
    let dep: Arc<Dep> = ctx.get_keyed_async(ServiceKey::new(Key::A)).await.unwrap();
    let again: Arc<Dep> = provider
        .get_keyed_async::<Dep>(ServiceKey::new(Key::A))
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&dep, &again));
}

#[tokio::test(flavor = "current_thread")]
async fn async_context_is_cloneable_and_exposes_provider() {
    let provider = ServiceCollection::new().build_async().await.unwrap();
    let ctx = provider.async_context().expect("provider");
    let clone = ctx.clone();
    assert!(Arc::ptr_eq(ctx.provider(), clone.provider()));
}

#[tokio::test(flavor = "current_thread")]
async fn sync_singleton_depending_on_async_service_is_reported_precisely() {
    struct AsyncDep;
    struct SyncConsumer(#[allow(dead_code)] Arc<AsyncDep>);

    let result = ServiceCollection::new()
        .async_singleton(|_| Box::pin(async { Arc::new(AsyncDep) }))
        .singleton(|r| {
            let any = r
                .get_any(std::any::type_name::<AsyncDep>())
                .expect("resolution returns Arc<Arc<T>> wrapper");
            let dep = any.downcast::<Arc<AsyncDep>>().unwrap();
            Arc::new(SyncConsumer(Arc::clone(&*dep)))
        })
        .build_async()
        .await;

    match result {
        Err(RdiError::SyncSingletonDependsOnAsyncService {
            singleton,
            async_service,
        }) => {
            assert!(singleton.contains("SyncConsumer"), "singleton={singleton}");
            assert!(
                async_service.contains("AsyncDep"),
                "async_service={async_service}"
            );
        }
        Ok(_) => panic!("sync->async dependency must fail"),
        Err(other) => panic!("expected SyncSingletonDependsOnAsyncService, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn sync_get_on_uninitialized_async_service_reports_get_async_requirement() {
    struct AsyncTransient;
    let provider = ServiceCollection::new()
        .async_transient(|_| Box::pin(async { Arc::new(AsyncTransient) }))
        .build_async()
        .await
        .unwrap();

    match provider.get::<AsyncTransient>() {
        Err(RdiError::AsyncServiceRequiresGetAsync { service }) => {
            assert!(service.contains("AsyncTransient"));
        }
        Ok(_) => panic!("sync get of an async service must fail"),
        Err(other) => panic!("expected AsyncServiceRequiresGetAsync, got {other:?}"),
    }
    // The async path works.
    let _: Arc<AsyncTransient> = provider.get_async().await.unwrap();
}

#[test]
fn is_circular_detects_direct_and_wrapped_cycles() {
    let direct = RdiError::CircularDependency("A -> B -> A".into());
    assert!(direct.is_circular());

    let wrapped = RdiError::FactoryPanic {
        service: "A".into(),
        payload: "called `Result::unwrap()` on an `Err` value: CircularDependency(\"A -> A\")"
            .into(),
    };
    assert!(wrapped.is_circular());

    assert!(!RdiError::ProviderDisposed.is_circular());
    assert!(!RdiError::not_found("X", None).is_circular());
}

#[test]
fn sync_cycle_is_circular() {
    struct A;
    struct B;
    let result = ServiceCollection::new()
        .singleton(|r| {
            let _ = r.get_any(std::any::type_name::<B>());
            Arc::new(A)
        })
        .singleton(|r| {
            let _ = r.get_any(std::any::type_name::<A>());
            Arc::new(B)
        })
        .build();
    match result {
        Err(err) => assert!(err.is_circular(), "got {err:?}"),
        Ok(_) => panic!("cycle must fail"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn async_self_cycle_reports_circular_via_is_circular() {
    struct SelfRef;
    let result = ServiceCollection::new()
        .async_singleton(|sp| {
            Box::pin(async move {
                // Forward the nested error by unwrapping it (user mistake).
                let _ = sp.get_async::<SelfRef>().await.unwrap();
                Arc::new(SelfRef)
            })
        })
        .build_async()
        .await;
    match result {
        Err(err) => assert!(err.is_circular(), "got {err:?}"),
        Ok(_) => panic!("cycle must fail"),
    }
}
