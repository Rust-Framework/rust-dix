use rust_dicore::*;
use std::sync::Arc;

struct ServiceA;
struct ServiceB;
struct ServiceC;

#[test]
fn circular_dependency_self_reference() {
    let result = ServiceCollection::new()
        .singleton(|r| {
            let _a = r.get_any(std::any::type_name::<ServiceA>());
            Arc::new(ServiceA)
        })
        .build();
    assert!(result.is_err());
    match result {
        Err(RdiError::CircularDependency(_)) => {},
        _ => panic!("Expected CircularDependency"),
    }
}

#[test]
fn circular_dependency_indirect() {
    let result = ServiceCollection::new()
        .singleton(|r| {
            let _b = r.get_any(std::any::type_name::<ServiceB>());
            Arc::new(ServiceA)
        })
        .singleton(|r| {
            let _a = r.get_any(std::any::type_name::<ServiceA>());
            Arc::new(ServiceB)
        })
        .build();
    assert!(result.is_err());
    match result {
        Err(RdiError::CircularDependency(_)) => {},
        _ => panic!("Expected CircularDependency"),
    }
}

#[test]
fn no_circular_dependency_valid() {
    let result = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceB))
        .singleton(|r| {
            let _b = r.get_any(std::any::type_name::<ServiceB>());
            Arc::new(ServiceA)
        })
        .build();
    assert!(result.is_ok());
}

#[test]
fn scoped_cached_at_root_scope() {
    // ServiceProvider 即 root scope：从根解析 Scoped 服务应在根内缓存复用。
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = provider.get();
    let a2: Arc<ServiceA> = provider.get();
    assert!(Arc::ptr_eq(&a1, &a2), "root scope 内 Scoped 应复用同一实例");
}

#[test]
fn scoped_isolated_between_root_and_child_scope() {
    // 根 root scope 与子 scope 的 Scoped 实例相互独立（MEDI 语义）。
    let provider = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(ServiceA))
            .build()
            .unwrap(),
    );

    let root_a: Arc<ServiceA> = provider.get();
    let scope = provider.scope();
    let child_a: Arc<ServiceA> = scope.get();
    assert!(
        !Arc::ptr_eq(&root_a, &child_a),
        "根与子 scope 的 Scoped 应相互独立"
    );
}

#[test]
fn singleton_depends_on_scoped_rejected() {
    // Captive dependency：Singleton 依赖 Scoped 应在 build 阶段被拒绝。
    let result = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .singleton(|r| {
            let _a = r.get_any(std::any::type_name::<ServiceA>());
            Arc::new(ServiceB)
        })
        .build();
    assert!(result.is_err());
    match result {
        Err(RdiError::SingletonDependsOnScoped { singleton, scoped }) => {
            assert!(singleton.contains("ServiceB"));
            assert!(scoped.contains("ServiceA"));
        }
        _ => panic!("Expected SingletonDependsOnScoped error"),
    }
}

#[test]
fn singleton_indirect_depends_on_scoped_rejected() {
    // 间接 captive dependency：Singleton → Transient → Scoped 也应被拒绝。
    let result = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .transient(|r| {
            let _a = r.get_any(std::any::type_name::<ServiceA>());
            Arc::new(ServiceC)
        })
        .singleton(|r| {
            let _c = r.get_any(std::any::type_name::<ServiceC>());
            Arc::new(ServiceB)
        })
        .build();
    assert!(result.is_err());
    match result {
        Err(RdiError::SingletonDependsOnScoped { .. }) => {}
        _ => panic!("Expected SingletonDependsOnScoped error"),
    }
}

#[test]
fn scoped_correct_usage() {
    let provider = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(ServiceA))
            .build()
            .unwrap()
    );

    let scope = provider.scope();
    let a1: Arc<ServiceA> = scope.get();
    let a2: Arc<ServiceA> = scope.get();
    assert!(Arc::ptr_eq(&a1, &a2));

    let scope2 = provider.scope();
    let a3: Arc<ServiceA> = scope2.get();
    assert!(!Arc::ptr_eq(&a1, &a3));
}

#[test]
fn transient_creates_new_instances() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get();
    let a2: Arc<ServiceA> = p.get();
    assert!(!Arc::ptr_eq(&a1, &a2));
}

#[test]
fn singleton_shared_globally() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get();
    let a2: Arc<ServiceA> = p.get();
    assert!(Arc::ptr_eq(&a1, &a2));
}

#[test]
fn diamond_dependency_valid() {
    struct D;
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(D))
        .singleton(|r| {
            let _d = r.get_any(std::any::type_name::<D>());
            Arc::new(ServiceB)
        })
        .singleton(|r| {
            let _d = r.get_any(std::any::type_name::<D>());
            Arc::new(ServiceC)
        })
        .singleton(|r| {
            let _b = r.get_any(std::any::type_name::<ServiceB>());
            let _c = r.get_any(std::any::type_name::<ServiceC>());
            Arc::new(ServiceA)
        })
        .build();
    assert!(p.is_ok());
}

#[test]
fn circular_dependency_error_message() {
    let result = ServiceCollection::new()
        .singleton(|r| {
            let _b = r.get_any(std::any::type_name::<ServiceB>());
            Arc::new(ServiceA)
        })
        .singleton(|r| {
            let _a = r.get_any(std::any::type_name::<ServiceA>());
            Arc::new(ServiceB)
        })
        .build();

    match result {
        Err(RdiError::CircularDependency(msg)) => {
            assert!(msg.contains("ServiceA"));
            assert!(msg.contains("ServiceB"));
        }
        _ => panic!("Expected CircularDependency error"),
    }
}
