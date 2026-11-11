<p align="center">
  <img src="../assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix · Dependency Injection Container</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/Rust-Framework/rust-dix/blob/main/crates/core/README.zh.md"><strong>中文</strong></a>
</p>

`rust-dix` is the core crate of the rust-dix framework — a Rust dependency injection
container inspired by Microsoft.Extensions.DependencyInjection (MEDI).

```toml
[dependencies]
rust-dix = "0.9"
```

> **MSRV**: Rust **1.95**. Both crates are `#![forbid(unsafe_code)]`; CI runs a
> test matrix plus RustSec `cargo audit`. See `SECURITY.md` at the repository root.

---

### When should you use this?

**1. You want to decouple service creation from usage**

Instead of `MyService::new(dep1, dep2, dep3)` scattered across your codebase,
you declare once how services are constructed and let the container wire them
up automatically.

```rust
// Without DI: every call site must know how to build everything
let svc = MyService::new(
    Arc::new(Logger::new("app")),
    Arc::new(Cache::new(1024)),
);

// With DI: declare once, resolve anywhere.
// `MyService` is annotated with `#[rust_dix::inject(transient)]`, so it is
// registered automatically (its definition is omitted here).
let provider = ServiceCollection::from_injected()
    .singleton(|_| Arc::new(Logger::new("app")))
    .singleton(|_| Arc::new(Cache::new(1024)))
    .build()
    .unwrap();

let svc: Arc<MyService> = provider.get().unwrap();
```

**2. You need to swap implementations for testing**

Change one registration line — no need to touch the code that consumes the
service.

```rust
#[cfg(test)]
fn test_provider() -> ServiceProvider {
    // `MyApi` is annotated with `#[rust_dix::inject(transient)]` and collected by
    // `from_injected()`; only the mock has to be registered here.
    ServiceCollection::from_injected()
        .singleton(|_| Arc::new(MockDatabase::new()))
        .build()
        .unwrap()
}
```

**3. You have objects with different lifetimes**

Some services should be shared globally (database pool), some per-request
(HTTP context), some created fresh each time (value objects). rust-dix gives you
three lifetimes to model this naturally.

| Lifetime | Behavior | Typical use |
|----------|----------|-------------|
| **Singleton** | Created once, shared everywhere | Database pool, config, event bus |
| **Scoped** | Created once per scope, shared within scope | HTTP request context, unit of work, transaction |
| **Transient** | Created every time you resolve it | Value objects, DTOs, lightweight stateless services |

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))           // one pool for the app
    .scoped(|_| Arc::new(RequestContext::new()))     // one per request
    .transient(|_| Arc::new(Query::new()))           // new each time
    .build()
    .unwrap();
```

**4. You want multiple instances of the same type**

Keyed services let you register several implementations of the same trait,
each distinguished by a key — a string or a typed `ServiceKey` value. All three
lifetimes are supported:

```rust
// Keyed with different lifetimes
let provider = ServiceCollection::new()
    // Singleton keyed - shared globally
    .keyed_singleton::<dyn Strategy>("fast", |_| Arc::new(FastPath))
    // Scoped keyed - shared within a scope
    .keyed_scoped::<dyn Strategy>("session", |_| Arc::new(SessionStrategy))
    // Transient keyed - new instance each time
    .keyed_transient::<dyn Strategy>("fresh", |_| Arc::new(FreshStrategy))
    // AnyKey fallback: used when no exact key is registered (cached per (registration, requested key))
    .keyed_any_singleton::<dyn Strategy>(|_, _key| Arc::new(DefaultStrategy))
    .build()
    .unwrap();

let fast: Arc<dyn Strategy> = provider.get_keyed("fast").unwrap();
let session: Arc<dyn Strategy> = provider.scope().get_keyed("session").unwrap();
let fresh1: Arc<dyn Strategy> = provider.get_keyed("fresh").unwrap();
let fresh2: Arc<dyn Strategy> = provider.get_keyed("fresh").unwrap();  // different instance
// Typed keys work too: provider.get_keyed(ServiceKey::new(StrategyKey::Fast))
```

**5. You are building a plugin / modular system**

- **Layered containers**: child-first, root-fallback resolution for plugin
  isolation. The plugin sees its own services first; host services are visible
  as fallback.
- **Named services**: register services by string name — critical for cdylib
  plugins where Rust's `TypeId` differs across compilation units.
- **`IProvider`**: pass a unified DI interface (resolution + named registry) to
  external modules that don't depend on rust-dix directly.

---

### Quick Reference · API

### Registration (`ServiceCollection`)

| Method | Lifetime | Use when |
|--------|----------|----------|
| `.from_injected()` | *mixed* | Collect all `#[rust_dix::inject]` annotations (see below) |
| `.singleton(f)` | Singleton | You need exactly one instance shared globally |
| `.scoped(f)` | Scoped | You need one instance per scope (request/transaction) |
| `.transient(f)` | Transient | You need a new instance every time |
| `.keyed_singleton(k, f)` | Singleton | Multiple keyed instances (strategy pattern); `k: impl Into<ServiceKey>` |
| `.keyed_scoped(k, f)` | Scoped | Keyed instances scoped per request |
| `.keyed_transient(k, f)` | Transient | Keyed instances created fresh each time |
| `.keyed_any_singleton(f)` | Singleton | `AnyKey` fallback (MEDI parity); `f: Fn(&dyn IServiceResolver, &ServiceKey) -> Arc<T>`, cached per (registration, requested key) |
| `.keyed_any_scoped(f)` | Scoped | `AnyKey` fallback, one instance per scope per (registration, requested key) |
| `.keyed_any_transient(f)` | Transient | `AnyKey` fallback, fresh per resolution |
| `.instance(arc)` | Singleton | You already have a built `Arc<T>` |
| `.try_add(f)` | Singleton | Only register if not already present (MEDI `TryAdd`) |
| `.try_add_scoped(f)` | Scoped | Only register if not already present |
| `.try_add_transient(f)` | Transient | Only register if not already present |
| `.singleton_value(v)` | Singleton | Register a plain value (wraps in `Arc`) |
| `.add(lt, f)` | *any* | Specify lifetime explicitly |
| `.build()` | — | Build the provider with default (strict) scope validation |
| `.build_with(options)` | — | Build with explicit `ServiceProviderOptions` (`lenient()` restores 0.7.x root-scope behavior) |
| `.build_async()` / `.build_async_with(options)` | — | Async build; required when any `async_*` registration exists |

#### Typed keys (`ServiceKey`) and `AnyKey` fallback

Keyed APIs accept `impl Into<ServiceKey>`: strings (`&str` / `String`) keep working, and
`ServiceKey::new(value)` supports enum/integer/custom keys. Key equality includes the
value's type — `ServiceKey::new(1u32) != ServiceKey::from("1")`. **String-key footgun**:
`ServiceKey::new("x")` is a `&str`-typed key and is *not* equal to the `String` key stored
by `keyed_singleton("x", …)`; use `"x".into()` / `ServiceKey::from("x")` for string keys
and `ServiceKey::new` only for typed (`&'static str` / enum / integer) keys.

```rust
use rust_dix::*;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Hash)]
enum DbKey { Read, Write }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IDbConnection>(ServiceKey::new(DbKey::Read), |_| Arc::new(ReadPool))
    .keyed_any_singleton::<dyn IDbConnection>(|_, key| {
        // `key` is the requested ServiceKey (exact keys win over this fallback).
        Arc::new(Pool::connect(&format!("{key:?}")))
    })
    .build()
    .unwrap();

let read: Arc<dyn IDbConnection> = provider.get_keyed(ServiceKey::new(DbKey::Read)).unwrap();
```

`AnyKey` semantics (MEDI `KeyedService.AnyKey` parity):
- exact-key registrations take precedence over the fallback;
- singleton/scoped fallback instances are cached **per (registration/type, requested
  key)** — different `AnyKey` registrations stay independent even for the same requested
  key; scoped: per scope per (registration, key). Transient creates a fresh instance per
  resolution;
- `get_all_keyed` returns explicit keys only — the fallback is excluded;
- resolving a single service **with** the any-key sentinel is invalid
  (`get_keyed(ServiceKey::any_key())` → `Err`; `KeyedService::any_key()` is the same
  sentinel; `is_registered_keyed(any_key())` → `false`);
- `AnyKey` registrations are sync-only (async resolution falls back to the sync path),
  are not eagerly initialized/validated at build (they need a requested key), and a
  scoped fallback captured by a sync singleton is still a captive dependency. During
  build-time validation a sync singleton factory may resolve an `AnyKey` dependency by
  passing the requested key via `get_keyed_any`;
- a singleton fallback consumed during build-time validation is **written into the
  (registration, requested key) cache**, so runtime resolution with the same registration
  + key reuses the same instance (single initialization; multiple consumers share one
  construction — no double construction);
- `get_keyed_owned` / `try_get_keyed_owned` for an `AnyKey` **scoped/transient** fallback
  is allowed at the strict root (same caller-owned exception as exact-key owned; not
  cached/tracked, transitive dependencies still validated);
- an `AnyKey` factory may nested-resolve a **different** requested key (legal); only
  re-entering the same registration with the same requested key is a cycle;
- the factory receives `&ServiceKey` with `Debug` / `Display` / `Eq` / `Hash`; there is
  intentionally **no** typed downcast accessor (the erased representation is private).

The factory closure `f` receives `&dyn IServiceResolver`. Generic `get::<T>()`
requires `Self: Sized`, so it cannot be called on that trait object: let the
inject macros generate the constructor (internal `__rdi_construct_*`,
`#[doc(hidden)]`), or resolve with `get_any` + downcast:

```rust
ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))
    // `Repo` is `#[derive(Inject)]` with `#[inject] pool: Arc<Pool>`; the
    // generated constructor resolves its fields during registration.
    .transient(|r| __rdi_construct_Repo(r))
    .build()
    .unwrap();
```

#### `#[rust_dix::inject]` — Attribute-based auto-registration (recommended)

The preferred way to register services is to annotate structs (or trait impl
blocks) directly:

```rust
use rust_dix::*;

// Annotate a struct → registered as its concrete type.
// Lifetime defaults to `singleton` when omitted.
#[rust_dix::inject]
struct Config { url: String }

#[rust_dix::inject(transient)]
struct Worker { logger: Arc<Config> }

// Annotate a trait impl → registered as `dyn Trait`.
// The trait type is auto-detected from `impl Trait for Type`,
// so no `as = dyn Trait` boilerplate is needed.
trait UserRepo { /* ... */ }

// For trait-oriented services, use #[derive(Inject)] on the struct (generates
// a constructor but does NOT register) and #[inject] on the impl block only.
// Putting #[inject] on BOTH struct and impl causes double registration.
#[derive(Inject)]
struct PgUserRepo { db: Arc<DbPool> }

#[rust_dix::inject]              // on the impl: trait registration (singleton by default)
impl UserRepo for PgUserRepo { /* ... */ }

// Then build from all annotations in one call:
let provider = ServiceCollection::from_injected().build().unwrap();
```

The `#[rust_dix::inject]` attribute automatically generates constructor code
(like `#[derive(Inject)]`) and registers the service via `inventory`
(on Linux, macOS, and Windows).
Supports both named-field structs and unit structs (zero fields):

```rust
// Unit struct — zero fields, works out of the box
#[rust_dix::inject]
struct RoleAuthorizer;

impl Default for RoleAuthorizer { fn default() -> Self { Self } }
```

**Two placement sites, one consistent syntax:**

| Placement | What it registers | Reuses constructor? |
|-----------|-------------------|---------------------|
| `#[inject]` on struct | Concrete type `T` | Generates its own |
| `#[inject]` on `impl Trait for T` | `dyn Trait` | Reuses the generated constructor (internal `__rdi_construct_T`, `#[doc(hidden)]`) |
| `#[derive(Inject)]` on struct | *(nothing — constructor only)* | — |

**Choose one site, don't double-register:** `#[inject]` generally goes on
either the struct *or* the impl block, not both. Putting `#[inject]` on both
registers the same struct twice (concrete type + `dyn Trait`), which is rarely
intended — consumers can then bypass the trait via `get::<ConcreteType>()`.
For trait-oriented services (handlers, services, repositories), prefer
`#[derive(Inject)]` on the struct (constructor only, no registration) plus
`#[inject]` on the impl block. This replaces the old `as = dyn Trait` /
`as = [dyn A, dyn B]` syntax.

#### Resolution (`ServiceProvider` / `Scope`)

| Method | Returns | Behavior |
|--------|---------|----------|
| `.get::<T>()` | `Result<Arc<T>, RdiError>` | Returns Err if not registered; **last registration wins** |
| `.get_optional::<T>()` | `Option<Arc<T>>` | Returns `None` if not registered |
| `.get_keyed::<T>(key)` | `Result<Arc<T>, RdiError>` | `key: impl Into<ServiceKey>`; exact key wins over an `AnyKey` fallback; `any_key()` is not a valid single-service key |
| `.try_get_keyed::<T>(key)` | `Option<Arc<T>>` | Returns `None` if key not found/unresolvable |
| `.get_all::<T>()` | `Vec<Arc<T>>` | All **non-keyed** registrations, in registration order (keyed excluded) |
| `.get_all_keyed::<T>()` | `Vec<Arc<T>>` | All **explicitly keyed** registrations of `T`, in registration order (`AnyKey` fallbacks excluded) |
| `.get_named::<T>(name)` | `Option<Arc<T>>` | Named resolution (cross-DLL), returns `None` if missing |
| `.get_named_any::<T>(name)` | `Option<Arc<T>>` | Named, returns `None` if missing |
| `.get_owned::<T>()` | `Result<T, RdiError>` | Owned instance; Singleton returns Err; **Scoped is allowed from the root** (caller-owned, never cached — its Scoped dependencies still obey scope validation) |
| `.try_get_owned::<T>()` | `Option<T>` | `None` if unregistered or Singleton |
| `.get_keyed_owned::<T>(key)` | `Result<T, RdiError>` | Owned keyed instance; Singleton returns Err |
| `.is_registered::<T>()` | `bool` | Whether `T` has a non-keyed registration (does not instantiate) |
| `.is_registered_keyed::<T>(key)` | `bool` | Whether `T` has an exact keyed registration or an `AnyKey` fallback (`any_key()` → `false`) |
| `.scope()` | `Scope` | New scope for scoped services |
| `.async_context()` | `Option<AsyncFactoryContext>` | Capture the current async chain for detached tasks (cross-task cycle detection) |
| `.dispose()` | `()` | Idempotent; releases instances in reverse creation order, later resolution returns `Err(ProviderDisposed)` (0.8.0+) |

> **0.9.x semantics**: single-service resolution is MEDI-style "last registration
> wins"; `get_all` is unkeyed-only (`get_all_keyed` lists explicit keyed registrations
> only); keys may be typed; and scope validation is **strict by default** — resolving a
> Scoped service from the root provider returns `Err(RdiError::ScopedFromRootProvider)`
> (the exception is `get_owned` / `try_get_owned`: the owned instance is caller-owned and
> not cached, while its Scoped dependencies still obey validation).
> Build with `build_with(ServiceProviderOptions::lenient())` to keep the 0.7.x
> root-scope behavior. See the [MEDI differences page](../../docs/rust-dix/01-introduction/medi-differences.md).

> **Enumeration and async**: `get_all` / `get_all_keyed` / `#[inject] Vec<Arc<T>>` include
> **already-initialized** async singletons. Synchronously enumerating an uninitialized
> async singleton/transient yields `RdiError::AsyncServiceRequiresGetAsync` (the
> infallible `get_all` / `get_all_keyed` panic with a typed `RdiError` payload); an async
> **scoped** service enumerated at the root is omitted by strict scope validation (it
> panics only inside a `Scope`), while `get_all_any_result` / `Vec<Arc<T>>` injection
> return `Err` for any uninitialized async service. Resolve async scoped/transient
> services item by item with `get_async`.

#### Diagnostics

| Error | When |
|-------|------|
| `RdiError::SyncSingletonDependsOnAsyncService { singleton, async_service }` | a sync singleton depends on an async service (build time); make the dependent async or resolve lazily |
| `RdiError::AsyncServiceRequiresGetAsync { service }` | synchronous `get` of an async service (`get_optional` returns `None`), or synchronous enumeration of an uninitialized async service; use `get_async` |
| `RdiError::AsyncServiceRequiresBuildAsync { service }` | `build()` used on a collection containing `async_*` registrations; use `build_async()` |
| `RdiError::is_circular()` | **heuristic** helper: `true` for a direct `CircularDependency` or a `FactoryPanic` payload containing `CircularDependency` (custom payloads may false-positive; prefer matching the typed variant) |

`T` can be a concrete type or `dyn Trait`:

```rust
let svc: Arc<MyService> = provider.get().unwrap();
let plugin: Arc<dyn IPlugin> = provider.get().unwrap();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

#### Container injection (`ServiceProvider` / `ScopeFactory`)

The container is itself resolvable as a built-in service (no registration
required) through every resolver path — `provider.get()`, `#[inject]` fields and
build-time validation alike:

| Field | Notes |
|-------|-------|
| `#[inject(provider)] provider: Arc<ServiceProvider>` | On-demand resolution; also available in eagerly built singletons. **Warning:** a singleton holding this creates a strong `container → singleton → container` cycle; prefer `ScopeFactory` |
| `#[inject] provider: Arc<ServiceProvider>` | Same built-in, plain form (MEDI `IServiceProvider` parity); same strong-reference caveat |
| `#[inject] scope_factory: Arc<dyn ScopeFactory>` | Recommended for singleton workers: one scope per task. Weak-backed singleton, so holding it does not create an `Arc` cycle |

Explicit `singleton::<dyn ScopeFactory>(...)` registrations still take precedence
over the built-in.

> Resolving services *through an injected provider* inside a singleton
> constructor is rejected at build time with a typed `FactoryPanic`: it would
> bypass cycle/captive validation and make the result order-dependent. Store the
> handle and resolve lazily in methods.

---

### Owned service resolution (`&mut self` without interior mutability)

`get::<T>()` returns `Arc<T>`, giving you only `&T` — you cannot call
`&mut self` methods. For services that need mutable access (e.g. a
`DbContext` with `save_changes(&mut self)`), use `get_owned::<T>()` to
obtain an owned `T` instead of resorting to `RwLock` + `unsafe`:

```rust
struct DbContext { count: u32 }
impl DbContext {
    fn add(&mut self) { self.count += 1; }
    fn total(&self) -> u32 { self.count }
}

let provider = ServiceCollection::new()
    .transient(|_| Arc::new(DbContext { count: 0 }))
    .build()
    .unwrap();

let mut ctx: DbContext = provider.get_owned().unwrap();
ctx.add();                 // ✓ &mut self — no RwLock, no unsafe
assert_eq!(ctx.total(), 1);
```

| Lifetime | `get_owned` behavior |
|----------|----------------------|
| **Transient** | New instance each call ✓ |
| **Scoped** | Bypasses cache, new instance each call ✓ (callable from the root too: the owned instance is not subject to root scope validation, but its Scoped dependencies are) |
| **Singleton** | `Err(OwnedNotSupported)` (shared instance cannot be owned); use `try_get_owned` for `None` |

**With `#[derive(Inject)]`**: mark bare `T` fields with `#[inject(owned)]`
and `Arc<T>` fields with `#[inject]`. `Option<T>` → `try_get_owned`,
`Option<Arc<T>>` → `try_get`:

```rust
#[derive(Inject)]
struct Handler {
    #[inject(owned)]
    ctx: DbContext,           // owned  → get_owned
    #[inject]
    logger: Arc<Logger>,      // shared → get
}
// ...
let mut h: Handler = provider.get_owned().unwrap();
h.ctx.add();                 // ✓ owned field is mutable
```

**Limitations**:
- Singleton cannot be owned — `get_owned` returns `Err`, `try_get_owned` returns `None`.
- Scoped `get_owned` bypasses the scope cache (each call is fresh). This is
  intentional: owned `&mut self` is incompatible with scope-shared semantics.
  Owned resolution is the exception to strict scope validation: `get_owned` /
  `try_get_owned` may resolve a Scoped service from the **root** (the instance is
  caller-owned, never cached or tracked) — but Scoped dependencies resolved inside
  its constructor still return `Err(ScopedFromRootProvider)` at the root.
- Trait bound remains `T: Send + Sync + 'static` (reuses the existing factory,
  no separate owned-factory path).

---

### Async support (`build_async`)

For services that need async initialization (DB connections, network I/O, config
loading), use `build_async()` and the `async_*` registration methods.

#### Registration

| Method | Lifetime | Use when |
|--------|----------|----------|
| `.async_singleton(f)` | Singleton | Async init, shared globally (DB pool, Redis) |
| `.async_scoped(f)` | Scoped | Async init, per scope (request-scoped resources) |
| `.async_transient(f)` | Transient | Async init, fresh each time |
| `.async_keyed_singleton(k, f)` | Singleton keyed | Async init, keyed instance (`k: impl Into<ServiceKey>`) |
| `.async_keyed_scoped(k, f)` | Scoped keyed | Async init, per-scope keyed instance |
| `.async_keyed_transient(k, f)` | Transient keyed | Async init, fresh keyed instance |

The async factory receives `Arc<ServiceProvider>` and returns a pinned future:

```rust
let provider = ServiceCollection::new()
    .async_singleton(|p: Arc<ServiceProvider>| Box::pin(async move {
        let config: Arc<AppConfig> = p.get().unwrap();
        Arc::new(DbPool::connect(&config.conn_str).await)
    }))
    .async_keyed_singleton::<dyn IPaymentGateway>("wechat", |_| Box::pin(async {
        Arc::new(WechatPay::init().await) as Arc<dyn IPaymentGateway>
    }))
    .build_async()
    .await
    .unwrap();
```

#### Resolution

Use `get_async()` / `get_keyed_async()` when resolving async-registered services
(both accept string or typed `ServiceKey` keys and return `impl Future`; `.await` usage is
unchanged):

```rust
let pool: Arc<DbPool> = provider.get_async().await.unwrap();
let gateway: Arc<dyn IPaymentGateway> = provider.get_keyed_async("wechat").await.unwrap();
```

For sync-registered services you can use either `get()` or `get_async()`: `get_async()`
falls back to the synchronous resolution path for services registered with the sync
`scoped` / `transient` builders. Async registration (`async_*`) is only needed
for services that require **asynchronous initialization**. Synchronous `get` of an
async service reports `RdiError::AsyncServiceRequiresGetAsync`; `AnyKey` fallbacks are
sync-only and are also resolved through the synchronous path.

Async singleton initialization is **dependency-first and registration-order independent**:
a factory may `await` any async singleton, and `build_async()` recursively
initializes dependencies first (deterministic dependency-first topological order, eager and
fail-fast). A sync singleton depending on an async service is rejected at build time with
`RdiError::SyncSingletonDependsOnAsyncService` — make the dependent async or resolve
lazily.

#### Cross-task cycle detection (`AsyncFactoryContext`)

Async cycle detection is chain-based: each resolution future captures the current chain
when constructed, so self/cross recursion returns `RdiError::CircularDependency`. For
detached tasks, capture `sp.async_context()` inside the factory and resolve through the
cloned context; without the context a fully detached task may still dead-lock:

```rust
let provider = ServiceCollection::new()
    .async_singleton(|sp| Box::pin(async move {
        let ctx = sp.async_context().expect("built provider");
        let dep = tokio::spawn({
            let ctx = ctx.clone();
            async move { ctx.get_async::<DbPool>().await }
        })
        .await
        .expect("task")
        .expect("DbPool must resolve");
        Arc::new(Service { dep })
    }))
    .build_async()
    .await
    .unwrap();
```

`AsyncFactoryContext` exposes `get_async`, `get_keyed_async` and `provider()`, is `Clone`,
and can also be obtained from a `Scope` (`scope.async_context()`).

#### Mixing sync and async

You can mix sync and async registrations in the same collection. Sync singletons
are validated and initialized alongside async ones during `build_async()`:

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool::new()) }))
    .singleton(|_| Arc::new(AppConfig::new()))       // sync OK
    // `UserRepository` is `#[derive(Inject)]`; its constructor resolves DbPool.
    .scoped(|r| __rdi_construct_UserRepository(r))   // sync OK
    .build_async()
    .await
    .unwrap();
```

**Key rule**: Once you use any `async_*` registration, call `build_async()`
instead of `build()`. Mixing is supported — sync singletons are validated and
initialized alongside async ones.

#### Per-request scope with async

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool::new()) }))
    .scoped(|r| __rdi_construct_UserRepository(r))
    .build_async().await.unwrap();

// Each request gets its own scope
fn handle_request(provider: &Arc<ServiceProvider>) {
    let scope = provider.scope();
    let repo: Arc<UserRepository> = scope.get().unwrap();
    // ... process request ...
    // scope dropped → scoped instances released (reverse creation order)
}
```

---

### Flexible Application Patterns

#### 🔹 Three-layered architecture (Controller → Service → Repository)

```rust
// `UserRepo` / `UserService` / `UserController` are `#[derive(Inject)]`; the
// generated constructors wire the dependencies for each layer.
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(DbPool::new(&config)))
    .transient(|r| __rdi_construct_UserRepo(r))
    .transient(|r| __rdi_construct_UserService(r))
    .transient(|r| __rdi_construct_UserController(r))
    .build()
    .unwrap();
```

#### 🔹 Strategy pattern with keyed services

```rust
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn PaymentGateway>("credit", |_| Arc::new(CreditCardGateway))
    .keyed_singleton::<dyn PaymentGateway>("alipay", |_| Arc::new(AlipayGateway))
    .keyed_singleton::<dyn PaymentGateway>("wechat", |_| Arc::new(WechatGateway))
    .build()
    .unwrap();

fn checkout(provider: &ServiceProvider, method: &str) {
    let gateway: Arc<dyn PaymentGateway> = provider.get_keyed(method).unwrap();
    gateway.charge(100);
}
```

#### 🔹 Scoped per-request (web server)

```rust
fn handle_request(container: &ServiceProvider) {
    let scope = container.scope();
    let ctx: Arc<RequestContext> = scope.get().unwrap();
    let svc: Arc<MyService> = scope.get().unwrap();
    // ctx and MyService share the same scope — scoped services are cached
    // Drops when scope goes out of scope
}
```

#### 🔹 Plugin isolation with ServiceProviderWrapper

```rust
let host_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(HostService::new()))
    .build().unwrap();

let plugin_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(PluginService::new()))
    .build().unwrap();

let wrapper = ServiceProviderWrapper::new(plugin_provider, host_provider);
// PluginService resolved from plugin container
// HostService falls back to host container
```

#### 🔹 Cross-DLL plugin with named services

```rust
// Host process
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(EventBus::new()))
    .build().unwrap();
provider.register_named("event_bus", provider.get::<EventBus>().unwrap());

// Plugin loaded from cdylib (separate compilation unit)
let bus = host.get_named::<EventBus>("event_bus");
```

#### 🔹 External system integration via IProvider

```rust
let provider: Arc<dyn IProvider> = provider as Arc<dyn IProvider>;
// Pass to third-party code or FFI boundary
```

---

### Architecture

```
┌──────────────────────────────────────────────┐
│               ServiceCollection               │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │singleton │ │  scoped  │ │  transient   │ │
│  │.keyed_singleton│ │.keyed_*()│ │ .instance()  │ │
│  └──────────┘ └──────────┘ └──────────────┘ │
│                   │ .build()                  │
│                   ▼                           │
│              ServiceProvider                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │  .get()  │ │.get_keyed│ │  .scope()   │ │
│  │ .get_all │ │.get_named│ │             │ │
│  └──────────┘ └──────────┘ └──────┬───────┘ │
│                                    │         │
└────────────────────────────────────┼─────────┘
                                     │
                                     ▼
                               ┌──────────┐
                               │   Scope  │
                               │ .get()   │
                               │ scoped   │
                               │ cached   │
                               └──────────┘
```

---

### Relationship with rust-dix-macros

`rust-dix` works with or without [`rust-dix-macros`](../macros/). The macros
provide three compile-time conveniences:

- **`#[rust_dix::inject(...)]`** (recommended) — Attribute macro that combines
  constructor generation + auto-registration. One annotation on a struct (or
  trait impl) is all you need — `ServiceCollection::from_injected()` collects
  everything.
- **`#[derive(Inject)]`** — Generate the factory function automatically from
  struct fields. Used internally by `#[rust_dix::inject]`.
- **`#[rust_dix::module]`** — Collect `rust_dix::register!()` declarations at compile
  time and generate a complete provider builder. Useful for external types,
  conditional compilation, and centralized management.

See [`macros/README.md`](../macros/README.md) for details.

---

### License

MIT.
