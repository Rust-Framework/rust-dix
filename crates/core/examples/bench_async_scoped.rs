//! async scoped 并发压力基准测试
//!
//! 三级并发压力 + 延迟工厂 + 脏工厂场景，验证 FuturesMutex 性能边界。
//! 运行方式：`cargo run --example bench_async_scoped --release`

use rust_dix::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// 共享服务类型，用于所有测试场景。
struct Svc {
    _value: u64,
}

/// 运行并发测试的通用框架。
async fn run_bench(
    name: &str,
    concurrency: usize,
    factory: impl Fn(Arc<rust_dix::ServiceProvider>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Arc<Svc>> + Send>>
        + Send
        + Sync
        + 'static,
    expected_factory_calls: u64,
    expected_serial_ms: u64,
    max_parallel_ms: u64,
) {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    CALLS.store(0, Ordering::SeqCst);

    let provider = ServiceCollection::new()
        .async_scoped(move |sp| {
            let f = factory(sp);
            Box::pin(async move {
                CALLS.fetch_add(1, Ordering::SeqCst);
                f.await
            })
        })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let sp = scope.scope_provider().clone();
        handles.push(tokio::spawn(async move {
            sp.get_async::<Svc>().await.unwrap()
        }));
    }

    let results: Vec<Arc<Svc>> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let elapsed = start.elapsed();
    let calls = CALLS.load(Ordering::SeqCst);
    let speedup = expected_serial_ms as f64 / elapsed.as_millis() as f64;

    // 正确性：所有结果指向同一实例
    let first = &results[0];
    let all_same = results.iter().all(|r| Arc::ptr_eq(first, r));

    println!(
        "{:<45} | {:>5}并发 | {:>8.2?} | 工厂{}次 | 串行需{}ms | {:.1}x加速 | {}",
        name,
        concurrency,
        elapsed,
        calls,
        expected_serial_ms,
        speedup,
        if all_same && calls == expected_factory_calls && elapsed < Duration::from_millis(max_parallel_ms) {
            "OK"
        } else {
            "FAIL"
        }
    );

    assert_eq!(calls, expected_factory_calls, "工厂执行次数");
    assert!(all_same, "所有解析应返回同一实例");
    assert!(
        elapsed < Duration::from_millis(max_parallel_ms),
        "总耗时 {:.2?} 应 < {}ms",
        elapsed,
        max_parallel_ms
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    println!("{:->60}", "");
    println!("rust-dix async scoped 并发基准测试");
    println!("rust-dix {} — 8 worker threads, release build", env!("CARGO_PKG_VERSION"));
    println!("{:->60}", "");
    println!(
        "{:<45} | {:>5} | {:>10} | {:>8} | {:>10} | {:>6} | {:>4}",
        "场景", "并发", "耗时", "工厂次数", "串行对比", "加速比", "结果"
    );
    println!("{:->60}", "");

    // ── 空工厂：测量 FuturesMutex 纯开销 ──
    run_bench(
        "空工厂 (FuturesMutex 纯开销)",
        100,
        |_| Box::pin(async { Arc::new(Svc { _value: 1 }) }),
        1, 0, 500,
    )
    .await;

    run_bench(
        "空工厂",
        500,
        |_| Box::pin(async { Arc::new(Svc { _value: 1 }) }),
        1, 0, 1000,
    )
    .await;

    run_bench(
        "空工厂",
        1000,
        |_| Box::pin(async { Arc::new(Svc { _value: 1 }) }),
        1, 0, 2000,
    )
    .await;

    // ── 延迟工厂：模拟真实 async I/O（10ms 延迟）──
    run_bench(
        "10ms 延迟工厂 (模拟 async I/O)",
        100,
        |_| Box::pin(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Arc::new(Svc { _value: 100 })
        }),
        1, 1000, 200,
    )
    .await;

    run_bench(
        "10ms 延迟工厂",
        500,
        |_| Box::pin(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Arc::new(Svc { _value: 500 })
        }),
        1, 5000, 500,
    )
    .await;

    // ── 脏工厂：工厂内有额外计算 ──
    run_bench(
        "脏工厂 (1ms CPU + 5ms IO)",
        100,
        |_| Box::pin(async {
            // 模拟 CPU 密集计算
            let mut sum: u64 = 0;
            for i in 0..100_000 {
                sum = sum.wrapping_add(i);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
            Arc::new(Svc { _value: sum })
        }),
        1, 600, 150,
    )
    .await;

    println!("{:->60}", "");
    println!("全部通过 — async scoped 并发性能可接受");
    println!("{:->60}", "");
}