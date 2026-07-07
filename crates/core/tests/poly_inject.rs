//! Polymorphic injection tests — `Vec<Arc<T>>` field collects all registered
//! implementations of a trait (or concrete type), equivalent to C# MEDI
//! `IEnumerable<IFoo>` constructor injection.
//!
//! Verifies:
//! - Baseline: `provider.get_all::<dyn IFoo>()` collects default + keyed.
//! - Macro: `#[inject]` on `Vec<Arc<dyn IFoo>>` produces the same collection.
//! - Empty Vec when no implementations registered (no panic).
//! - Concrete (non-trait) types also supported via `Vec<Arc<T>>`.

#![allow(non_snake_case)]

mod common;

use rust_dicore::*;
use rust_dicore_macros::Inject;
use std::sync::Arc;

// ── Shared trait + implementations for polymorphic tests ──

trait IFoo: Send + Sync {
    fn name(&self) -> &'static str;
}

struct FooA;
impl IFoo for FooA {
    fn name(&self) -> &'static str {
        "A"
    }
}

struct FooB;
impl IFoo for FooB {
    fn name(&self) -> &'static str {
        "B"
    }
}

struct FooC;
impl IFoo for FooC {
    fn name(&self) -> &'static str {
        "C"
    }
}

// ── Test 1: baseline — direct get_all without macro ──

#[test]
fn get_all_baseline() {
    let p = ServiceCollection::new()
        .singleton::<dyn IFoo>(|_| Arc::new(FooA))
        .keyed_singleton::<dyn IFoo>("b", |_| Arc::new(FooB))
        .keyed_singleton::<dyn IFoo>("c", |_| Arc::new(FooC))
        .build()
        .unwrap();

    let all: Vec<Arc<dyn IFoo>> = p.get_all();
    assert_eq!(all.len(), 3, "1 default + 2 keyed");

    let names: Vec<&str> = all.iter().map(|f| f.name()).collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
    assert!(names.contains(&"C"));
}

// ── Test 2: macro-driven Vec<Arc<dyn IFoo>> field injection ──

#[derive(Inject)]
struct FooConsumer {
    #[inject]
    foos: Vec<Arc<dyn IFoo>>,
}

#[test]
fn vec_inject_collects_all_implementations() {
    let p = ServiceCollection::new()
        .singleton::<dyn IFoo>(|_| Arc::new(FooA))
        .keyed_singleton::<dyn IFoo>("b", |_| Arc::new(FooB))
        .keyed_singleton::<dyn IFoo>("c", |_| Arc::new(FooC))
        .transient(|r| __rdi_construct_FooConsumer(r))
        .build()
        .unwrap();

    let consumer: FooConsumer = p.get_owned().unwrap();
    assert_eq!(consumer.foos.len(), 3, "should collect default + 2 keyed");

    let names: Vec<&str> = consumer.foos.iter().map(|f| f.name()).collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
    assert!(names.contains(&"C"));
}

// ── Test 3: empty Vec when no implementations registered ──

#[derive(Inject)]
struct EmptyFooConsumer {
    #[inject]
    foos: Vec<Arc<dyn IFoo>>,
}

#[test]
fn vec_inject_empty_when_unregistered() {
    let p = ServiceCollection::new()
        .transient(|r| __rdi_construct_EmptyFooConsumer(r))
        .build()
        .unwrap();

    let consumer: EmptyFooConsumer = p.get_owned().unwrap();
    assert!(
        consumer.foos.is_empty(),
        "no IFoo registered → empty Vec, no panic"
    );
}

// ── Test 4: concrete (non-trait) type polymorphic injection ──

#[derive(Debug, PartialEq)]
struct Widget {
    id: u32,
}

#[derive(Inject)]
struct WidgetConsumer {
    #[inject]
    widgets: Vec<Arc<Widget>>,
}

#[test]
fn vec_inject_concrete_type() {
    let p = ServiceCollection::new()
        .keyed_singleton::<Widget>("w1", |_| Arc::new(Widget { id: 1 }))
        .keyed_singleton::<Widget>("w2", |_| Arc::new(Widget { id: 2 }))
        .transient(|r| __rdi_construct_WidgetConsumer(r))
        .build()
        .unwrap();

    let consumer: WidgetConsumer = p.get_owned().unwrap();
    assert_eq!(consumer.widgets.len(), 2, "2 keyed Widgets collected");

    let ids: Vec<u32> = consumer.widgets.iter().map(|w| w.id).collect();
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
}

// ── Test 5: polymorphic injection through a Scope ──
//
// Verifies Vec<Arc<T>> field injection works when resolved via Scope,
// and scoped implementations are collected per-scope (independent instances).

#[derive(Inject)]
struct ScopedFooConsumer {
    #[inject]
    foos: Vec<Arc<dyn IFoo>>,
}

#[test]
fn vec_inject_works_through_scope() {
    let p = ServiceCollection::new()
        .singleton::<dyn IFoo>(|_| Arc::new(FooA))
        .keyed_scoped::<dyn IFoo>("b", |_| Arc::new(FooB))
        .transient(|r| __rdi_construct_ScopedFooConsumer(r))
        .build()
        .unwrap()
        .into_arc();

    let scope = p.scope();
    let consumer: ScopedFooConsumer = scope.get_owned().unwrap();
    assert_eq!(consumer.foos.len(), 2, "1 singleton + 1 scoped");

    let names: Vec<&str> = consumer.foos.iter().map(|f| f.name()).collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
}
