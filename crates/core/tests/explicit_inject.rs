//! Tests for the explicit field injection semantics introduced in 0.6.0.
//!
//! Invariants verified:
//! - Unmarked fields use `Default::default()` and never consult the resolver.
//! - `#[inject]` marks shared injection (Arc<T> / Option<Arc<T>>).
//! - `#[inject(owned)]` marks owned injection (bare T / Option<T>).
//! - `#[inject(provider)]` injects `Arc<ServiceProvider>` for on-demand resolution.
//! - Optionality is driven by the field type (`Option<...>`), not by a marker.

#![allow(non_snake_case)]
#![allow(dead_code)]

mod common;

use rust_dicore::*;
use rust_dicore_macros::Inject;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────
// Unmarked fields use Default and never touch the resolver.
//
// `Logger` is registered as a singleton below, yet the unmarked `internal`
// field must remain `Default::default()` (prefix == "").
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct UnmarkedHandler {
    #[inject]
    dep: Arc<common::Logger>,
    internal: common::Logger,
    label: String,
}

#[test]
fn unmarked_field_uses_default_even_when_type_registered() {
    let p = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "registered".into(),
            })
        })
        .transient(|r| __rdi_construct_UnmarkedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<UnmarkedHandler>().unwrap();
    assert_eq!(h.dep.prefix, "registered", "marked Arc<T> field injects");
    assert_eq!(
        h.internal.prefix, "",
        "unmarked field must use Default, not resolve from container"
    );
    assert_eq!(h.label, "", "unmarked String field uses Default");
}

// ─────────────────────────────────────────────────────────────────
// #[inject] on Option<Arc<T>> returns None when dependency absent.
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct OptionalSharedHandler {
    #[inject]
    maybe: Option<Arc<common::MyService>>,
}

#[test]
fn inject_option_arc_returns_none_when_absent() {
    let p = ServiceCollection::new()
        .transient(|r| __rdi_construct_OptionalSharedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<OptionalSharedHandler>().unwrap();
    assert!(h.maybe.is_none(), "absent shared dep → None, no panic");
}

#[test]
fn inject_option_arc_returns_some_when_present() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 7 }))
        .transient(|r| __rdi_construct_OptionalSharedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<OptionalSharedHandler>().unwrap();
    assert_eq!(h.maybe.as_ref().unwrap().value, 7);
}

// ─────────────────────────────────────────────────────────────────
// #[inject(owned)] on Option<T> returns None when dependency absent.
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct OptionalOwnedHandler {
    #[inject(owned)]
    maybe: Option<common::MyService>,
}

#[test]
fn inject_owned_option_returns_none_when_absent() {
    let p = ServiceCollection::new()
        .transient(|r| __rdi_construct_OptionalOwnedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<OptionalOwnedHandler>().unwrap();
    assert!(h.maybe.is_none(), "absent owned dep → None, no panic");
}

#[test]
fn inject_owned_option_returns_some_when_present() {
    let p = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 11 }))
        .transient(|r| __rdi_construct_OptionalOwnedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<OptionalOwnedHandler>().unwrap();
    assert_eq!(h.maybe.as_ref().unwrap().value, 11);
}

// ─────────────────────────────────────────────────────────────────
// Mixed: marked + unmarked + keyed shared + keyed owned.
// ─────────────────────────────────────────────────────────────────

#[derive(Inject)]
struct MixedMarkedHandler {
    #[inject]
    shared: Arc<common::Logger>,
    #[inject(key = "audit")]
    audit: Arc<common::Logger>,
    #[inject(owned)]
    owned_svc: common::MyService,
    #[inject(owned, key = "primary")]
    keyed_owned: common::MyService,
    // Unmarked internal fields.
    counter: u32,
    tag: String,
}

#[test]
fn mixed_marked_and_unmarked_fields() {
    let p = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "main".into(),
            })
        })
        .keyed_singleton("audit", |_| {
            Arc::new(common::Logger {
                prefix: "audit".into(),
            })
        })
        .transient(|_| Arc::new(common::MyService { value: 1 }))
        .keyed_transient("primary", |_| Arc::new(common::MyService { value: 2 }))
        .transient(|r| __rdi_construct_MixedMarkedHandler(r))
        .build()
        .unwrap();

    let h = p.get_owned::<MixedMarkedHandler>().unwrap();
    assert_eq!(h.shared.prefix, "main");
    assert_eq!(h.audit.prefix, "audit");
    assert_eq!(h.owned_svc.value, 1);
    assert_eq!(h.keyed_owned.value, 2);
    assert_eq!(h.counter, 0, "unmarked u32 uses Default");
    assert_eq!(h.tag, "", "unmarked String uses Default");
}
