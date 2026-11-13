//! DIX-1 regression: macros must address the framework crate through its *real*
//! dependency name instead of hardcoding `::rust_dix`.
//!
//! `rust-dix-macros`'s manifest declares the core crate as
//! `dix = { package = "rust-dix", path = "../core" }`. If any expansion still
//! emits `::rust_dix::…`, this file fails to compile — exactly the failure
//! Travel hit before adding `extern crate dix as rust_dix;` shims.
//!
//! Deliberately no `extern crate dix as rust_dix;` here.

use std::sync::Arc;

use dix::{inject, Inject};

// ─────────────────────────────────────────────────────────────────────────
// `#[inject]` on a trait impl (dyn registration)
// ─────────────────────────────────────────────────────────────────────────

pub trait IClock: Send + Sync {
    fn now(&self) -> u64;
}

#[derive(Inject)]
pub struct FakeClock;

#[inject(singleton)]
impl IClock for FakeClock {
    fn now(&self) -> u64 {
        42
    }
}

// ─────────────────────────────────────────────────────────────────────────
// `#[derive(Inject)]` with a required trait-object field
// ─────────────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct Service {
    #[inject]
    clock: Arc<dyn IClock>,
}

#[test]
fn derive_inject_resolves_renamed_crate() {
    let provider = dix::ServiceCollection::from_injected()
        .singleton(|r| __rdi_construct_Service(r))
        .build()
        .unwrap();
    let svc: Arc<Service> = provider.get().unwrap();
    assert_eq!(svc.clock.now(), 42);
}

// ─────────────────────────────────────────────────────────────────────────
// `#[derive(Inject)]` with optional / keyed fields (macros_support paths)
// ─────────────────────────────────────────────────────────────────────────

struct KeyedConfig {
    value: u8,
}

/// Registered by nothing: exercises the optional-field (None) expansion path.
pub trait IUnregistered: Send + Sync {}

#[derive(Inject)]
struct KeyedConsumer {
    #[inject(key = "primary")]
    config: Arc<KeyedConfig>,
    #[inject]
    missing: Option<Arc<dyn IUnregistered>>,
}

#[test]
fn derive_inject_keyed_and_optional_resolve_renamed_crate() {
    let provider = dix::ServiceCollection::from_injected()
        .keyed_singleton("primary", |_| Arc::new(KeyedConfig { value: 9 }))
        .singleton(|r| __rdi_construct_KeyedConsumer(r))
        .build()
        .unwrap();
    let svc: Arc<KeyedConsumer> = provider.get().unwrap();
    assert_eq!(svc.config.value, 9);
    assert!(svc.missing.is_none());
}

// ─────────────────────────────────────────────────────────────────────────
// `#[module]` + `register!` through the renamed path
// ─────────────────────────────────────────────────────────────────────────

pub struct ModuleService {
    pub value: u8,
}

impl Default for ModuleService {
    fn default() -> Self {
        Self { value: 7 }
    }
}

#[dix::module]
mod rename_module {
    // Written through the *renamed* path on purpose: the scanner must accept
    // `<resolved crate>::register!(…)`, not only `rust_dix::register!(…)`.
    dix::register!(singleton: super::ModuleService);
}

#[test]
fn module_attribute_resolves_renamed_crate() {
    let provider = rename_module::__rdi_build_provider_rename_module().unwrap();
    let svc: Arc<ModuleService> = provider.get().unwrap();
    assert_eq!(svc.value, 7);
}
