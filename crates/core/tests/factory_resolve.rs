//! DIX-3 regression: typed resolution from inside factory closures.
//!
//! Factories receive `&dyn IServiceResolver`; the generic `get::<T>()` has a
//! `Self: Sized` bound and therefore cannot be called through the trait object.
//! Before DIX-3 the only option was `get_any(type_name::<T>())` + manual
//! `downcast`, which silently breaks when a type is renamed.
//!
//! Covered here:
//! - `resolver.resolve_arc::<T>()` in a fallible factory helper (`?` works);
//! - `resolver.resolve_arc::<dyn Trait>()`;
//! - existing dyn-safe `resolver.get_by_type_id(TypeId)` (no extra API needed);
//! - `resolve_arc` reports framework-level failures as `Err`, never panics;
//! - `get_any` stays available (source compatibility).

mod common;

use rust_dix::*;
use std::any::TypeId;
use std::sync::Arc;

pub trait IClock: Send + Sync {
    fn now(&self) -> u64;
}

pub struct FakeClock;

impl IClock for FakeClock {
    fn now(&self) -> u64 {
        11
    }
}

pub struct Handler {
    pub logger: Arc<common::Logger>,
    pub clock: Arc<dyn IClock>,
}

/// Fallible construction helper: `?` on the typed resolver API is the point.
fn build_handler(resolver: &dyn IServiceResolver) -> Result<Arc<Handler>, RdiError> {
    let logger: Arc<common::Logger> = resolver.resolve_arc()?;
    let clock: Arc<dyn IClock> = resolver.resolve_arc()?;
    Ok(Arc::new(Handler { logger, clock }))
}

#[test]
fn factory_resolves_typed_dependencies() {
    let p = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "factory".into(),
            })
        })
        .singleton(|_| Arc::new(FakeClock) as Arc<dyn IClock>)
        .singleton(|r| build_handler(r).expect("dependencies are registered"))
        .build()
        .unwrap();

    let handler: Arc<Handler> = p.get().unwrap();
    assert_eq!(handler.logger.prefix, "factory");
    assert_eq!(handler.clock.now(), 11);
}

#[test]
fn get_by_type_id_resolves_inside_factory() {
    let p = ServiceCollection::new()
        .singleton(|_| {
            Arc::new(common::Logger {
                prefix: "by-id".into(),
            })
        })
        .singleton(|r| {
            let erased = r
                .get_by_type_id(TypeId::of::<common::Logger>())
                .expect("registered");
            let arc = erased.downcast::<Arc<common::Logger>>().expect("shape");
            Arc::new(Publisher {
                logger: Arc::clone(&*arc),
            })
        })
        .build()
        .unwrap();

    let publisher: Arc<Publisher> = p.get().unwrap();
    assert_eq!(publisher.logger.prefix, "by-id");
}

pub struct Publisher {
    pub logger: Arc<common::Logger>,
}

#[test]
fn resolve_arc_reports_missing_dependency_typed() {
    let p = ServiceCollection::new().build().unwrap();
    let err = p.resolve_arc::<common::MyService>().unwrap_err();
    let text = err.to_string();
    assert!(text.contains("MyService"), "unexpected error: {text}");
}

#[test]
fn resolve_arc_returns_err_for_registered_but_unresolvable_service() {
    // A scoped service cannot be resolved from the root provider under strict
    // scope validation. The container's `get` reports a typed error; the
    // dyn-aware `resolve_arc` must match that contract instead of leaking the
    // internal `panic_any(RdiError)` used by `get_by_type_id`.
    let p = ServiceCollection::new()
        .scoped(|_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();

    assert!(
        p.get::<common::MyService>().is_err(),
        "scoped-at-root must be a typed error"
    );
    assert!(
        p.resolve_arc::<common::MyService>().is_err(),
        "resolve_arc must return Err, not panic, for framework-level failures"
    );
}
