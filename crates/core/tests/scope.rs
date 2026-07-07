mod common;

use rust_dicore::*;
use std::sync::Arc;

#[test]
fn create_scope_medinaming() {
    let p = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(common::MyService { value: 10 }))
            .build()
            .unwrap(),
    );
    let scope = p.create_scope();
    let svc: Arc<common::MyService> = scope.get().unwrap();
    assert_eq!(svc.value, 10);
}

#[test]
fn scope_get_service_and_get_required() {
    let p = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(common::MyService { value: 20 }))
            .build()
            .unwrap(),
    );
    let scope = p.scope();
    let opt: Option<Arc<common::MyService>> = scope.get_service();
    assert_eq!(opt.unwrap().value, 20);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<common::Logger> = scope.get_required_service();
    }));
    assert!(result.is_err());
}

#[test]
fn scope_get_services_returns_empty() {
    let p = Arc::new(ServiceCollection::new().build().unwrap());
    let scope = p.scope();
    let all: Vec<Arc<common::MyService>> = scope.get_services();
    assert!(all.is_empty());
}

#[test]
fn scope_get_named_any_delegates_to_parent() {
    let p = Arc::new(ServiceCollection::new().build().unwrap());
    p.register_named("scope_test", Arc::new(common::MyService { value: 5 }));
    let scope = p.scope();
    let retrieved = scope.get_named_any("scope_test");
    assert!(retrieved.is_some());
}

#[test]
fn scoped_cached_within_same_scope() {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    use std::sync::atomic::Ordering;
    let p = Arc::new(
        ServiceCollection::new()
            .scoped(|_| {
                COUNTER.fetch_add(1, Ordering::SeqCst);
                Arc::new(common::MyService { value: 1 })
            })
            .build()
            .unwrap(),
    );
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

    let provider = Arc::new(
        ServiceCollection::new()
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
            .build()
            .unwrap(),
    );

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
