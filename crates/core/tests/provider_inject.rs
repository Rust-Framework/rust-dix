//! Tests for `#[inject(provider)]` — injecting `Arc<ServiceProvider>` for on-demand resolution.
//!
//! Verifies:
//! - A service with `#[inject(provider)]` receives a usable `Arc<ServiceProvider>`.
//! - The injected provider can resolve other services via `get_owned::<T>()`.
//! - Works when the provider is created via `into_arc()`.
//! - Works through a `Scope` (delegates to parent provider).

#![allow(non_snake_case)]

mod common;

use rust_dicore::*;
use rust_dicore_macros::Inject;
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
        self.provider.get_owned::<common::MyService>()
    }
}

#[test]
fn provider_inject_resolves_via_into_arc() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 42 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap()
        .into_arc();

    let h = p.get_owned::<HandlerWithProvider>();
    assert_eq!(h.resolve_dep().value, 42);
}

#[test]
fn provider_inject_field_is_some() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 0 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap()
        .into_arc();

    let h = p.get_owned::<HandlerWithProvider>();
    // provider_arc should return Some because into_arc was used
    assert!(IServiceResolver::provider_arc(&*p).is_some());
    // The field should be a valid Arc<ServiceProvider> — can resolve without panic
    assert_eq!(h.provider.get_owned::<common::MyService>().value, 0);
}

#[test]
fn provider_inject_works_through_scope() {
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 99 }))
        .transient(|r| __rdi_construct_HandlerWithProvider(r))
        .build()
        .unwrap()
        .into_arc();

    let scope = p.scope();
    let h: HandlerWithProvider = scope.get_owned();
    // Through scope, provider_arc delegates to parent — owned resolution bypasses cache
    assert_eq!(h.resolve_dep().value, 99);
}

#[test]
#[should_panic(expected = "ServiceProvider not available")]
fn provider_inject_panics_without_into_arc() {
    // Using Arc::new(build()) instead of into_arc() — self_weak is never initialized.
    // provider_arc() returns None → .expect() panics.
    let p = Arc::new(
        ServiceCollection::new()
            .transient(|r| __rdi_construct_HandlerWithProvider(r))
            .build()
            .unwrap(),
    );

    // This should panic because self_weak was never set.
    let _h = p.get_owned::<HandlerWithProvider>();
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
        .unwrap()
        .into_arc();

    let h = p.get_owned::<MixedProviderHandler>();
    assert_eq!(h.logger.prefix, "log");
    assert_eq!(h.provider.get_owned::<common::MyService>().value, 7);
}
