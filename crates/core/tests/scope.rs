mod common;

use rust_dix::*;
use std::sync::Arc;

#[test]
fn scope_resolves_scoped() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 10 }))
        .build()
        .unwrap();
    let scope = p.scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 10);
}

#[test]
fn scope_get_optional_and_get_unwrap() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 20 }))
        .build()
        .unwrap();
    let scope = p.scope();
    let opt: Option<Arc<common::MyService>> = scope.get_optional();
    assert_eq!(opt.unwrap().value, 20);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<common::Logger> = scope.get().unwrap();
    }));
    assert!(result.is_err());
}

#[test]
fn scope_get_all_returns_empty() {
    let p = ServiceCollection::new().build().unwrap();
    let scope = p.scope();
    let all: Vec<Arc<common::MyService>> = scope.get_all();
    assert!(all.is_empty());
}

#[test]
fn scope_get_named_any_delegates_to_parent() {
    let p = ServiceCollection::new().build().unwrap();
    p.register_named("scope_test", Arc::new(common::MyService { value: 5 }));
    let scope = p.scope();
    let retrieved = scope.get_named_any("scope_test");
    assert!(retrieved.is_some());
}

#[test]
fn scoped_cached_within_same_scope() {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    use std::sync::atomic::Ordering;
    let p = ServiceCollection::new()
        .scoped(|_| {
            COUNTER.fetch_add(1, Ordering::SeqCst);
            Arc::new(common::MyService { value: 1 })
        })
        .build()
        .unwrap();
    let scope = p.scope();
    let _a = scope.get::<common::MyService>().unwrap();
    let _b = scope.get::<common::MyService>().unwrap();
    assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
}

#[test]
fn scope_transient_scoped_dependency_binds_to_child_scope() {
    // 子 scope 解析 Transient 时，Transient 工厂内的 Scoped 依赖
    // 应绑定到当前子 scope（使用子 scope 的 scoped_cache），
    // 而非回退到根 root_scoped_cache（MEDI 子 scope 隔离语义）。
    // 若 scope.rs 的 Transient 分支错误地用 self.parent 解析依赖，
    // 两个子 scope 的 Transient 内的 Scoped 会指向根缓存同一实例。
    static SCOPED_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    use std::sync::atomic::Ordering;

    struct ScopedDep(u64);
    struct TransientSvc {
        scoped: Arc<ScopedDep>,
    }

    let provider = ServiceCollection::new()
        .scoped(|_| {
            let n = SCOPED_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedDep(n))
        })
        .transient(|r| {
            // IServiceResolver::get_any 返回 Arc<dyn Any>，内部存的是
            // collection::push 包装后的 Arc<Arc<T>>（见 collection.rs push 方法）。
            // 因此 downcast 目标是 Arc<ScopedDep>，再解引用得到 Arc<ScopedDep>。
            let any = r.get_any(std::any::type_name::<ScopedDep>()).unwrap();
            let outer = any.downcast::<Arc<ScopedDep>>().unwrap();
            let s: Arc<ScopedDep> = Arc::clone(&*outer);
            Arc::new(TransientSvc { scoped: s })
        })
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    let scope1 = provider.scope();
    let t1: Arc<TransientSvc> = scope1.get().unwrap();
    // 同一子 scope 内两次解析 Transient，其 Scoped 依赖应复用同一实例
    // （Transient 每次新建，但其内部 Scoped 在 scope1 缓存中复用）。
    let t1_again: Arc<TransientSvc> = scope1.get().unwrap();
    assert!(
        Arc::ptr_eq(&t1.scoped, &t1_again.scoped),
        "同一子 scope 内 Transient 的 Scoped 依赖应复用同一实例"
    );

    let scope2 = provider.scope();
    let t2: Arc<TransientSvc> = scope2.get().unwrap();
    assert!(
        !Arc::ptr_eq(&t1.scoped, &t2.scoped),
        "不同子 scope 的 Transient 的 Scoped 依赖应独立（绑定子 scope 而非根）"
    );
    // 序号严格证明：scope1 的 Scoped 是序号 0，scope2 是序号 1（各自首次执行工厂）。
    assert_eq!(t1.scoped.0, 0, "scope1 的 Scoped 应为序号 0");
    assert_eq!(t2.scoped.0, 1, "scope2 的 Scoped 应为序号 1");

    // 根 scope 解析 Scoped 应得到第三个独立实例，与两个子 scope 都不同。
    let root_s: Arc<ScopedDep> = provider.get().unwrap();
    assert!(
        !Arc::ptr_eq(&t1.scoped, &root_s),
        "子 scope 的 Scoped 不应泄漏到根 root_scoped_cache"
    );
    assert!(
        !Arc::ptr_eq(&t2.scoped, &root_s),
        "子 scope 的 Scoped 不应泄漏到根 root_scoped_cache"
    );
    assert_eq!(root_s.0, 2, "根 scope 的 Scoped 应为序号 2");
}

#[test]
fn scope_drop_with_concurrent_resolution() {
    // 验证 Scope drop 与并发解析无死锁。
    // 工厂内 register_disposable 持锁，drop 的 dispose_scoped 用 try_lock 跳过。
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::thread;

    static RESOLVE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DISPOSE_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct ScopedSvc;

    let provider = ServiceCollection::new()
        .scoped(|r| {
            // 工厂内注册 disposable，持锁短暂窗口
            let p = r.provider_arc().unwrap();
            p.register_disposable(Box::new(|| {
                DISPOSE_COUNT.fetch_add(1, Ordering::SeqCst);
            }));
            RESOLVE_COUNT.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedSvc)
        })
        .build()
        .unwrap();

    let scope = provider.scope();
    let barrier = Arc::new(Barrier::new(5));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let scope_clone = scope.scope_provider().clone();
        let barrier_clone = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier_clone.wait();
            let _: Arc<ScopedSvc> = scope_clone.get().unwrap();
        }));
    }

    // 主线程也参与一次解析，触发 disposable 注册
    barrier.wait();
    let _: Arc<ScopedSvc> = scope.get().unwrap();

    for h in handles {
        h.join().unwrap();
    }

    // 至少 5 次解析成功（无死锁）
    assert!(RESOLVE_COUNT.load(Ordering::SeqCst) >= 1, "解析应成功完成");

    // 显式 dispose——此时无并发解析，应执行所有 disposable
    scope.dispose();
    assert!(
        DISPOSE_COUNT.load(Ordering::SeqCst) >= 1,
        "dispose 应执行已注册的清理闭包"
    );
}

#[test]
fn scope_factory_injected_into_singleton() {
    // 验证 ScopeFactory trait 可注入 singleton 服务，按需创建独立 scope。
    // 场景：singleton Worker 注入 Arc<dyn ScopeFactory>，每次任务创建新 scope。
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SCOPE_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct ScopedReq(usize);

    let provider = ServiceCollection::new()
        .scoped(|_| {
            let n = SCOPE_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedReq(n))
        })
        .build()
        .unwrap();

    // 注册 ScopeFactory：provider 实现了 ScopeFactory，用 singleton::<dyn ScopeFactory> 注册为 trait 对象
    let provider_with_factory = ServiceCollection::new()
        .scoped(|_| {
            let n = SCOPE_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedReq(n))
        })
        .singleton::<dyn ScopeFactory>(move |_| provider.clone() as Arc<dyn ScopeFactory>)
        .build()
        .unwrap();

    // 通过 DI 获取 ScopeFactory
    let sf: Arc<dyn ScopeFactory> = provider_with_factory.get().unwrap();

    // 创建两个独立 scope，验证 Scoped 服务实例独立
    let scope1 = sf.create_scope();
    let req1: Arc<ScopedReq> = scope1.get().unwrap();

    let scope2 = sf.create_scope();
    let req2: Arc<ScopedReq> = scope2.get().unwrap();

    assert!(
        !Arc::ptr_eq(&req1, &req2),
        "不同 scope 的 Scoped 服务应独立"
    );
    assert_eq!(req1.0, 0, "scope1 的 Scoped 应为序号 0");
    assert_eq!(req2.0, 1, "scope2 的 Scoped 应为序号 1");
}
