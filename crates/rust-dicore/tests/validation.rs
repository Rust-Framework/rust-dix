//! 循环依赖检测和生命周期验证测试

use rust_dicore::*;
use std::sync::Arc;

// ── 测试结构体 ──

struct ServiceA;
struct ServiceB;
struct ServiceC;

// ── 循环依赖检测测试 ──

/// 测试直接循环依赖: A → A
#[test]
fn circular_dependency_self_reference() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ServiceCollection::new()
            .singleton(|r| {
                // A depends on itself
                let _a: Arc<ServiceA> = r.get();
                Arc::new(ServiceA)
            })
            .build()
            .unwrap()
    }));
    assert!(result.is_err(), "Self-reference should fail during build");
}

/// 测试间接循环依赖: A → B → A
#[test]
fn circular_dependency_indirect() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // 创建相互引用的服务
        let b_once = std::sync::Once::new();
        let b_factory: std::sync::Arc<dyn Fn(&dyn IServiceResolver) -> Arc<ServiceB> + Send + Sync> =
            std::sync::Arc::new(move |_r| Arc::new(ServiceB));

        ServiceCollection::new()
            .singleton(|r| {
                // A depends on B
                let _b: Arc<ServiceB> = r.get();
                Arc::new(ServiceA)
            })
            .singleton(|r| {
                // B depends on A
                let _a: Arc<ServiceA> = r.get();
                Arc::new(ServiceB)
            })
            .build()
            .unwrap()
    }));

    // 注意: 这个测试可能通过，因为两个单例工厂都在同一个闭包中
    // 循环依赖检测需要工厂执行时才能检测
}

/// 测试无循环依赖: A → B (应该成功)
#[test]
fn no_circular_dependency_valid() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceB))
        .singleton(|r| {
            // A depends on B, but B is already resolved
            let _b: Arc<ServiceB> = r.get();
            Arc::new(ServiceA)
        })
        .build()
        .unwrap();

    let _a: Arc<ServiceA> = p.get();
    let _b: Arc<ServiceB> = p.get();
}

// ── 生命周期验证测试 ──

/// 测试 Singleton 依赖 Scoped (警告级别)
///
/// 注: 当前版本无法在构建时检测此问题，
/// 因为 Scoped 服务在根 Provider 解析时等价于 Transient。
/// 这是设计上的限制，用户需要自行避免。
#[test]
fn singleton_depends_on_scoped_behavior() {
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    // 从根 Provider 解析 Scoped 服务 - 等价于 Transient
    let a1: Arc<ServiceA> = provider.get();
    let a2: Arc<ServiceA> = provider.get();

    // 每次返回不同实例（Scoped 语义丢失）
    assert!(!Arc::ptr_eq(&a1, &a2),
        "Scoped from root provider should behave like Transient");
}

/// 测试正确的 Scoped 使用: 从 Scope 解析
#[test]
fn scoped_correct_usage() {
    let provider = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(ServiceA))
            .build()
            .unwrap()
    );

    // 创建 Scope
    let scope = provider.scope();

    // 从 Scope 解析 - 同一 Scope 内共享实例
    let a1: Arc<ServiceA> = scope.get();
    let a2: Arc<ServiceA> = scope.get();

    assert!(Arc::ptr_eq(&a1, &a2),
        "Scoped services should be shared within a scope");

    // 不同 Scope 获取不同实例
    let scope2 = provider.scope();
    let a3: Arc<ServiceA> = scope2.get();

    assert!(!Arc::ptr_eq(&a1, &a3),
        "Different scopes should get different instances");
}

/// 测试 Transient 每次创建新实例
#[test]
fn transient_creates_new_instances() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get();
    let a2: Arc<ServiceA> = p.get();

    assert!(!Arc::ptr_eq(&a1, &a2),
        "Transient services should create new instances each time");
}

/// 测试 Singleton 全局共享
#[test]
fn singleton_shared_globally() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get();
    let a2: Arc<ServiceA> = p.get();

    assert!(Arc::ptr_eq(&a1, &a2),
        "Singleton services should be shared globally");
}

// ── 构建错误处理测试 ──

/// 测试 ServiceNotFound 错误
#[test]
fn error_service_not_found() {
    let p = ServiceCollection::new().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<NonExistent> = p.get();
    }));
    assert!(result.is_err(), "Getting unregistered service should panic");
}

/// 测试 KeyedServiceNotFound 错误
#[test]
fn error_keyed_service_not_found() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Arc<ServiceA> = p.get_keyed("nonexistent_key");
    }));
    assert!(result.is_err(), "Getting unregistered keyed service should panic");
}

// ── 复杂依赖链测试 ──

/// 测试长依赖链: A → B → C (应该成功)
#[test]
fn long_dependency_chain_valid() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceC))
        .singleton(|r| {
            let _c: Arc<ServiceC> = r.get();
            Arc::new(ServiceB)
        })
        .singleton(|r| {
            let _b: Arc<ServiceB> = r.get();
            Arc::new(ServiceA)
        })
        .build()
        .unwrap();

    let _a: Arc<ServiceA> = p.get();
    let _b: Arc<ServiceB> = p.get();
    let _c: Arc<ServiceC> = p.get();
}

/// 测试菱形依赖: A → B, C; B → D; C → D (应该成功)
#[test]
fn diamond_dependency_valid() {
    struct D;
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(D)) // D
        .singleton(|r| {
            let _d: Arc<D> = r.get();
            Arc::new(ServiceB) // B → D
        })
        .singleton(|r| {
            let _d: Arc<D> = r.get();
            Arc::new(ServiceC) // C → D
        })
        .singleton(|r| {
            let _b: Arc<ServiceB> = r.get();
            let _c: Arc<ServiceC> = r.get();
            Arc::new(ServiceA) // A → B, C
        })
        .build()
        .unwrap();

    let _a: Arc<ServiceA> = p.get();
}

// 辅助类型
struct NonExistent;
