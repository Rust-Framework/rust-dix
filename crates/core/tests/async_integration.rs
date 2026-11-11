//! Complex async integration test — e-commerce order processing scenario.
//!
//! Models a realistic web application with:
//! - Async infrastructure (DB pool, Redis cache, payment gateways)
//! - Sync business services (config, repositories, order processing)
//! - Per-request scope lifecycle (UserRepository scoped, OrderService transient)
//! - Cross-cutting dependency chains across sync/async boundaries

mod common;

use rust_dix::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ── Infrastructure (async) ──────────────────────────────────────────────

struct DbPool {
    conn_str: String,
    pool_id: u64,
}

struct RedisCache {
    url: String,
    cache_id: u64,
}

// ── Business traits ─────────────────────────────────────────────────────

trait IPaymentGateway: Send + Sync {
    fn name(&self) -> &str;
    fn charge(&self, amount: u64) -> bool;
}

struct WechatPay {
    #[allow(dead_code)]
    merchant_id: String,
}
impl IPaymentGateway for WechatPay {
    fn name(&self) -> &str {
        "wechat"
    }
    fn charge(&self, _amount: u64) -> bool {
        true
    }
}

struct Alipay {
    #[allow(dead_code)]
    app_id: String,
}
impl IPaymentGateway for Alipay {
    fn name(&self) -> &str {
        "alipay"
    }
    fn charge(&self, _amount: u64) -> bool {
        true
    }
}

// ── Sync business services ──────────────────────────────────────────────

#[derive(Clone)]
struct AppConfig {
    max_retries: u32,
    payment_provider: String,
}

/// Per-request scoped: depends on async singletons (DbPool, RedisCache)
/// and sync singleton (AppConfig). Resolved via sync path because
/// async singletons are cached during build_async.
struct UserRepository {
    db: Arc<DbPool>,
    cache: Arc<RedisCache>,
    config: Arc<AppConfig>,
}

/// Fresh each resolution: depends on scoped UserRepository.
struct OrderService {
    repo: Arc<UserRepository>,
}

/// Transient: depends on sync singleton AppConfig.
struct NotificationService {
    config: Arc<AppConfig>,
}

// ── The integration test ────────────────────────────────────────────────

#[tokio::test]
async fn ecommerce_order_processing_full_lifecycle() {
    static NEXT_POOL_ID: AtomicU64 = AtomicU64::new(1);
    static NEXT_CACHE_ID: AtomicU64 = AtomicU64::new(1);
    static REPO_BUILDS: AtomicU64 = AtomicU64::new(0);
    static ORDER_BUILDS: AtomicU64 = AtomicU64::new(0);

    // ── Build phase ─────────────────────────────────────────────────
    let provider = ServiceCollection::new()
        // Async infrastructure — initialized during build_async
        .async_singleton(|_| {
            let id = NEXT_POOL_ID.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Arc::new(DbPool {
                    conn_str: format!("postgres://db{}/mydb", id),
                    pool_id: id,
                })
            })
        })
        .async_singleton(|_| {
            let id = NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Arc::new(RedisCache {
                    url: format!("redis://cache{}/0", id),
                    cache_id: id,
                })
            })
        })
        // Async keyed services — multiple payment gateways
        .async_keyed_singleton::<dyn IPaymentGateway>("wechat", |_| {
            Box::pin(async {
                Arc::new(WechatPay {
                    merchant_id: "mch_123".into(),
                }) as Arc<dyn IPaymentGateway>
            })
        })
        .async_keyed_singleton::<dyn IPaymentGateway>("alipay", |_| {
            Box::pin(async {
                Arc::new(Alipay {
                    app_id: "app_456".into(),
                }) as Arc<dyn IPaymentGateway>
            })
        })
        // Sync singletons
        .singleton(|_| {
            Arc::new(AppConfig {
                max_retries: 3,
                payment_provider: "wechat".into(),
            })
        })
        // Scoped: one UserRepository per request scope
        .scoped(|r| {
            REPO_BUILDS.fetch_add(1, Ordering::SeqCst);
            let db: Arc<DbPool> = r
                .get_any(std::any::type_name::<DbPool>())
                .and_then(|a| a.downcast::<Arc<DbPool>>().ok().map(|d| Arc::clone(&*d)))
                .expect("DbPool not resolved");
            let cache: Arc<RedisCache> = r
                .get_any(std::any::type_name::<RedisCache>())
                .and_then(|a| {
                    a.downcast::<Arc<RedisCache>>()
                        .ok()
                        .map(|d| Arc::clone(&*d))
                })
                .expect("RedisCache not resolved");
            let config: Arc<AppConfig> = r
                .get_any(std::any::type_name::<AppConfig>())
                .and_then(|a| a.downcast::<Arc<AppConfig>>().ok().map(|d| Arc::clone(&*d)))
                .expect("AppConfig not resolved");
            Arc::new(UserRepository { db, cache, config })
        })
        // Transient: fresh OrderService each resolution
        .transient(|r| {
            ORDER_BUILDS.fetch_add(1, Ordering::SeqCst);
            let repo: Arc<UserRepository> = r
                .get_any(std::any::type_name::<UserRepository>())
                .and_then(|a| {
                    a.downcast::<Arc<UserRepository>>()
                        .ok()
                        .map(|d| Arc::clone(&*d))
                })
                .expect("UserRepository not resolved");
            Arc::new(OrderService { repo })
        })
        .transient(|r| {
            let config: Arc<AppConfig> = r
                .get_any(std::any::type_name::<AppConfig>())
                .and_then(|a| a.downcast::<Arc<AppConfig>>().ok().map(|d| Arc::clone(&*d)))
                .expect("AppConfig not resolved");
            Arc::new(NotificationService { config })
        })
        .build_async()
        .await
        .unwrap();

    // ── Verify build-time async singletons ───────────────────────────
    let db: Arc<DbPool> = provider.get_async().await.unwrap();
    assert_eq!(db.pool_id, 1);
    assert_eq!(db.conn_str, "postgres://db1/mydb");

    let cache: Arc<RedisCache> = provider.get_async().await.unwrap();
    assert_eq!(cache.cache_id, 1);
    assert_eq!(cache.url, "redis://cache1/0");

    let config: Arc<AppConfig> = provider.get().unwrap();
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.payment_provider, "wechat");

    // Async keyed payment gateways
    let wechat: Arc<dyn IPaymentGateway> = provider.get_keyed_async("wechat").await.unwrap();
    assert_eq!(wechat.name(), "wechat");
    assert!(wechat.charge(100));

    let alipay: Arc<dyn IPaymentGateway> = provider.get_keyed_async("alipay").await.unwrap();
    assert_eq!(alipay.name(), "alipay");
    assert!(alipay.charge(200));

    // ── Request scope 1 ──────────────────────────────────────────────
    let scope1 = provider.scope();

    // Resolve scoped UserRepository — factory executes once per scope
    let repo1: Arc<UserRepository> = scope1.get().unwrap();
    assert_eq!(repo1.db.pool_id, 1);
    assert_eq!(repo1.cache.cache_id, 1);
    assert_eq!(repo1.config.max_retries, 3);

    // Same scope: UserRepository is cached
    let repo1_again: Arc<UserRepository> = scope1.get().unwrap();
    assert!(Arc::ptr_eq(&repo1, &repo1_again));
    assert_eq!(REPO_BUILDS.load(Ordering::SeqCst), 1);

    // Transient OrderService: fresh each resolution, but shares UserRepository
    let order1a: Arc<OrderService> = scope1.get().unwrap();
    let order1b: Arc<OrderService> = scope1.get().unwrap();
    assert!(
        !Arc::ptr_eq(&order1a, &order1b),
        "OrderService must be fresh each resolution"
    );
    assert!(
        Arc::ptr_eq(&order1a.repo, &order1b.repo),
        "OrderService must share scoped UserRepository"
    );
    assert_eq!(ORDER_BUILDS.load(Ordering::SeqCst), 2);

    // NotificationService: transient, depends on AppConfig
    let notif1: Arc<NotificationService> = scope1.get().unwrap();
    assert_eq!(notif1.config.max_retries, 3);

    // ── Request scope 2 — independent scoped state ───────────────────
    let scope2 = provider.scope();

    let repo2: Arc<UserRepository> = scope2.get().unwrap();
    assert!(
        !Arc::ptr_eq(&repo1, &repo2),
        "UserRepository must be independent across scopes"
    );
    assert_eq!(REPO_BUILDS.load(Ordering::SeqCst), 2);

    // Scope2's UserRepository still sees the same singletons
    assert_eq!(repo2.db.pool_id, 1);
    assert_eq!(repo2.cache.cache_id, 1);

    // ── Strict scope validation: scoped cannot be resolved at root ────
    match provider.get::<UserRepository>() {
        Ok(_) => panic!("root provider must reject scoped resolution"),
        Err(RdiError::ScopedFromRootProvider { .. }) => {}
        Err(other) => panic!("expected ScopedFromRootProvider at root, got {other:?}"),
    }
    assert!(
        provider.get_optional::<UserRepository>().is_none(),
        "infallible lookups treat scoped-at-root as not resolvable"
    );
    assert_eq!(REPO_BUILDS.load(Ordering::SeqCst), 2);

    // ── Verify singleton identity ────────────────────────────────────
    // All scopes share the same DbPool and RedisCache (singletons)
    let scope3 = provider.scope();
    let repo3: Arc<UserRepository> = scope3.get().unwrap();
    assert!(!Arc::ptr_eq(&repo1, &repo3));
    assert!(!Arc::ptr_eq(&repo2, &repo3));
    assert!(Arc::ptr_eq(&repo1.db, &repo2.db));
    assert!(Arc::ptr_eq(&repo1.db, &repo3.db));
    assert!(Arc::ptr_eq(&repo1.cache, &repo2.cache));
    assert!(Arc::ptr_eq(&repo1.cache, &repo3.cache));

    // ── Verify factory execution counts ──────────────────────────────
    // Async singletons: executed exactly once during build_async
    assert_eq!(
        NEXT_POOL_ID.load(Ordering::SeqCst),
        2,
        "DbPool factory should execute once"
    );
    assert_eq!(
        NEXT_CACHE_ID.load(Ordering::SeqCst),
        2,
        "RedisCache factory should execute once"
    );

    // Scoped UserRepository: once per scope (scope1, scope2, scope3) = 3 times
    assert_eq!(REPO_BUILDS.load(Ordering::SeqCst), 3);

    // Transient OrderService: 2 in scope1
    assert_eq!(ORDER_BUILDS.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn async_build_validates_sync_dependencies() {
    // Even with async build, sync singleton dependency validation still works.
    // Circular dependency should be caught during build_async.

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
        .async_singleton(|_| {
            Box::pin(async {
                Arc::new(DbPool {
                    conn_str: "ok".into(),
                    pool_id: 0,
                })
            })
        })
        .build_async()
        .await;

    assert!(result.is_err());
    if let Err(RdiError::CircularDependency(cycle)) = &result {
        assert!(cycle.contains("A") || cycle.contains("B"));
    } else {
        panic!("expected CircularDependency, got {:?}", result.err());
    }
}

#[tokio::test]
async fn async_captive_dependency_rejected() {
    // Singleton must not depend on Scoped — captive dependency detection
    // still works with async build.

    struct ScopedDep;
    struct BadSingleton {
        _scoped: Arc<ScopedDep>,
    }

    let result = ServiceCollection::new()
        .scoped(|_| Arc::new(ScopedDep))
        .singleton(|r| {
            let dep: Arc<ScopedDep> = r
                .get_any(std::any::type_name::<ScopedDep>())
                .and_then(|a| a.downcast::<Arc<ScopedDep>>().ok().map(|d| Arc::clone(&*d)))
                .unwrap();
            Arc::new(BadSingleton { _scoped: dep })
        })
        .async_singleton(|_| {
            Box::pin(async {
                Arc::new(DbPool {
                    conn_str: "ok".into(),
                    pool_id: 0,
                })
            })
        })
        .build_async()
        .await;

    assert!(result.is_err());
    if let Err(RdiError::SingletonDependsOnScoped { .. }) = &result {
        // expected
    } else {
        panic!("expected SingletonDependsOnScoped, got {:?}", result.err());
    }
}

#[tokio::test]
async fn async_singleton_can_depend_on_sync_singleton() {
    // Async singleton depends on sync singleton — this is the normal case
    // for services like DbPool that need AppConfig for connection strings.

    let provider = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(AppConfig {
                max_retries: 5,
                payment_provider: "alipay".into(),
            })
        })
        .async_singleton(|p: Arc<ServiceProvider>| {
            Box::pin(async move {
                let config: Arc<AppConfig> = p.get().unwrap();
                Arc::new(DbPool {
                    conn_str: format!("pg://host?retries={}", config.max_retries),
                    pool_id: 42,
                })
            })
        })
        .build_async()
        .await
        .unwrap();

    let pool: Arc<DbPool> = provider.get_async().await.unwrap();
    assert_eq!(pool.conn_str, "pg://host?retries=5");
}

#[tokio::test]
async fn async_multiple_scopes_independent() {
    // Async scoped services should be cached within a scope and independent per scope.
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct ScopedCounter(u64);

    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            let id = NEXT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Arc::new(ScopedCounter(id)) })
        })
        .build_async()
        .await
        .unwrap();

    let scope1 = provider.scope();
    let c1: Arc<ScopedCounter> = scope1.get_async().await.unwrap();
    let c1b: Arc<ScopedCounter> = scope1.get_async().await.unwrap();

    let scope2 = provider.scope();
    let c2: Arc<ScopedCounter> = scope2.get_async().await.unwrap();

    // Different scopes get different instances
    assert!(!Arc::ptr_eq(&c1, &c2));
    assert_ne!(c1.0, c2.0);
    // Same scope: cached, factory executes once per scope
    assert!(Arc::ptr_eq(&c1, &c1b));
    assert_eq!(c1.0, c1b.0);
    // 2 factory calls: scope1 + scope2
    assert_eq!(NEXT.load(Ordering::SeqCst), 2);
}
