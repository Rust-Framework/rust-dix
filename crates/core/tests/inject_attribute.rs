//! Tests for the `#[rust_dicore::inject]` attribute macro.
//!
//! Covers both placement sites:
//! - On a struct: generates constructor + registers as concrete type
//! - On a trait impl: registers as the trait type (auto-detected)
//!
//! Note: `inventory` collects registrations globally across the binary, so each
//! test defines its own trait to avoid `dyn Trait` collisions.

#![allow(non_snake_case)]

mod common;

use rust_dicore::*;
use rust_dicore_macros::Inject;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────
// Struct placement: concrete type registration
// ─────────────────────────────────────────────────────────────────

/// Default lifetime (singleton) when no args given.
#[rust_dicore::inject]
struct DefaultLifetimeService;

#[test]
fn inject_on_struct_default_lifetime() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let svc: Arc<DefaultLifetimeService> = p.get().unwrap();
    let svc2: Arc<DefaultLifetimeService> = p.get().unwrap();
    assert!(Arc::ptr_eq(&svc, &svc2));
}

/// Explicit singleton with a skipped field.
#[rust_dicore::inject(singleton)]
struct ExplicitSingleton {
    label: String,
}

#[test]
fn inject_on_struct_explicit_singleton() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let svc: Arc<ExplicitSingleton> = p.get().unwrap();
    assert_eq!(svc.label, "");
}

/// Transient lifetime — new instance each time.
#[rust_dicore::inject(transient)]
struct TransientService;

#[test]
fn inject_on_struct_transient() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let a: Arc<TransientService> = p.get().unwrap();
    let b: Arc<TransientService> = p.get().unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
}

/// Struct with field injection from another auto-registered service.
#[rust_dicore::inject]
#[allow(dead_code)]
struct FieldInjectionService {
    #[inject]
    dep: Arc<DefaultLifetimeService>,
    #[inject]
    maybe: Option<Arc<ExplicitSingleton>>,
}

#[test]
fn inject_on_struct_with_fields() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let svc: Arc<FieldInjectionService> = p.get().unwrap();
    assert!(svc.maybe.is_some());
}

// ─────────────────────────────────────────────────────────────────
// Impl-block placement: trait registration
//
// Each test defines its OWN trait to avoid global inventory collisions.
// ─────────────────────────────────────────────────────────────────

trait IThingA: Send + Sync {
    fn name(&self) -> &str;
}

/// Constructor via #[derive(Inject)]; trait registration via #[inject] on impl.
#[derive(Inject)]
struct TraitOnlyRegistration;

#[rust_dicore::inject]
impl IThingA for TraitOnlyRegistration {
    fn name(&self) -> &str {
        "trait_only"
    }
}

#[test]
fn inject_on_impl_trait_registration() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let svc: Arc<dyn IThingA> = p.get::<dyn IThingA>().unwrap();
    assert_eq!(svc.name(), "trait_only");
}

// ─────────────────────────────────────────────────────────────────
// Combined: struct placement (concrete) + impl placement (trait)
// ─────────────────────────────────────────────────────────────────

trait IThingB: Send + Sync {
    fn kind(&self) -> &str;
}

#[rust_dicore::inject] // concrete registration + constructor
struct DualRegistered;

#[rust_dicore::inject] // trait registration
impl IThingB for DualRegistered {
    fn kind(&self) -> &str {
        "dual"
    }
}

#[test]
fn inject_combined_concrete_and_trait() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let concrete: Arc<DualRegistered> = p.get().unwrap();
    let trait_obj: Arc<dyn IThingB> = p.get::<dyn IThingB>().unwrap();
    assert_eq!(trait_obj.kind(), "dual");
    // Concrete is also usable as the trait
    let concrete_as_trait: Arc<dyn IThingB> = concrete;
    assert_eq!(concrete_as_trait.kind(), "dual");
}

// ─────────────────────────────────────────────────────────────────
// Multiple traits: #[inject] on multiple impl blocks
// ─────────────────────────────────────────────────────────────────

trait IThingC: Send + Sync {
    fn c(&self) -> &str;
}
trait IThingD: Send + Sync {
    fn d(&self) -> &str;
}

#[rust_dicore::inject] // constructor + concrete registration
struct MultiTraitService;

#[rust_dicore::inject]
impl IThingC for MultiTraitService {
    fn c(&self) -> &str {
        "c"
    }
}

#[rust_dicore::inject]
impl IThingD for MultiTraitService {
    fn d(&self) -> &str {
        "d"
    }
}

#[test]
fn inject_multiple_traits() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let c: Arc<dyn IThingC> = p.get::<dyn IThingC>().unwrap();
    let d: Arc<dyn IThingD> = p.get::<dyn IThingD>().unwrap();
    assert_eq!(c.c(), "c");
    assert_eq!(d.d(), "d");
}

// ─────────────────────────────────────────────────────────────────
// Scoped lifetime on impl block
// ─────────────────────────────────────────────────────────────────

trait IThingE: Send + Sync {
    fn e(&self) -> &str;
}

#[derive(Inject)]
struct ScopedTraitImpl;

#[rust_dicore::inject(scoped)]
impl IThingE for ScopedTraitImpl {
    fn e(&self) -> &str {
        "scoped"
    }
}

#[test]
fn inject_on_impl_scoped() {
    let p = ServiceCollection::from_injected().build().unwrap();
    let scope1 = p.scope();
    let scope2 = p.scope();
    let a: Arc<dyn IThingE> = scope1.get().unwrap();
    let b: Arc<dyn IThingE> = scope2.get().unwrap();
    assert_eq!(a.e(), "scoped");
    assert_eq!(b.e(), "scoped");
    assert!(!Arc::ptr_eq(&a, &b));
}
