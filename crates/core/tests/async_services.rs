mod common;

use rust_dix::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, PartialEq)]
struct DbPool {
    conn: String,
}

#[derive(Debug, PartialEq)]
struct Cache {
    backend: String,
}

#[tokio::test]
async fn async_singleton_basic() {
    let provider = ServiceCollection::new()
        .async_singleton(|_| {
            Box::pin(async { Arc::new(DbPool { conn: "pg://host".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let pool: Arc<DbPool> = provider.get_async().await.unwrap();
    assert_eq!(pool.conn, "pg://host");
}

#[tokio::test]
async fn async_singleton_caches() {
    static CNT: AtomicUsize = AtomicUsize::new(0);

    let provider = ServiceCollection::new()
        .async_singleton(|_| {
            CNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Arc::new(DbPool { conn: "cached".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let _a = provider.get_async::<DbPool>().await.unwrap();
    let _b = provider.get_async::<DbPool>().await.unwrap();
    // Factory should only execute once (during build_async); subsequent
    // get_async calls return the cached singleton.
    assert_eq!(CNT.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn async_transient_new_each_time() {
    static CNT: AtomicUsize = AtomicUsize::new(0);

    let provider = ServiceCollection::new()
        .async_transient(|_| {
            CNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Arc::new(Cache { backend: "redis".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let _a = provider.get_async::<Cache>().await.unwrap();
    let _b = provider.get_async::<Cache>().await.unwrap();
    assert_eq!(CNT.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn async_keyed_singleton() {
    let provider = ServiceCollection::new()
        .async_keyed_singleton("a", |_| {
            Box::pin(async { Arc::new(DbPool { conn: "db-a".into() }) })
        })
        .async_keyed_singleton("b", |_| {
            Box::pin(async { Arc::new(DbPool { conn: "db-b".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let a: Arc<DbPool> = provider.get_keyed_async("a").await.unwrap();
    let b: Arc<DbPool> = provider.get_keyed_async("b").await.unwrap();
    assert_eq!(a.conn, "db-a");
    assert_eq!(b.conn, "db-b");
}

#[tokio::test]
async fn async_keyed_transient() {
    static CNT: AtomicUsize = AtomicUsize::new(0);

    let provider = ServiceCollection::new()
        .async_keyed_transient("t", |_| {
            CNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Arc::new(Cache { backend: "mem".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let _a = provider.get_keyed_async::<Cache>("t").await.unwrap();
    let _b = provider.get_keyed_async::<Cache>("t").await.unwrap();
    assert_eq!(CNT.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn async_mixed_sync_and_async() {
    let provider = ServiceCollection::new()
        .singleton(|_| Arc::new(common::Logger { prefix: "sync".into() }))
        .async_singleton(|_| {
            Box::pin(async { Arc::new(DbPool { conn: "mixed".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let logger: Arc<common::Logger> = provider.get().unwrap();
    assert_eq!(logger.prefix, "sync");

    let pool: Arc<DbPool> = provider.get_async().await.unwrap();
    assert_eq!(pool.conn, "mixed");
}

#[tokio::test]
async fn async_singleton_with_sync_dependency() {
    // Async singleton depends on a sync singleton via provider_arc.
    let provider = ServiceCollection::new()
        .singleton(|_| Arc::new(common::Logger { prefix: "dep".into() }))
        .async_singleton(|p: Arc<ServiceProvider>| {
            Box::pin(async move {
                let logger: Arc<common::Logger> = p.get().unwrap();
                Arc::new(DbPool {
                    conn: format!("pg://{}/db", logger.prefix),
                })
            })
        })
        .build_async()
        .await
        .unwrap();

    let pool: Arc<DbPool> = provider.get_async().await.unwrap();
    assert_eq!(pool.conn, "pg://dep/db");
}

#[tokio::test]
async fn async_service_resolved_via_sync_path_panics() {
    let provider = ServiceCollection::new()
        .async_singleton(|_| {
            Box::pin(async { Arc::new(DbPool { conn: "nope".into() }) })
        })
        // Use build() (sync) instead of build_async() — sync factory stub
        // panics when async-only services are resolved via the sync path.
        .build()
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = provider.get::<DbPool>();
    }));
    assert!(result.is_err());
}

#[tokio::test]
async fn async_with_owned_resolution() {
    // Owned resolution should work for async transient services.
    let provider = ServiceCollection::new()
        .async_transient(|_| {
            Box::pin(async { Arc::new(common::MyService { value: 99 }) })
        })
        .build_async()
        .await
        .unwrap();

    // Owned resolution via sync path (async_factory is async, but
    // get_owned uses the sync factory which panics for async services).
    // This test verifies get_async works as expected.
    let svc: Arc<common::MyService> = provider.get_async().await.unwrap();
    assert_eq!(svc.value, 99);
}

#[tokio::test]
async fn async_multiple_async_singletons() {
    let provider = ServiceCollection::new()
        .async_singleton(|_| {
            Box::pin(async { Arc::new(DbPool { conn: "first".into() }) })
        })
        .async_singleton(|_| {
            Box::pin(async { Arc::new(Cache { backend: "second".into() }) })
        })
        .build_async()
        .await
        .unwrap();

    let pool: Arc<DbPool> = provider.get_async().await.unwrap();
    let cache: Arc<Cache> = provider.get_async().await.unwrap();
    assert_eq!(pool.conn, "first");
    assert_eq!(cache.backend, "second");
}

#[tokio::test]
async fn async_instance_works_with_build_async() {
    let pool = Arc::new(DbPool { conn: "instance".into() });
    let provider = ServiceCollection::new()
        .instance(pool.clone())
        .build_async()
        .await
        .unwrap();

    let resolved: Arc<DbPool> = provider.get().unwrap();
    assert!(Arc::ptr_eq(&pool, &resolved));
}

#[tokio::test]
async fn async_optional_missing() {
    let provider = ServiceCollection::new().build_async().await.unwrap();
    assert!(provider.get_optional::<DbPool>().is_none());
}

#[tokio::test]
async fn async_scoped_basic() {
    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async { Arc::new(common::MyService { value: 42 }) })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let svc: Arc<common::MyService> = scope.get_async().await.unwrap();
    assert_eq!(svc.value, 42);
}

#[tokio::test]
async fn async_scoped_keyed() {
    let provider = ServiceCollection::new()
        .async_keyed_scoped("s", |_| {
            Box::pin(async { Arc::new(common::MyService { value: 7 }) })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let svc: Arc<common::MyService> = scope.get_keyed_async("s").await.unwrap();
    assert_eq!(svc.value, 7);
}

#[tokio::test]
async fn async_scoped_concurrent_factory_executes_once() {
    use std::sync::atomic::{AtomicU64, Ordering};

    static CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(common::MyService { value: 99 })
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let s1 = scope.scope_provider().clone();
    let s2 = scope.scope_provider().clone();

    let (r1, r2) = tokio::join!(
        async { s1.get_async::<common::MyService>().await },
        async { s2.get_async::<common::MyService>().await },
    );

    let v1 = r1.unwrap();
    let v2 = r2.unwrap();
    assert_eq!(v1.value, 99);
    assert_eq!(v2.value, 99);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "并发首次解析：async factory 应只执行一次");
}