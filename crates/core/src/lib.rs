//! rust-dix — Rust Dependency Injection Framework
//!
//! A dependency injection container for Rust inspired by
//! Microsoft.Extensions.DependencyInjection (MEDI).
//!
//! # Quick start
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! // Trait-based interface (recommended)
//! trait ILogger: Send + Sync {
//!     fn log(&self, msg: &str);
//! }
//!
//! struct ConsoleLogger;
//! impl ILogger for ConsoleLogger {
//!     fn log(&self, msg: &str) { println!("[LOG] {}", msg); }
//! }
//!
//! // Register as trait interface
//! let provider = ServiceCollection::new()
//!     .singleton::<dyn ILogger>(|_| Arc::new(ConsoleLogger))
//!     .build()
//!     .unwrap();
//!
//! // Resolve as trait
//! let logger: Arc<dyn ILogger> = provider.get().unwrap();
//! logger.log("Hello");
//! ```
//!
//! # Keyed services (strategy pattern)
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! trait Strategy: Send + Sync { fn execute(&self) -> &'static str; }
//!
//! struct FastStrategy;
//! impl Strategy for FastStrategy { fn execute(&self) -> &'static str { "fast" } }
//!
//! struct SafeStrategy;
//! impl Strategy for SafeStrategy { fn execute(&self) -> &'static str { "safe" } }
//!
//! let provider = ServiceCollection::new()
//!     .keyed_singleton::<dyn Strategy>("fast", |_| Arc::new(FastStrategy))
//!     .keyed_singleton::<dyn Strategy>("safe", |_| Arc::new(SafeStrategy))
//!     .build()
//!     .unwrap();
//!
//! let fast: Arc<dyn Strategy> = provider.get_keyed("fast").unwrap();
//! assert_eq!(fast.execute(), "fast");
//! ```
//!
//! # Build-time validation
//!
//! rust-dix validates dependencies during `build()` to catch errors early:
//!
//! ```rust
//! # use rust_dix::*;
//! # use std::sync::Arc;
//! # struct A;
//! # struct B;
//! // Circular dependency detection — factories resolve deps via `get_any`
//! // (the `get::<T>()` method requires `Self: Sized` and cannot be called
//! // on `&dyn IServiceResolver`).
//! let result = ServiceCollection::new()
//!     .singleton(|r| {
//!         let _b = r.get_any(std::any::type_name::<B>());  // A depends on B
//!         Arc::new(A)
//!     })
//!     .singleton(|r| {
//!         let _a = r.get_any(std::any::type_name::<A>());  // B depends on A → CIRCULAR!
//!         Arc::new(B)
//!     })
//!     .build();
//!
//! assert!(result.is_err());
//! if let Err(RdiError::CircularDependency(cycle)) = result {
//!     println!("Circular dependency detected: {}", cycle);
//! }
//! ```
//!
//! # Async support
//!
//! For services that require async initialization (e.g., database
//! connections, remote config loading), use `build_async()` and
//! the `async_*` registration methods. `build_async()` returns
//! `Arc<ServiceProvider>` so that `provider_arc()` is available for
//! async resolution:
//!
//! ```rust,ignore
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! struct DbPool { conn: String }
//!
//! async fn connect_to_db() -> DbPool {
//!     // async connection setup...
//!     DbPool { conn: "connected".into() }
//! }
//!
//! # async fn run() {
//! let provider = ServiceCollection::new()
//!     .async_singleton(|_| Box::pin(async {
//!         Arc::new(connect_to_db().await)
//!     }))
//!     .build_async()
//!     .await
//!     .unwrap();
//!
//! let pool: Arc<DbPool> = provider.get_async().await.unwrap();
//! # }
//! ```
//!
//! # Features
//!
//! - **Three lifetimes**: Singleton, Scoped, Transient
//! - **Keyed services**: multiple implementations of the same trait by key
//!   - `keyed_singleton(key, factory)` — shared globally
//!   - `keyed_scoped(key, factory)` — shared within scope
//!   - `keyed_transient(key, factory)` — new instance each time
//! - **Constructor injection**: `#[derive(Inject)]` on structs
//! - **Attribute-based auto-registration**: `#[rust_dix::inject]` on structs or trait impls
//! - **Compile-time module scanning**: `#[rust_dix::module]` collects `register!()` declarations
//! - **Cross-DLL support**: named service registry for cdylib plugins
//! - **Layered containers**: child-first resolution via `ServiceProviderWrapper`
//! - **Build-time validation**: circular dependency detection during `build()`
//!
//! # Crate layout
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`ServiceCollection`] | Register services with a builder API |
//! | [`ServiceProvider`] | The root DI container |
//! | [`Scope`] / [`ServiceScope`] | Scoped container (one per scope) |
//! | [`IServiceResolver`] | Resolution trait (implemented by `ServiceProvider` & `Scope`) |
//! | [`ServiceProviderWrapper`] | Child-first layered container |
//! | [`ServiceLifetime`] | Enum: Singleton, Scoped, Transient |
//! | [`IProvider`] | Unified trait for resolution + named registry |
//! | [`RdiError`] | Error types (includes `CircularDependency`) |
//!
//! # Proc-macros
//!
//! - `#[derive(Inject)]` — auto-generates constructor injection code
//! - `#[rust_dix::inject]` — auto-registration on structs or trait impls
//! - `#[rust_dix::module]` — compile-time module scanning
//! - `rust_dix::register!(...)` — declare services inside `#[rust_dix::module]`

pub mod bridge;
mod cache;
pub mod collection;
pub mod entry;
pub mod error;
#[doc(hidden)]
pub mod macros_support;
pub mod provider;
pub mod registration;
pub mod scope;
mod validation;
pub mod wrapper;

// ── Core types (MEDI-inspired naming) ──

/// Service collection — register services, then call `.build()`.
///
/// Analogous to `IServiceCollection` in Microsoft.Extensions.DependencyInjection.
pub use collection::ServiceCollection;

/// Built service provider — the root DI container (read-only after build).
///
/// 兼具 MEDI 中 `IServiceProvider` 与 root scope 双重身份：
/// - 作为 `IServiceProvider`：提供类型/键控/命名解析入口。
/// - 作为 root scope：从根直接解析 Scoped 服务时，实例在根内缓存复用，
///   语义等同于在根 scope 内单例化。通过 [`ServiceProvider::scope`] 创建的子 Scope
///   拥有独立的 scoped 缓存，不回退到根。
/// - 与 MEDI 对齐，容器自身可作为内建服务解析：`provider.get::<ServiceProvider>()`
///   与 `#[inject] Arc<ServiceProvider>`（或专用 `#[inject(provider)]`）均可用，
///   eager singleton 与懒解析服务一致。
///
/// Analogous to `IServiceProvider` in MEDI.
pub use provider::ServiceProvider;

/// Service descriptor containing registration metadata.
pub use entry::ServiceDescriptor;

/// Service lifetime enum: [`Singleton`](ServiceLifetime::Singleton),
/// [`Scoped`](ServiceLifetime::Scoped), [`Transient`](ServiceLifetime::Transient).
pub use entry::ServiceLifetime;

/// Core resolution trait. Both [`ServiceProvider`] and [`Scope`] implement this.
pub use entry::IServiceResolver;

/// A scoped service provider — created via [`ServiceProvider::scope`].
///
/// Analogous to `IServiceScope` in MEDI.
pub use scope::Scope;

/// Alias for [`Scope`] with MEDI-inspired naming (`IServiceScope`).
pub use scope::Scope as ServiceScope;

/// Factory trait for creating independent scopes.
///
/// The built provider always offers an implementation (no manual registration
/// required): `provider.get::<dyn ScopeFactory>()` and
/// `#[inject] Arc<dyn ScopeFactory>` fields both work, including inside
/// eagerly built singletons (e.g. background workers that need one scope per
/// tick). Explicit `singleton::<dyn ScopeFactory>(...)` registrations take
/// precedence.
///
/// The built-in implementation holds a `Weak` reference to the container, so a
/// singleton may store it without creating an `Arc` cycle; the container must
/// therefore outlive services created from it.
pub use scope::ScopeFactory;

/// Internal types used by the container.
pub use entry::{ServiceEntry, ServiceFactory};

/// Wrapper combining child + root provider with child-first resolution.
pub use wrapper::ServiceProviderWrapper;

// ── Plugin / cross-DLL support ──

/// Unified trait for service resolution and named registry access.
pub use bridge::IProvider;

// ── Error types ──

/// Error types returned from service resolution.
pub use error::{inject_arc, RdiError};

// ── Runtime registration ──

pub use registration::ServiceRegistration;

/// Re-export of `inventory` so that `#[rust_dix::inject]` generated code
/// can call `rust_dix::inventory::submit!` without requiring downstream crates
/// to add `inventory` to their own `Cargo.toml`.
#[doc(hidden)]
pub use inventory;

// ── Proc-macros ──

// `inject` is the attribute macro (placed on structs or trait impls).
// `register` is the function-like macro used inside `#[rust_dix::module]`.
pub use rust_dix_macros::{inject, module, register, Inject};
