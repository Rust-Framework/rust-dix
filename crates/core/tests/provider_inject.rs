//! Tests for `#[inject(provider)]` — injecting `Arc<ServiceProvider>` for on-demand resolution.
//!
//! Verifies:
//! - A service with `#[inject(provider)]` receives a usable `Arc<ServiceProvider>`.
//! - The injected provider can resolve other services via `get_owned::<T>()`.
//! - Works when the provider is created via `build()` (returns `Arc<ServiceProvider>`).
//! - Works through a `Scope` (delegates to parent provider).

#![allow(non_snake_case)]

mod common;

use rust_dix::*;
use rust_dix_macros::Inject;
use std::sync::Arc;

/// Service that holds the provider for on-demand resolution.
/// Mimics a web handler that resolves DbContext per-request.
#[derive(Inject)]
struct HandlerWithProvider {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,
}

impl HandlerWithProvider {
    fn resolve_dep(&self) -> common::MyService {
        self.provider.get_owned::<common::MyService>().unwrap()
    }
}

#[test]
fn provider_inject_resolves_via_build() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 42 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap();

    let h = p.get_owned::<HandlerWithProvider>().unwrap();
    assert_eq!(h.resolve_dep().value, 42);
}

#[test]
fn provider_inject_field_is_some() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 0 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap();

    let h = p.get_owned::<HandlerWithProvider>().unwrap();
    // provider_arc should return Some because build() returns Arc<ServiceProvider>
    assert!(IServiceResolver::provider_arc(&*p).is_some());
    // The field should be a valid Arc<ServiceProvider> — can resolve without panic
    assert_eq!(h.provider.get_owned::<common::MyService>().unwrap().value, 0);
}

#[test]
fn provider_inject_works_through_scope() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 99 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap();

    let scope = p.scope();
    let h: HandlerWithProvider = scope.get_owned().unwrap();
    // Scope 现持有 scope_provider；注入的字段是 scope 范围 provider，
    // 解析 Scoped 命中 scope 自己的 root_scoped_cache（不泄漏到 root）。
    assert_eq!(h.resolve_dep().value, 99);

    // 验证注入的 provider 是 scope 范围（非 root）：
    // 同一 scope 内多次 get Scoped 返回同一 Arc（scope 缓存命中）。
    let s1: Arc<common::MyService> = h.provider.get().unwrap();
    let s2: Arc<common::MyService> = h.provider.get().unwrap();
    assert!(
        Arc::ptr_eq(&s1, &s2),
        "scope 内 Scoped 应缓存复用（同一 Arc）"
    );
    // scope 与 root 应有独立 Scoped 实例（不同 Arc），证明未泄漏到 root。
    let root_svc: Arc<common::MyService> = p.get().unwrap();
    assert!(
        !Arc::ptr_eq(&s1, &root_svc),
        "scope 与 root 的 Scoped 实例应独立"
    );
}

#[test]
fn scope_provider_resolves_scoped_within_scope() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CALLS: AtomicU64 = AtomicU64::new(0);

    struct ScopedSvc(u64);

    let p = ServiceCollection::new()
        .scoped(|_| {
            let n = CALLS.fetch_add(1, Ordering::SeqCst);
            Arc::new(ScopedSvc(n))
        })
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap();

    // scope1：注入字段拿到 scope-local provider，解析 ScopedSvc 命中 scope1 缓存。
    let scope1 = p.scope();
    let h1: HandlerWithProvider = scope1.get_owned().unwrap();
    let a: Arc<ScopedSvc> = h1.provider.get().unwrap();
    let b: Arc<ScopedSvc> = h1.provider.get().unwrap();
    assert_eq!(a.0, b.0, "scope1 内 ScopedSvc 复用同一序号");
    assert!(Arc::ptr_eq(&a, &b), "scope1 缓存命中（同一 Arc）");

    // scope2：独立子 scope，ScopedSvc 工厂再次执行（新序号）。
    let scope2 = p.scope();
    let h2: HandlerWithProvider = scope2.get_owned().unwrap();
    let c: Arc<ScopedSvc> = h2.provider.get().unwrap();
    assert_ne!(a.0, c.0, "scope2 应有独立 ScopedSvc 实例");
    assert!(!Arc::ptr_eq(&a, &c), "不同 scope 的 ScopedSvc 必须独立");

    // 根 scope：第三个独立实例，证明子 scope 未泄漏到根。
    let root_svc: Arc<ScopedSvc> = p.get().unwrap();
    assert_ne!(a.0, root_svc.0);
    assert_ne!(c.0, root_svc.0);

    // 全程工厂只执行 3 次（scope1、scope2、根各一次）。
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        3,
        "ScopedSvc 工厂应执行 3 次（三 scope 各一次）"
    );
}

/// Service with provider + regular injected dependency side by side.
#[derive(Inject)]
struct MixedProviderHandler {
    #[inject]
    logger: Arc<common::Logger>,
    #[inject(provider)]
    provider: Arc<ServiceProvider>,
}

#[test]
fn provider_inject_mixed_with_regular_inject() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::Logger { prefix: "log".into() }))
        .transient(|_| Arc::new(common::MyService { value: 7 }))
        .transient(|r| __rdi_construct_MixedProviderHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<MixedProviderHandler>().unwrap();
    assert_eq!(h.logger.prefix, "log");
    assert_eq!(h.provider.get_owned::<common::MyService>().unwrap().value, 7);
}
