<p align="center">
  <img src="assets/logo.svg" alt="rust-dicore" width="80">
</p>

<h1 align="center">rust-dicore · Dependency Injection for Rust</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dicore"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dicore.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dicore"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dicore?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.zh.md"><strong>中文</strong></a>
</p>

**rust-dicore** is a dependency injection framework for Rust, inspired by
Microsoft.Extensions.DependencyInjection (MEDI). It provides a service container
with constructor injection, scoped lifetimes, async initialization, and
compile-time validation — all with zero `unsafe` code.

## Crate overview

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`rust-dicore`](./crates/core/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`rust-dicore-macros`](./crates/macros/) | Procedural macros — `#[derive(Inject)]`, `#[rust_dicore::inject]`, `#[rust_dicore::module]` | Anyone using `rust-dicore` who wants compile-time code generation instead of manual wiring |

## Quick Start

```toml
[dependencies]
rust-dicore = "0.5"
```

### Sync applications

```rust
use rust_dicore::*;
use std::sync::Arc;

#[rust_dicore::inject(singleton)]
struct Config;

#[rust_dicore::inject(transient)]
struct Worker { config: Arc<Config> }

let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let worker: Arc<Worker> = provider.get().unwrap();
```

### Async applications (e.g., axum / actix-web)

```rust
use rust_dicore::*;
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
| `#[rust_dicore::inject]` | Attribute-macro-based auto-registration on structs or trait impls |
| `#[derive(Inject)]` | Derive-macro for constructor injection on structs |
| `#[rust_dicore::module]` | Compile-time module scanning via `register!(...)` |
| **Three lifetimes** | Singleton, Scoped, Transient |
| **Keyed services** | Multiple implementations of the same trait by string key |
| **Async support** | `build_async()` + `async_*` registration for services needing async init |
| **Build-time validation** | Circular dependency detection, captive dependency rejection |
| **Cross-DLL support** | Named service registry for cdylib plugin systems |
| **Layered containers** | Child-first resolution via `ServiceProviderWrapper` |
| **Owned resolution** | `get_owned::<T>()` returns owned `T` for `&mut self` — no interior mutability |
| **Scoped cleanup** | Cleanup closures + automatic `Drop` for scoped resource release |

## When should you use rust-dicore?

- **You are building a medium-to-large Rust application** where manually
  constructing dependencies becomes tedious and error-prone.
- **You need testability** — swap real implementations with mocks by changing
  one registration line.
- **You are designing a plugin system** — named services and layered containers
  let host and plugin each have their own DI container.
- **You come from the .NET ecosystem** and want the same
  `IServiceCollection` / `IServiceProvider` patterns in Rust.

## Best practices

### Lifetime selection

| Lifetime | When to use | Example |
|----------|------------|---------|
| **Singleton** | Stateless services, shared configuration, connection pools | `AppConfig`, `DbPool`, `Logger` |
| **Scoped** | Per-request state, unit-of-work objects | `DbContext`, `UserRepository`, `UnitOfWork` |
| **Transient** | Lightweight, stateless workers created fresh each time | `OrderService`, `Validator`, `EmailSender` |

**Rule**: Singleton must not depend on Scoped (enforced at build time). Scoped
can depend on Singleton. Transient can depend on anything.

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
use rust_dicore::*;
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
- [Business scenario tutorials](docs/rust-dicore/08-best-practices/business-scenarios.md) — real-world patterns
- [Integration tests](crates/core/tests/) — real-world usage patterns

## License

MIT.