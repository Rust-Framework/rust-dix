<p align="center">
  <img src="assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix · Dependency Injection for Rust</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <strong>English</strong> · <a href="README.zh-CN.md">简体中文</a>
</p>

**rust-dix** is a dependency injection framework for Rust, inspired by
Microsoft.Extensions.DependencyInjection (MEDI). It provides a service container
with constructor injection, scoped lifetimes, async initialization, and
compile-time validation — all with zero `unsafe` code.

## Crate overview

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`rust-dix`](./crates/core/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`rust-dix-macros`](./crates/macros/) | Procedural macros — `#[derive(Inject)]`, `#[rust_dix::inject]`, `#[rust_dix::module]` | Anyone using `rust-dix` who wants compile-time code generation instead of manual wiring |

## Quick Start

```toml
[dependencies]
rust-dix = "0.7"
```

### Sync applications

```rust
use rust_dix::*;
use std::sync::Arc;

#[rust_dix::inject(singleton)]
struct Config;

#[rust_dix::inject(transient)]
struct Worker { config: Arc<Config> }

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
| **Keyed services** | Multiple implementations of the same trait by string key |
| **Async support** | `build_async()` + `async_*` registration for services needing async init |
| **Build-time validation** | Circular dependency detection, captive dependency rejection |
| **Cross-DLL support** | Named service registry for cdylib plugin systems |
| **Layered containers** | Child-first resolution via `ServiceProviderWrapper` |
| **Owned resolution** | `get_owned::<T>()` returns owned `T` for `&mut self` — no interior mutability |
| **Scoped cleanup** | Cleanup closures + automatic `Drop` for scoped resource release |

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

// Register three implementations of the same trait
let provider = ServiceCollection::new()
    .singleton::<dyn INotificationChannel>(|_| Arc::new(EmailChannel))
    .keyed_singleton::<dyn INotificationChannel>("sms", |_| Arc::new(SmsChannel))
    .keyed_singleton::<dyn INotificationChannel>("push", |_| Arc::new(PushChannel))
    .build()
    .unwrap();
```

**Consumption side:**

```rust
// Get the default (unkeyed) implementation
let default: Arc<dyn INotificationChannel> = provider.get().unwrap();

// Get a specific named implementation
let sms: Arc<dyn INotificationChannel> = provider.get_keyed("sms").unwrap();

// Get ALL implementations at once (unkeyed + all keyed)
let all: Vec<Arc<dyn INotificationChannel>> = provider.get_all();

// Broadcast a message to every channel
for channel in &all {
    channel.send("System alert: disk usage > 90%");
}
```

### Keyed services: runtime strategy routing

Keyed services let you select an implementation at runtime by name —
the foundation of the Strategy pattern.

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

### Scoped services: per-request isolation

Scoped services are created once per scope and cached within it. The root
`ServiceProvider` itself acts as the root scope — use child scopes for request-level isolation.

**Registration side:**

```rust
pub trait IUnitOfWork: Send + Sync {
    fn commit(&self) -> Result<(), String>;
    fn rollback(&self);
}

let provider = ServiceCollection::new()
    .singleton::<dyn IDbPool>(|_| Arc::new(DbPool::new()))   // global
    .scoped::<dyn IUnitOfWork>(|r| {                          // per-scope
        let db: Arc<dyn IDbPool> = r.get().unwrap();
        Arc::new(UnitOfWork::new(db))
    })
    .transient::<dyn IOrderService>(|r| {                     // fresh each time
        let uow: Arc<dyn IUnitOfWork> = r.get().unwrap();
        Arc::new(OrderService::new(uow))
    })
    .build()
    .unwrap();
```

**Consumption side:**

```rust
// ── Root scope: UoW is cached, always returns the same instance ──
let uow1: Arc<dyn IUnitOfWork> = provider.get().unwrap();
let uow2: Arc<dyn IUnitOfWork> = provider.get().unwrap();
// uow1 and uow2 are the SAME instance (root scope cache)

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
// OrderService's IUnitOfWork is scope_a's UoW, not the root's
```

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
- [Business scenario tutorials](docs/rust-dix/08-best-practices/business-scenarios.md) — real-world patterns
- [Integration tests](crates/core/tests/) — real-world usage patterns

## License

MIT.