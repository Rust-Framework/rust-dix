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
fn singleton_depends_on_scoped_behavior() {
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = provider.get();
    let a2: Arc<ServiceA> = provider.get();

    assert!(!Arc::ptr_eq(&a1, &a2));
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
