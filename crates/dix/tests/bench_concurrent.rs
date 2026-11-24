//! 高并发基准测试：验证 async scoped 缓存竞态修复的性能损耗。
//!
//! 测试场景：
//! 1. 空工厂 100 并发 — 测量 `FuturesMutex` 纯开销
//! 2. 带延迟工厂 100 并发 — 测量真实场景（工厂内有 async I/O）
//! 3. sync 基线对比 — 对比 `OnceLock` 路径的开销

mod common;

use rust_dix::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::time::Instant;

/// 100 并发 async scoped 解析（空工厂）— 测量 FuturesMutex 纯开销。
///
/// 预期：工厂执行 1 次，100 个任务拿到同一 Arc 实例。
/// 性能：FuturesMutex 锁获取/释放 100 次，耗时应 < 100ms。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_async_scoped_100_concurrent_empty_factory() {
    static CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(common::MyService { value: 42 })
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(100);
    for _ in 0..100 {
        let sp = scope.scope_provider().clone();
        handles.push(tokio::spawn(async move {
            sp.get_async::<common::MyService>().await.unwrap()
        }));
    }

    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    // 正确性：工厂只执行 1 次
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "100 并发：async factory 应只执行一次"
    );

    // 所有结果指向同一实例（缓存命中）
    let first = results[0].as_ref().unwrap();
    for (i, r) in results.iter().enumerate() {
        assert!(
            Arc::ptr_eq(first, r.as_ref().unwrap()),
            "任务 {} 应拿到同一实例",
            i
        );
    }

    println!(
        "bench_async_scoped_100_concurrent_empty_factory: {:?} (100 并发, FuturesMutex 纯开销)",
        elapsed
    );
    // 宽松阈值：不同机器性能差异，500ms 是绝对上限
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "100 并发空工厂解析耗时应 < 500ms, 实际 {:?}",
        elapsed
    );
}

/// 100 并发 async scoped 解析（带 10ms 延迟工厂）— 测量真实场景。
///
/// 预期：工厂执行 1 次（仅首个任务执行 factory，其余 99 个 await 等待）。
/// 性能：总耗时应接近 10ms + 调度开销，而非 100 × 10ms = 1000ms。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_async_scoped_100_concurrent_with_delay() {
    static CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                CALLS.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Arc::new(common::MyService { value: 99 })
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(100);
    for _ in 0..100 {
        let sp = scope.scope_provider().clone();
        handles.push(tokio::spawn(async move {
            sp.get_async::<common::MyService>().await.unwrap()
        }));
    }

    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "带延迟工厂：async factory 应只执行一次"
    );

    let first = results[0].as_ref().unwrap();
    for r in &results {
        assert!(Arc::ptr_eq(first, r.as_ref().unwrap()));
    }

    println!(
        "bench_async_scoped_100_concurrent_with_delay: {:?} (100 并发, 10ms 工厂延迟, 串行需 1000ms)",
        elapsed
    );
    // 并发应远快于串行：10ms + 调度开销 < 200ms
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "100 并发带延迟解析应 < 200ms (串行需 1000ms), 实际 {:?}",
        elapsed
    );
}

/// 100 并发 sync scoped 解析 — OnceLock 基线对比。
///
/// sync 路径用 `OnceLock::get_or_init`，多线程同时 miss 时只有一个线程
/// 执行工厂，其余线程阻塞等待。这是性能最优的基线。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_sync_scoped_100_concurrent_baseline() {
    static CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = ServiceCollection::new()
        .scoped(|_| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(common::MyService { value: 7 })
        })
        .build()
        .unwrap();

    let scope = provider.scope();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(100);
    for _ in 0..100 {
        let sp = scope.scope_provider().clone();
        handles.push(tokio::task::spawn_blocking(move || {
            sp.get::<common::MyService>().unwrap()
        }));
    }

    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "sync 基线：factory 应只执行一次"
    );

    let first = results[0].as_ref().unwrap();
    for r in &results {
        assert!(Arc::ptr_eq(first, r.as_ref().unwrap()));
    }

    println!(
        "bench_sync_scoped_100_concurrent_baseline: {:?} (100 并发, OnceLock 基线)",
        elapsed
    );
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "100 并发 sync 基线解析应 < 500ms, 实际 {:?}",
        elapsed
    );
}

/// 1000 并发压力测试 — 验证高负载下 FuturesMutex 无死锁、无饥饿。
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stress_async_scoped_1000_concurrent() {
    static CALLS: AtomicU64 = AtomicU64::new(0);

    let provider = ServiceCollection::new()
        .async_scoped(|_| {
            Box::pin(async {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Arc::new(common::MyService { value: 1000 })
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let sp = scope.scope_provider().clone();
        handles.push(tokio::spawn(async move {
            sp.get_async::<common::MyService>().await.unwrap()
        }));
    }

    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "1000 并发：async factory 应只执行一次"
    );

    let first = results[0].as_ref().unwrap();
    for r in &results {
        assert!(Arc::ptr_eq(first, r.as_ref().unwrap()));
    }

    println!(
        "stress_async_scoped_1000_concurrent: {:?} (1000 并发, 8 worker threads)",
        elapsed
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "1000 并发压力测试应 < 2s, 实际 {:?}",
        elapsed
    );
}
