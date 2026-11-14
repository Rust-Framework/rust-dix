<p align="center">
  <img src="assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix · Dependency Injection for Rust</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix?style=flat-square"></a>
  <a href="https://rust-framework.github.io/rust-dix/"><img alt="Documentation" src="https://img.shields.io/badge/docs-mdBook-blue.svg?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <strong>English</strong> · <a href="README.zh-CN.md">简体中文</a>
</p>

**rust-dix** is a dependency injection framework for Rust, inspired by
Microsoft.Extensions.DependencyInjection (MEDI). It provides a service container
with constructor injection, scoped lifetimes, async initialization, and
compile-time validation — all with zero `unsafe` code.

**[Online documentation](https://rust-framework.github.io/rust-dix/)** — the full
developer handbook (mdBook, Chinese), built from `docs/rust-dix/`.

## Crate overview

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`rust-dix`](./crates/core/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`rust-dix-macros`](./crates/macros/) | Procedural macros — `#[derive(Inject)]`, `#[rust_dix::inject]`, `#[rust_dix::module]` | Anyone using `rust-dix` who wants compile-time code generation instead of manual wiring |

## Quick Start

```toml
[dependencies]
rust-dix = "1.0"
```

### Sync applications

```rust
use rust_dix::*;
use std::sync::Arc;

#[rust_dix::inject(singleton)]
struct Config;

#[rust_dix::inject(transient)]
struct Worker { #[inject] config: Arc<Config> }

let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let worker: Arc<Worker> = provider.get().unwrap();
```

### Async applications (e.g., axum / actix-web)

```rust
use rust_dix::*;
use std::sync::Arc;

struct DbPool { conn: String }

let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async {
        // async connect, config load, etc.
        Arc::new(DbPool { conn: "pg://localhost".into() })
    }))
    .build_async()
    .await
    .unwrap();

let pool: Arc<DbPool> = provider.get_async().await.unwrap();
```

## Features

| Feature | Description |
|---------|-------------|
| `#[rust_dix::inject]` | Attribute-macro-based auto-registration on structs or trait impls |
| `#[derive(Inject)]` | Derive-macro for constructor injection on structs |
| `#[rust_dix::module]` | Compile-time module scanning via `register!(...)` |
| **Three lifetimes** | Singleton, Scoped, Transient |
| **Keyed services** | Multiple implementations of the same trait by key — string **or typed** (`ServiceKey`) keys |
| **`AnyKey` fallback** | `keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`: MEDI `KeyedService.AnyKey` fallback, cached per (registration/type, requested key) |
| **Async support** | `build_async()` + `async_*` registration for services needing async init |
| **Async context** | `provider.async_context()` propagates the resolution chain into spawned tasks → cross-task cycle detection |
| **Precise diagnostics** | `SyncSingletonDependsOnAsyncService`, `AsyncServiceRequiresGetAsync`, `RdiError::is_circular()` |
| **Build-time validation** | Circular dependency detection, captive dependency rejection, constructor-time resolution rejection |
| **Container injection** | `#[inject(provider)]`, `Arc<dyn ScopeFactory>`, `Arc<ServiceProvider>` — works in eager singletons and lazily resolved services |
| **Cross-DLL support** | Named service registry for cdylib plugin systems |
| **Layered containers** | Child-first resolution via `ServiceProviderWrapper` |
| **Owned resolution** | `get_owned::<T>()` returns owned `T` for `&mut self` — no interior mutability |
| **Deterministic disposal** | `Scope::dispose()` / `ServiceProvider::dispose()` release instances in reverse creation order |
| **Memory-safe** | Both crates are `#![forbid(unsafe_code)]`; MSRV **1.95**; CI matrix + RustSec `cargo audit`; see [SECURITY.md](SECURITY.md) |

## Container semantics (0.10.x)

rust-dix 0.10.x aligns single-service resolution with MEDI while keeping build-time
safety. The five rules that most often surprise new users (full semantics:
[docs/rust-dix/09-reference/semantics.md](docs/rust-dix/09-reference/semantics.md)):

| Rule | Behavior |
|------|----------|
| **Last registration wins** | `get` / `get_keyed*` / `get_owned` / `get_async` return the last registration (same key for keyed); `try_add*` = "only add if absent". |
| **Strict scope validation (default)** | Root resolution of Scoped returns `ScopedFromRootProvider`; resolve inside `provider.scope()`. Owned `get_owned`/keyed owned is the caller-owned exception (not cached/tracked; transitive Scoped deps still validated). |
| **Typed keys & `AnyKey`** | Keyed APIs take `impl Into<ServiceKey>`; exact key wins over `keyed_any_*`; AnyKey caches per **(registration/type, requested key)**; `get_all_keyed` excludes fallbacks; `any_key()` is invalid for single resolution. |
| **Async: resolve & enumerate** | Async services use `get_async` (sync `get` → `AsyncServiceRequiresGetAsync`, `get_optional` → `None`); cross-task cycles need `AsyncFactoryContext`; enumeration includes initialized async singletons only (uninitialized → typed panic/`Err`). |
| **Diagnostics & baseline** | `is_circular()` is heuristic; MSRV 1.95, `#![forbid(unsafe_code)]`, CI matrix + `cargo audit`, see [SECURITY.md](SECURITY.md). |

New in 0.10: macro key **expressions** (`#[inject(key = <expr>)]` /
`#[inject(owned, key = <expr>)]`, `register!(keyed <expr>: ...)`) accepting string literals,
typed `ServiceKey::new(...)`, or downstream key types via `impl From<LocalKey> for ServiceKey`;
`IServiceResolver` typed-keyed methods with string-only fallback; `ServiceKey::as_str()`;
reference chapter incl. [extension points](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md).
**async AnyKey is still not provided** (workaround in extension points). MEDI comparison:
[medi-differences](docs/rust-dix/01-introduction/medi-differences.md).

## When should you use rust-dix?

- **You are building a medium-to-large Rust application** where manually
  constructing dependencies becomes tedious and error-prone.
- **You need testability** — swap real implementations with mocks by changing
  one registration line.
- **You are designing a plugin system** — named services and layered containers
  let host and plugin each have their own DI container.
- **You come from the .NET ecosystem** and want the same
  `IServiceCollection` / `IServiceProvider` patterns in Rust.

## Interface-oriented programming

The core principle of rust-dix: **depend on traits, not concrete types**. This
enables mock substitution in tests and swapping implementations without changing
consumers.

### Why interfaces matter

| Without interfaces | With interfaces |
|-------------------|-----------------|
| `Arc<WechatPay>` — locked to one impl | `Arc<dyn IPaymentGateway>` — any impl |
| Tests can't swap payment gateway | Mock with `MockPaymentGateway` |
| Adding Alipay requires changing every consumer | Just register the new implementation |

### Polymorphism: multiple implementations of the same trait

Register multiple services under the same trait, then resolve them all at once.

**Registration side:**

```rust
use rust_dix::*;
use std::sync::Arc;

pub trait INotificationChannel: Send + Sync {
    fn send(&self, msg: &str);
}

// Register two unkeyed implementations of the same trait, plus one keyed
let provider = ServiceCollection::new()
    .singleton::<dyn INotificationChannel>(|_| Arc::new(EmailChannel))
    .singleton::<dyn INotificationChannel>(|_| Arc::new(SmsChannel))
    .keyed_singleton::<dyn INotificationChannel>("push", |_| Arc::new(PushChannel))
    .build()
    .unwrap();
```

**Consumption side:**

```rust
// Get the default (unkeyed) implementation — LAST registration wins
let default: Arc<dyn INotificationChannel> = provider.get().unwrap(); // SmsChannel

// Get a specific named implementation
let push: Arc<dyn INotificationChannel> = provider.get_keyed("push").unwrap();

// Get ALL unkeyed implementations at once (keyed implementations are NOT included)
let all: Vec<Arc<dyn INotificationChannel>> = provider.get_all();
assert_eq!(all.len(), 2);

// Keyed implementations: resolve all of them explicitly
let keyed: Vec<Arc<dyn INotificationChannel>> = provider.get_all_keyed();

// Broadcast a message to every channel (unkeyed + keyed)
for channel in all.iter().chain(keyed.iter()) {
    channel.send("System alert: disk usage > 90%");
}
```

### Keyed services: runtime strategy routing

Keyed services let you select an implementation at runtime by key (a string or a
typed `ServiceKey` value) — the foundation of the Strategy pattern.

**Registration side:**

```rust
pub trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> Result<String, String>;
}

let provider = ServiceCollection::new()
    // Default (unkeyed) — used as fallback
    .singleton::<dyn IPaymentGateway>(|_| Arc::new(WechatPay))
    // Named alternatives
    .keyed_singleton::<dyn IPaymentGateway>("alipay", |_| Arc::new(Alipay))
    .keyed_transient::<dyn IPaymentGateway>("credit_card", |_| Arc::new(CreditCardPay))
    .build()
    .unwrap();
```

**Consumption side:**

```rust
// ── Pattern A: Static dispatch (compile-time known key) ──
let gateway: Arc<dyn IPaymentGateway> = provider.get_keyed("alipay").unwrap();
gateway.charge(100)?;

// ── Pattern B: Dynamic routing (runtime key from request) ──
fn process_payment(provider: &ServiceProvider, method: &str, amount: u64) {
    let gateway: Arc<dyn IPaymentGateway> = provider
        .try_get_keyed(method)                          // returns Option
        .unwrap_or_else(|| provider.get().unwrap());    // fallback to default
    gateway.charge(amount).unwrap();
}

// ── Pattern C: Inject provider for lazy routing ──
#[derive(Inject)]
struct OrderService {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,  // resolve strategy on demand
}

impl OrderService {
    fn checkout(&self, method: &str, amount: u64) {
        let gateway: Arc<dyn IPaymentGateway> = self.provider
            .try_get_keyed(method)
            .unwrap_or_else(|| self.provider.get().unwrap());
        gateway.charge(amount).unwrap();
    }
}
```

#### Typed keys (`ServiceKey`)

Keyed APIs accept `impl Into<ServiceKey>`, so enum/integer keys work in addition to
strings (string calls stay source-compatible). Key equality includes the value's type.
**String-key footgun**: `ServiceKey::new("x")` is a `&str`-typed key and is *not* equal
to the `String` key stored by `keyed_singleton("x", …)` — use `"x".into()` /
`ServiceKey::from("x")` for strings.

```rust
use rust_dix::*;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Hash)]
enum DbKey { Read, Write }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IDbConnection>(ServiceKey::new(DbKey::Read), |_| Arc::new(ReadPool))
    .keyed_singleton::<dyn IDbConnection>(ServiceKey::new(DbKey::Write), |_| Arc::new(WritePool))
    .build()
    .unwrap();

let read: Arc<dyn IDbConnection> =
    provider.get_keyed(ServiceKey::new(DbKey::Read)).unwrap();

// The value's type is part of the key: 1u32 is not the string "1".
assert!(!provider.is_registered_keyed::<dyn IDbConnection>(ServiceKey::new(1u32)));
assert!(provider.get_keyed::<dyn IDbConnection>(ServiceKey::from("1")).is_err());
```

##### Macro key expressions (0.10)

`#[inject(key = <expr>)]` / `#[inject(owned, key = <expr>)]` accept any
`impl Into<ServiceKey>`. Downstream key types can support them via
`impl From<LocalKey> for ServiceKey` — the official extension path (do **not** copy the
injection macro):

```rust
impl From<DbKey> for ServiceKey {
    fn from(k: DbKey) -> Self { ServiceKey::new(k) }
}

#[derive(Inject)]
struct ReadService {
    #[inject(key = DbKey::Read)]                 // typed key expression
    db: Arc<dyn IDbConnection>,
    #[inject(key = "audit")]                     // string literal still works (String key)
    audit: Arc<dyn IAuditLog>,
}
```

`register!(keyed <expr>: <lifetime>: <Type>)` accepts expressions too; compile-time duplicate
detection (`rdi-E004`) covers **string literals only** — dynamic keys follow runtime
last-registration-wins. String keys must use `ServiceKey::from("x")` / `"x".into()`
(`ServiceKey::new("x")` is a `&str` typed key, not equal). Async AnyKey is **not** provided in
0.10 — see [extension points](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md).

#### `AnyKey` fallback (MEDI `KeyedService.AnyKey`)

`keyed_any_*` registers a fallback used when no **exact** key is registered. The factory
receives the requested key, and singleton/scoped fallback instances are cached
**per (registration/type, requested key)** — same registration + key reuses one instance;
different keys or different `AnyKey` registrations stay independent (scoped: per scope per
(registration, key))

```rust
let provider = ServiceCollection::new()
    // exact key wins
    .keyed_singleton::<dyn IDbConnection>("admin", |_| Arc::new(AdminPool))
    // fallback for every other key; `key` is the requested ServiceKey
    .keyed_any_singleton::<dyn IDbConnection>(|_, key| {
        // `key: &ServiceKey` — use it to select/record the tenant.
        Arc::new(Pool::connect(&format!("{key:?}")))
    })
    .build()
    .unwrap();

let admin: Arc<dyn IDbConnection> = provider.get_keyed("admin").unwrap();   // exact
let t1: Arc<dyn IDbConnection> = provider.get_keyed("tenant-1").unwrap();  // fallback, cached per registration+key
let t1_again = provider.get_keyed::<dyn IDbConnection>("tenant-1").unwrap();
assert!(Arc::ptr_eq(&t1, &t1_again));

// AnyKey fallbacks are NOT part of `get_all_keyed`, and the sentinel is not a valid
// single-service key:
assert!(provider.get_keyed::<dyn IDbConnection>(ServiceKey::any_key()).is_err());
let explicit = provider.get_all_keyed::<dyn IDbConnection>(); // only "admin"
```

> `keyed_any_*` registrations are sync-only (async resolution falls back to the sync
> path), are not eagerly initialized/validated at build (they need a requested key), and
> a `keyed_any_scoped` fallback used by a sync singleton is still rejected as a captive
> dependency. A singleton fallback consumed during build-time validation is prefilled into
> the `(registration, requested key)` cache (runtime with the same registration + key
> reuses the **same instance**; multiple consumers share one construction); an
> `AnyKey` scoped/transient fallback supports `get_keyed_owned` at the strict root (owned
> exception); a factory may nested-resolve a **different** requested key (only the same
> registration + same key is a cycle); and the `&ServiceKey` argument has no typed
> downcast accessor (use `Debug`/`Display`/`Eq`/`Hash`; erasure is intentional).

#### Cross-task async resolution (`AsyncFactoryContext`)

Inside an async factory, capture `sp.async_context()` and clone it into spawned tasks so
dependency cycles **across tasks** return `CircularDependency` instead of dead-locking:

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

Container injection also works **during `build()`**: eagerly built singletons
may hold the provider or the built-in scope factory. The weak-backed, cached
`ScopeFactory` is the recommended choice for singleton workers — it creates an
independent scope per task without keeping the container alive through an `Arc`
cycle. Prefer it over `#[inject(provider)]` in singletons (holding
`Arc<ServiceProvider>` creates a strong cycle), and resolve lazily in methods:
resolving through the injected provider inside a build-time constructor is
rejected.

```rust
#[derive(Inject)]
struct OutboxWorker {
    #[inject]
    scope_factory: Arc<dyn ScopeFactory>, // no manual registration needed
}

impl OutboxWorker {
    fn tick(&self) {
        let scope = self.scope_factory.create_scope();
        let db: Arc<DbContext> = scope.get().unwrap();
        // ... deliver one batch in this scope ...
    } // dropping the scope disposes scoped services
}
```

### Scoped services: per-request isolation

Scoped services are created once per scope and cached within it. Scoped services
must be resolved from a child scope created with `provider.scope()` — the root
provider does not resolve Scoped services by default (strict scope validation,
`Err(RdiError::ScopedFromRootProvider)`). The exception is `get_owned` /
`try_get_owned`: the owned instance is caller-owned and not cached, so it may be
taken from the root, while its Scoped dependencies still obey validation.

**Registration side:**

```rust
pub trait IUnitOfWork: Send + Sync {
    fn commit(&self) -> Result<(), String>;
    fn rollback(&self);
}

#[derive(Inject)]
struct UnitOfWork {
    #[inject]
    db: Arc<dyn IDbPool>,
}

#[rust_dix::inject(scoped)]           // register `dyn IUnitOfWork` as Scoped
impl IUnitOfWork for UnitOfWork { /* ... */ }

#[derive(Inject)]
struct OrderService {
    #[inject]
    uow: Arc<dyn IUnitOfWork>,
}

#[rust_dix::inject(transient)]        // register `dyn IOrderService` as Transient
impl IOrderService for OrderService { /* ... */ }

let provider = ServiceCollection::from_injected()
    .singleton::<dyn IDbPool>(|_| Arc::new(DbPool::new()))    // global
    .build()
    .unwrap();
```

**Consumption side:**

```rust
// ── Root provider: Scoped services are not resolvable with the default strict options ──
let err = provider.get::<dyn IUnitOfWork>(); // Err(RdiError::ScopedFromRootProvider)

// ── Child scope: each scope gets its own UoW instance ──
let scope_a = provider.scope();
let scope_b = provider.scope();

let uow_a: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
let uow_b: Arc<dyn IUnitOfWork> = scope_b.get().unwrap();
// uow_a != uow_b (different scopes, different instances)

// Within the same child scope, Scoped is cached:
let uow_a2: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
// uow_a == uow_a2 (same scope, cached)

// ── Transient from scope: resolves Scoped deps within that scope ──
let order_svc: Arc<dyn IOrderService> = scope_a.get().unwrap();
// OrderService's IUnitOfWork is scope_a's UoW

// Dropping (or explicitly disposing) the scope releases its scoped instances
// in reverse creation order.
```

Need the old 0.7 root-scope behavior? Build with
`ServiceProviderOptions::lenient()` — see
[the MEDI differences page](docs/rust-dix/01-introduction/medi-differences.md#2-scope-校验validatescopes).

**Key rule**: Singleton must not depend on Scoped (enforced at build time).
Scoped can depend on Singleton. Transient can depend on anything.

For detailed patterns (contracts layer, layered architecture, plugin systems),
see the [business scenario tutorials](docs/rust-dix/08-best-practices/business-scenarios.md).

## Best practices

### Lifetime selection

| Lifetime | When to use | Example |
|----------|------------|---------|
| **Singleton** | Stateless services, shared configuration, connection pools | `AppConfig`, `DbPool`, `Logger` |
| **Scoped** | Per-request state, unit-of-work objects | `DbContext`, `UserRepository`, `UnitOfWork` |
| **Transient** | Lightweight, stateless workers created fresh each time | `OrderService`, `Validator`, `EmailSender` |

### Sync vs async registration

```
Need async init (DB, network, I/O)?
├── Yes → use build_async() + async_* registration
│   ├── Shared across app → async_singleton
│   ├── Per-request scope → async_scoped
│   └── Fresh each call → async_transient
└── No → use build() + sync registration
    ├── Shared across app → singleton
    ├── Per-request scope → scoped
    └── Fresh each call → transient
```

**Important**: Once you use any `async_*` registration, you must call
`build_async()` instead of `build()`. Mixing them in the same collection is
supported — sync singletons are validated and initialized alongside async ones.

### Dependency chains across sync/async boundaries

Sync services can depend on async singletons because async singletons are
cached during `build_async()`. Use `get_any()` in sync factories:

```rust
// Sync scoped service depends on async singleton DbPool
.scoped(|r| {
    let db = r.get_any(std::any::type_name::<DbPool>())
        .and_then(|a| a.downcast::<Arc<DbPool>>().ok().map(|d| Arc::clone(&*d)))
        .unwrap();
    Arc::new(UserRepository { db })
})
```

Async services can depend on sync singletons via the `Arc<ServiceProvider>`
parameter:

```rust
.async_singleton(|p: Arc<ServiceProvider>| {
    Box::pin(async move {
        let config: Arc<AppConfig> = p.get().unwrap();
        Arc::new(DbPool { conn_str: config.conn_str.clone() })
    })
})
```

### Scope lifecycle (per-request pattern)

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool { ... }) }))
    .scoped(|r| { /* UserRepository depends on DbPool */ Arc::new(...) })
    .build_async().await.unwrap();

// Each request gets its own scope
fn handle_request(provider: &Arc<ServiceProvider>) {
    let scope = provider.scope();           // creates isolated scope
    let repo = scope.get::<UserRepository>().unwrap();
    // ... process request ...
    // scope is dropped here → scoped instances released
}
```

### Error handling

- Prefer `try_get::<T>()` / `get_optional::<T>()` (returns `Option`) for
  optional dependencies
- Use `get::<T>()` (returns `Result`) when the dependency is required —
  absence is a recoverable error, not a panic

### Architecture patterns

For common design patterns (Strategy, Factory, Decorator, Observer, Chain of
Responsibility, Pipeline, Plugin, Composite, Proxy, Feature Toggle), see the
[patterns test suite](crates/core/tests/patterns.rs).

## Real-world example: Web application

```rust
use rust_dix::*;
use std::sync::Arc;

// ── Infrastructure (async) ──
struct DbPool { conn_str: String }
struct RedisCache { url: String }

// ── Business services ──
struct AppConfig { max_retries: u32 }
struct UserRepository { db: Arc<DbPool>, cache: Arc<RedisCache>, config: Arc<AppConfig> }
struct OrderService { repo: Arc<UserRepository> }

trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> bool;
}

#[tokio::main]
async fn main() {
    let provider = ServiceCollection::new()
        // Async infrastructure — initialized at build time
        .async_singleton(|_| Box::pin(async {
            Arc::new(DbPool { conn_str: "pg://localhost".into() })
        }))
        .async_singleton(|_| Box::pin(async {
            Arc::new(RedisCache { url: "redis://localhost".into() })
        }))
        .async_keyed_singleton::<dyn IPaymentGateway>("wechat", |_| {
            Box::pin(async { Arc::new(WechatPay { ... }) as Arc<dyn IPaymentGateway> })
        })
        // Sync services
        .singleton(|_| Arc::new(AppConfig { max_retries: 3 }))
        .scoped(|r| {
            let db = /* resolve DbPool via get_any */;
            Arc::new(UserRepository { db, ... })
        })
        .transient(|r| {
            let repo = /* resolve UserRepository */;
            Arc::new(OrderService { repo })
        })
        .build_async()
        .await
        .unwrap();

    // Per-request scope
    let scope = provider.scope();
    let order_svc: Arc<OrderService> = scope.get().unwrap();
    // ... handle request ...
}
```

Full runnable example: [`async_integration.rs`](crates/core/tests/async_integration.rs)

## Further reading

- [crates/core — core library docs](./crates/core/README.md)
- [crates/macros — procedural macro docs](./crates/macros/README.md)
- [Differences from MEDI (scope validation, dispose, keyed, get_all)](docs/rust-dix/01-introduction/medi-differences.md) — read this if you come from .NET
- [Reference: current (0.10) semantics, RdiError catalog, production checklist](docs/rust-dix/09-reference/INDEX.md)
- [Extension points & boundaries (macro key `From`, custom resolver, async AnyKey)](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md)
- [Business scenario tutorials](docs/rust-dix/08-best-practices/business-scenarios.md) — real-world patterns
- [Development & build-artifact hygiene](docs/DEVELOPMENT.md) — why `target/` grows to tens of GB, the tuned Cargo profiles, and the `scripts/clean-target.ps1` pruner
- [Security policy](./SECURITY.md) — vulnerability reporting, supported versions, security boundaries
- [Changelog](./CHANGELOG.md) — release history
- [Integration tests](crates/core/tests/) — real-world usage patterns

## License

MIT.