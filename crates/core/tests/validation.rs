use rust_dix::*;
use std::sync::atomic::{AtomicU64, Ordering};
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
        Err(RdiError::CircularDependency(_)) => {}
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
        Err(RdiError::CircularDependency(_)) => {}
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
    // 0.8 默认严格 scope 校验拒绝 root 解析 Scoped；lenient 模式保留 0.7 的
    // root-scope 缓存语义：从根解析 Scoped 服务在根内缓存复用。
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    let a1: Arc<ServiceA> = provider.get().unwrap();
    let a2: Arc<ServiceA> = provider.get().unwrap();
    assert!(Arc::ptr_eq(&a1, &a2), "root scope 内 Scoped 应复用同一实例");
}

#[test]
fn scoped_isolated_between_root_and_child_scope() {
    // 根 root scope 与子 scope 的 Scoped 实例相互独立（MEDI 语义）。
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    let root_a: Arc<ServiceA> = provider.get().unwrap();
    let scope = provider.scope();
    let child_a: Arc<ServiceA> = scope.get().unwrap();
    assert!(
        !Arc::ptr_eq(&root_a, &child_a),
        "根与子 scope 的 Scoped 应相互独立"
    );
}

#[test]
fn root_scope_scoped_factory_executed_once() {
    // 用工厂执行计数器严格证明 root scope 缓存真实生效：
    // 工厂只执行一次，即使解析多次。Arc::ptr_eq 不够 —— 计数器排除了
    // "工厂碰巧返回同一 Arc"的可能。
    static SCOPED_CALLS: AtomicU64 = AtomicU64::new(0);
    struct ScopedService(u64);

    let provider = ServiceCollection::new()
        .scoped(|_| {
            let n = SCOPED_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedService(n))
        })
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    // 首次解析：工厂执行，计数 0 → 1，实例携带序号 0。
    let a1: Arc<ScopedService> = provider.get().unwrap();
    assert_eq!(SCOPED_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(a1.0, 0);

    // 第二、三次解析：应命中 root_scoped_cache，工厂不再执行。
    let a2: Arc<ScopedService> = provider.get().unwrap();
    let a3: Arc<ScopedService> = provider.get().unwrap();
    assert_eq!(
        SCOPED_CALLS.load(Ordering::SeqCst),
        1,
        "root scope 缓存应阻止工厂再次执行"
    );
    assert_eq!(a2.0, 0);
    assert_eq!(a3.0, 0);

    // 三次解析返回同一实例（双重佐证：序号一致 + Arc 指针相等）。
    assert!(Arc::ptr_eq(&a1, &a2));
    assert!(Arc::ptr_eq(&a2, &a3));
}

#[test]
fn root_scope_transient_factory_executed_each_time() {
    // 对照组：Transient 工厂每次解析都执行，计数递增，证明计数器机制有效，
    // 且 Scoped 的"只执行一次"确实是缓存生效而非计数器故障。
    static TRANSIENT_CALLS: AtomicU64 = AtomicU64::new(0);
    struct TransientService(u64);

    let provider = ServiceCollection::new()
        .transient(|_| {
            let n = TRANSIENT_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(TransientService(n))
        })
        .build()
        .unwrap();

    let t1: Arc<TransientService> = provider.get().unwrap();
    let t2: Arc<TransientService> = provider.get().unwrap();
    let t3: Arc<TransientService> = provider.get().unwrap();
    assert_eq!(TRANSIENT_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(t1.0, 0);
    assert_eq!(t2.0, 1);
    assert_eq!(t3.0, 2);
    assert!(!Arc::ptr_eq(&t1, &t2));
}

#[test]
fn scoped_factory_not_triggered_at_build() {
    // build 阶段不应预触发 Scoped 工厂：validation 只验证 Singleton 依赖链，
    // 独立注册的 Scoped 服务在 build 后工厂应未执行。
    static BUILD_CALLS: AtomicU64 = AtomicU64::new(0);
    struct S;

    let provider = ServiceCollection::new()
        .scoped(|_| {
            BUILD_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(S)
        })
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    assert_eq!(
        BUILD_CALLS.load(Ordering::SeqCst),
        0,
        "build 阶段不应触发 Scoped 工厂"
    );

    // 首次解析才触发工厂。
    let _s1: Arc<S> = provider.get().unwrap();
    assert_eq!(BUILD_CALLS.load(Ordering::SeqCst), 1);

    // 再次解析命中缓存，工厂不再执行。
    let _s2: Arc<S> = provider.get().unwrap();
    assert_eq!(BUILD_CALLS.load(Ordering::SeqCst), 1);
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
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let scope = provider.scope();
    let a1: Arc<ServiceA> = scope.get().unwrap();
    let a2: Arc<ServiceA> = scope.get().unwrap();
    assert!(Arc::ptr_eq(&a1, &a2));

    let scope2 = provider.scope();
    let a3: Arc<ServiceA> = scope2.get().unwrap();
    assert!(!Arc::ptr_eq(&a1, &a3));
}

#[test]
fn transient_creates_new_instances() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get().unwrap();
    let a2: Arc<ServiceA> = p.get().unwrap();
    assert!(!Arc::ptr_eq(&a1, &a2));
}

#[test]
fn singleton_shared_globally() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = p.get().unwrap();
    let a2: Arc<ServiceA> = p.get().unwrap();
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

#[test]
fn root_scope_scoped_factory_executed_once_under_concurrency() {
    // 端到端 TOCTOU 验证：8 线程并发从根 scope 解析 Scoped 服务，
    // LazyCache 的 per-key OnceLock 应保证工厂只执行一次，
    // 彻底消除 read-check-then-write 竞态。
    // 若回退到旧 RwLock<HashMap> 的 check-then-act 写法，
    // 多线程同时 miss 时工厂会被多次执行，计数 > 1。
    static SCOPED_CALLS: AtomicU64 = AtomicU64::new(0);
    struct ScopedService(u64);

    let provider = ServiceCollection::new()
        .scoped(|_| {
            let n = SCOPED_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedService(n))
        })
        .build_with(ServiceProviderOptions::lenient())
        .unwrap();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let p = provider.clone();
        handles.push(std::thread::spawn(move || {
            let s: Arc<ScopedService> = p.get().unwrap();
            s
        }));
    }
    let results: Vec<Arc<ScopedService>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(
        SCOPED_CALLS.load(Ordering::SeqCst),
        1,
        "8 线程并发解析应只触发一次工厂（LazyCache 消除 TOCTOU）"
    );
    assert_eq!(results[0].0, 0, "唯一实例应携带序号 0");
    for r in &results {
        assert!(Arc::ptr_eq(&results[0], r), "所有线程应拿到同一 Arc 实例");
    }
}

#[test]
fn validation_does_not_leak_transient_to_runtime_cache() {
    // Singleton 依赖 Transient（合法，非 captive）：
    // build 阶段 validation 会执行 Transient 工厂一次（用于验证依赖链），
    // 但 Transient 实例不应泄漏到 singleton_cache。
    // 验证方式：build 后运行时解析 Transient 应得到新实例（计数递增），
    // 而非返回 validation 阶段缓存的实例（那会让计数停留在 1）。
    static TRANSIENT_CALLS: AtomicU64 = AtomicU64::new(0);
    struct TransientService(u64);
    struct SingletonService;

    let provider = ServiceCollection::new()
        .transient(|_| {
            let n = TRANSIENT_CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(TransientService(n))
        })
        .singleton(|r| {
            let _t = r.get_any(std::any::type_name::<TransientService>());
            Arc::new(SingletonService)
        })
        .build()
        .unwrap();

    // build 阶段 validation 执行过一次 Transient 工厂（Singleton 依赖触发）。
    assert_eq!(
        TRANSIENT_CALLS.load(Ordering::SeqCst),
        1,
        "build 阶段 validation 应执行一次 Transient 工厂"
    );

    // 运行时解析 Transient：应得到新实例（计数递增到 2、3），
    // 而非返回 validation 缓存的实例（那会让计数停留在 1，序号停在 0）。
    let t1: Arc<TransientService> = provider.get().unwrap();
    assert_eq!(
        TRANSIENT_CALLS.load(Ordering::SeqCst),
        2,
        "运行时首次解析应执行新工厂，而非复用 validation 缓存"
    );
    assert_eq!(t1.0, 1, "运行时首次解析应得到序号 1（validation 是 0）");

    let t2: Arc<TransientService> = provider.get().unwrap();
    assert_eq!(TRANSIENT_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(t2.0, 2);
    assert!(
        !Arc::ptr_eq(&t1, &t2),
        "Transient 每次解析应返回新实例，无缓存泄漏"
    );
}

#[test]
fn overridden_duplicate_singleton_is_still_validated() {
    // 0.8 last-wins: the earlier registration is overridden for resolution, but
    // eager build validates *every* registration under its own cache key, so a
    // panicking overridden factory fails the build deterministically.
    #[derive(Debug)]
    struct Dup;

    let result = ServiceCollection::new()
        .singleton::<Dup>(|_| panic!("first factory panics"))
        .singleton(|_| Arc::new(Dup))
        .build();
    match result {
        Err(RdiError::FactoryPanic { payload, .. }) => {
            assert!(
                payload.contains("first factory panics"),
                "payload={payload}"
            );
        }
        Ok(_) => panic!("overridden factory must still be validated"),
        Err(other) => panic!("expected FactoryPanic, got {other:?}"),
    }
}

#[test]
fn validation_error_is_deterministic_registration_order() {
    // Two failing singletons: the first registered one is reported, regardless
    // of HashMap iteration order.
    #[derive(Debug)]
    struct FailingA;
    #[derive(Debug)]
    struct FailingB;

    let result = ServiceCollection::new()
        .singleton::<FailingA>(|_| panic!("A fails"))
        .singleton::<FailingB>(|_| panic!("B fails"))
        .build();
    match result {
        Err(RdiError::FactoryPanic { payload, .. }) => {
            assert!(payload.contains("A fails"), "payload={payload}");
        }
        Ok(_) => panic!("expected deterministic FactoryPanic for A"),
        Err(other) => panic!("expected deterministic FactoryPanic for A, got {other:?}"),
    }
}
