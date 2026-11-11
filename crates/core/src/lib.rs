#![forbid(unsafe_code)]

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
//! # Keyed services: typed keys and `AnyKey` fallbacks
//!
//! Keys can be any `Any + Send + Sync + Eq + Hash + Debug` value
//! ([`ServiceKey::new`]); string keys (`"fast"`) remain supported and are stored
//! as `String` keys. Key equality includes the value type, so
//! `ServiceKey::new(1u32)` is **not** equal to `"1"`; use
//! `ServiceKey::from("name")` / `"name".into()` for string-keyed registrations.
//!
//! The injection macros accept any **expression** convertible to
//! [`ServiceKey`] — string literals keep working:
//! `#[inject(key = "fast")]`, `#[inject(key = ServiceKey::new(Kind::Fast))]`, or a
//! downstream key value implementing `From<LocalKey> for ServiceKey` (the
//! official extension path for frameworks; do not duplicate the injection
//! macros). See the repository reference chapter for details:
//! <https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md>.
//!
//! A registration with [`KeyedService::any_key`] (or
//! [`ServiceKey::any_key`]) acts as a fallback for keys that have no exact
//! registration. Singleton/scoped fallback instances are cached per
//! **(registration, requested key)**; `get_all_keyed` never enumerates them.
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! #[derive(Debug, PartialEq, Eq, Hash)]
//! enum Kind { Fast, Safe }
//!
//! trait Strategy: Send + Sync { fn name(&self) -> &'static str; }
//! struct Fast;
//! impl Strategy for Fast { fn name(&self) -> &'static str { "fast" } }
//! struct Fallback;
//! impl Strategy for Fallback { fn name(&self) -> &'static str { "fallback" } }
//!
//! let provider = ServiceCollection::new()
//!     .keyed_singleton::<dyn Strategy>(ServiceKey::new(Kind::Fast), |_| Arc::new(Fast))
//!     .keyed_any_singleton::<dyn Strategy>(|_, _| Arc::new(Fallback))
//!     .build()
//!     .unwrap();
//!
//! let fast = provider.get_keyed::<dyn Strategy>(ServiceKey::new(Kind::Fast)).unwrap();
//! assert_eq!(fast.name(), "fast");
//! // No exact registration for "unknown": the AnyKey fallback is used.
//! let fallback = provider.get_keyed::<dyn Strategy>("unknown").unwrap();
//! assert_eq!(fallback.name(), "fallback");
//! ```
//!
//! For typed-key constructor injection:
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! #[derive(Debug, PartialEq, Eq, Hash)]
//! enum Channel { Primary }
//! struct Sink;
//!
//! #[derive(Inject)]
//! struct Consumer {
//!     #[inject(key = ServiceKey::new(Channel::Primary))]
//!     sink: Arc<Sink>,
//! }
//!
//! let provider = ServiceCollection::new()
//!     .keyed_singleton(ServiceKey::new(Channel::Primary), |_| Arc::new(Sink))
//!     .transient(|r| __rdi_construct_Consumer(r))
//!     .build()
//!     .unwrap();
//! let consumer: Arc<Consumer> = provider.get().unwrap();
//! let _ = &consumer.sink;
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
//! # Async services
//!
//! For services that require async initialization (e.g., database
//! connections, remote config loading), use `build_async()` and the `async_*`
//! registration methods. Async singletons are initialized **dependency-first**
//! (registration order does not matter), so a factory may `await` another async
//! singleton registered later. `build_async()` returns `Arc<ServiceProvider>`.
//!
//! Async resolution is **chain-based**: each `get_async()` future captures the
//! current resolution chain when constructed, so self/cross dependency cycles
//! are reported as [`RdiError::CircularDependency`] instead of dead-locking.
//! To detect cycles across detached tasks, propagate
//! [`AsyncFactoryContext`] (see [`ServiceProvider::async_context`]) into the
//! spawned task; without it, a fully detached task cannot inherit the chain.
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! struct DbPool { url: String }
//!
//! async fn connect() -> DbPool { DbPool { url: "sqlite::memory:".into() } }
//!
//! let provider = futures::executor::block_on(
//!     ServiceCollection::new()
//!         .async_singleton(|_| Box::pin(async { Arc::new(connect().await) }))
//!         .build_async(),
//! )
//! .unwrap();
//!
//! let pool: Arc<DbPool> = futures::executor::block_on(provider.get_async()).unwrap();
//! assert_eq!(pool.url, "sqlite::memory:");
//! ```
//!
//! # Features
//!
//! - **Three lifetimes**: Singleton, Scoped, Transient
//! - **Keyed services**: multiple implementations by key
//!   - string keys (`"fast"`) and typed keys ([`ServiceKey::new`], e.g. enums)
//!   - [`KeyedService::any_key`] / [`ServiceKey::any_key`] fallback registrations
//!     (`keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`)
//!   - per-`(registration, requested key)` caching for AnyKey fallbacks; exact
//!     registrations take precedence; `get_all_keyed` excludes fallbacks
//! - **Constructor injection**: `#[derive(Inject)]` on structs
//! - **Attribute-based auto-registration**: `#[rust_dix::inject]` on structs or
//!   trait impls (`#[inject(key = <expr>)]`, `#[inject(owned)]`,
//!   `#[inject(owned, key = <expr>)]`; key expressions accept string literals,
//!   typed [`ServiceKey`] values, or downstream key types via
//!   `From<LocalKey> for ServiceKey`)
//! - **Compile-time module scanning**: `#[rust_dix::module]` collects
//!   `register!()` declarations
//! - **Cross-DLL support**: named service registry for cdylib plugins
//! - **Layered containers**: child-first resolution via
//!   [`ServiceProviderWrapper`]
//! - **Build-time validation**: sync/async dependency checks (cycles, captive
//!   dependencies, sync singleton depending on async services) during build
//! - **Async resolution with cycle detection**: [`ServiceProvider::get_async`] /
//!   [`ServiceProvider::get_keyed_async`]; propagate [`AsyncFactoryContext`] into
//!   detached tasks so cross-task cycles also return
//!   [`RdiError::CircularDependency`]
//! - **Scope validation**: scoped services cannot be resolved from the root
//!   provider by default (MEDI `ValidateScopes` parity; opt out with
//!   [`ServiceProviderOptions::lenient`]); `get_owned` /
//!   `get_keyed_owned` are the documented exception
//! - **Deterministic lifecycle**: eager singletons are created in registration
//!   order (async singletons dependency-first); [`ServiceProvider::dispose`] /
//!   [`Scope::dispose`] release cached instances in reverse creation order
//! - **MEDI registration semantics**: the last registration wins for a single
//!   service; `get_all` returns unkeyed implementations in registration order
//!   (`get_all_keyed` for keyed ones)
//! - **Typed diagnostics**: [`RdiError`] exposes dedicated variants such as
//!   `SyncSingletonDependsOnAsyncService` and `AsyncServiceRequiresGetAsync`,
//!   plus [`RdiError::is_circular`] (heuristic for factory-panic-wrapped cycles)
//! - **Engineering baseline**: MSRV **1.95**, `#![forbid(unsafe_code)]`,
//!   fmt/clippy/doc `-D warnings` + tests + RustSec `cargo audit` in CI
//!
//! # Diagnostics
//!
//! Prefer matching the typed [`RdiError`] variants over strings:
//!
//! ```rust
//! use rust_dix::*;
//! use std::sync::Arc;
//!
//! struct A;
//! struct B;
//! let result = ServiceCollection::new()
//!     .singleton(|r| {
//!         let _ = r.get_any(std::any::type_name::<B>());
//!         Arc::new(A)
//!     })
//!     .singleton(|r| {
//!         let _ = r.get_any(std::any::type_name::<A>());
//!         Arc::new(B)
//!     })
//!     .build();
//!
//! let err = result.err().expect("cycle must be rejected");
//! assert!(err.is_circular());
//! if let RdiError::CircularDependency(cycle) = err {
//!     assert!(cycle.contains("->"));
//! }
//! ```
//!
//! Sync resolution of an async service returns
//! `RdiError::AsyncServiceRequiresGetAsync` (use `get_async` / `get_keyed_async`);
//! registering `async_*` services and calling [`ServiceCollection::build`]
//! returns `RdiError::AsyncServiceRequiresBuildAsync`.
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
//! | [`ServiceKey`] / [`KeyedService`] | Typed keys and the `AnyKey` sentinel |
//! | [`AsyncFactoryContext`] | Propagate the async resolution chain into tasks |
//! | [`ServiceProviderOptions`] | Scope-validation options (`lenient()`) |
//! | [`IProvider`] | Unified trait for resolution + named registry |
//! | [`RdiError`] | Typed errors (cycles, disposed, async diagnostics, ...) |
//!
//! # MSRV and security
//!
//! - MSRV: **Rust 1.95** (`rust-version` in `Cargo.toml`, verified in CI).
//! - `#![forbid(unsafe_code)]` in both `rust-dix` and `rust-dix-macros`.
//! - No network/filesystem access; containers only manage in-memory services.
//! - Release/security policy: <https://github.com/Rust-Framework/rust-dix/blob/main/SECURITY.md>
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
pub mod key;
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
/// - 作为 root scope：**默认启用严格 scope 校验**（对齐 MEDI/ASP.NET Core 开发环境的
///   `ValidateScopes`），从根解析 Scoped 服务返回 [`RdiError::ScopedFromRootProvider`]；
///   在 [`ServiceProvider::scope`] 创建的 Scope 内解析则正常缓存复用。若确实需要
///   0.7 的"根 scope 内单例化"行为，可用
///   [`ServiceProviderOptions::lenient`] + [`ServiceCollection::build_with`]。
/// - 与 MEDI 对齐，容器自身可作为内建服务解析：`provider.get::<ServiceProvider>()`
///   与 `#[inject] Arc<ServiceProvider>`（或专用 `#[inject(provider)]`）均可用，
///   eager singleton 与懒解析服务一致。
/// - [`ServiceProvider::dispose`] 按创建逆序释放 singleton / root-scoped 实例并
///   标记容器不可用（后续解析返回 [`RdiError::ProviderDisposed`]）。
/// - ⚠️ singleton 持有 `Arc<ServiceProvider>` 会形成 `容器 → singleton → 容器` 的
///   强引用环，容器不再可能被回收；需要按任务创建 scope 的 singleton 应优先注入
///   [`ScopeFactory`]（内置实现为弱引用，无环）。构造函数内也不要通过注入的
///   provider 解析服务（会被拒绝），请保存句柄并在方法中延迟解析。
///
/// Analogous to `IServiceProvider` in MEDI.
pub use provider::ServiceProvider;

/// Key used by keyed registrations, including the `AnyKey` fallback sentinel.
pub use key::{KeyedService, ServiceKey};

/// Handle for propagating the async resolution chain into detached tasks
/// (enables cross-task dependency cycle detection).
pub use provider::AsyncFactoryContext;

/// Options for [`ServiceCollection::build_with`] /
/// [`ServiceCollection::build_async_with`].
///
/// Defaults match MEDI/ASP.NET Core development-time scope validation:
/// `validate_scopes = true`. Use [`ServiceProviderOptions::lenient`] to allow
/// resolving scoped services from the root provider (pre-0.8 behavior).
pub use provider::ServiceProviderOptions;

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
