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
rust-dix = "0.7"
```

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

// With DI: declare once, resolve anywhere
// (`MyService` uses `#[derive(Inject)]`; its definition is omitted here)
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger::new("app")))
    .singleton(|_| Arc::new(Cache::new(1024)))
    .transient(|p| __rdi_construct_MyService(p))
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
    ServiceCollection::new()
        .singleton(|_| Arc::new(MockDatabase::new()))
        .transient(|p| __rdi_construct_MyApi(p))
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
each distinguished by a string key. All three lifetimes are supported:

```rust
// Keyed with different lifetimes
let provider = ServiceCollection::new()
    // Singleton keyed - shared globally
    .keyed_singleton::<dyn Strategy>("fast", |_| Arc::new(FastPath))
    // Scoped keyed - shared within a scope
    .keyed_scoped::<dyn Strategy>("session", |_| Arc::new(SessionStrategy))
    // Transient keyed - new instance each time
    .keyed_transient::<dyn Strategy>("fresh", |_| Arc::new(FreshStrategy))
    .build()
    .unwrap();

let fast: Arc<dyn Strategy> = provider.get_keyed("fast").unwrap();
	let session: Arc<dyn Strategy> = provider.scope().get_keyed("session").unwrap();
	let fresh1: Arc<dyn Strategy> = provider.get_keyed("fresh").unwrap();
	let fresh2: Arc<dyn Strategy> = provider.get_keyed("fresh").unwrap();  // different instance
```

**5. You are building a plugin / modular system**

- **Layered containers**: child-first, root-fallback resolution for plugin
  isolation. The plugin sees its own services first; host services are visible
  as fallback.
- **Named services**: register services by string name — critical for cdylib
  plugins where Rust's `TypeId` differs across compilation units.
- **`IServiceLocator`**: pass a unified DI interface to external modules that
  don't depend on rust-dix directly.

---

### Quick Reference · API

### Registration (`ServiceCollection`)

| Method | Lifetime | Use when |
|--------|----------|----------|
| `.from_injected()` | *mixed* | Collect all `#[rust_dix::inject]` annotations (see below) |
| `.singleton(f)` | Singleton | You need exactly one instance shared globally |
| `.scoped(f)` | Scoped | You need one instance per scope (request/transaction) |
| `.transient(f)` | Transient | You need a new instance every time |
| `.keyed_singleton(k, f)` | Singleton | Multiple named instances (strategy pattern) |
| `.keyed_scoped(k, f)` | Scoped | Named instances scoped per request |
| `.keyed_transient(k, f)` | Transient | Named instances created fresh each time |
| `.instance(arc)` | Singleton | You already have a built `Arc<T>` |
| `.try_add(f)` | Singleton | Only register if not already present |
| `.singleton_value(v)` | Singleton | Register a plain value (wraps in `Arc`) |
| `.add(lt, f)` | *any* | Specify lifetime explicitly |

The factory closure `f` receives `&dyn IServiceResolver` so you can resolve
dependencies:

```rust
ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))
    .transient(|p| Arc::new(Repo::new(p.get::<Pool>().unwrap())))
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
| `#[inject]` on `impl Trait for T` | `dyn Trait` | Reuses `__rdi_construct_T` |
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
| `.get::<T>()` | `Result<Arc<T>, RdiError>` | Returns Err if not registered; use `.unwrap()` for required services |
| `.get_optional::<T>()` | `Option<Arc<T>>` | Returns `None` if not registered |
| `.get_keyed::<T>(key)` | `Result<Arc<T>, RdiError>` | Returns Err if key not found; use `.unwrap()` for required keys |
| `.try_get_keyed::<T>(key)` | `Option<Arc<T>>` | Returns `None` if key not found |
| `.get_all::<T>()` | `Vec<Arc<T>>` | All registered instances (keyed + unkeyed) |
| `.get_named::<T>(name)` | `Option<Arc<T>>` | Named resolution (cross-DLL), returns `None` if missing |
| `.get_named_any::<T>(name)` | `Option<Arc<T>>` | Named, returns `None` if missing |
| `.get_owned::<T>()` | `Result<T, RdiError>` | Owned instance; Singleton returns Err; use `.unwrap()` for transient/scoped |
| `.try_get_owned::<T>()` | `Option<T>` | `None` if unregistered or Singleton |
| `.get_keyed_owned::<T>(key)` | `Result<T, RdiError>` | Owned keyed instance; Singleton returns Err |
| `.scope()` | `Scope` | New scope for scoped services |

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
| **Scoped** | Bypasses cache, new instance each call ✓ |
| **Singleton** | Panics (shared instance cannot be owned); use `try_get_owned` for `None` |

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
- Singleton cannot be owned — `get_owned` panics, `try_get_owned` returns `None`.
- Scoped `get_owned` bypasses the scope cache (each call is fresh). This is
  intentional: owned `&mut self` is incompatible with scope-shared semantics.
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
| `.async_keyed_singleton(k, f)` | Singleton keyed | Async init, named instance |
| `.async_keyed_scoped(k, f)` | Scoped keyed | Async init, per-scope named instance |
| `.async_keyed_transient(k, f)` | Transient keyed | Async init, fresh named instance |

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

Use `get_async()` / `get_keyed_async()` when resolving async-registered services:

```rust
let pool: Arc<DbPool> = provider.get_async().await.unwrap();
let gateway: Arc<dyn IPaymentGateway> = provider.get_keyed_async("wechat").await.unwrap();
```

For sync services, you can still use `get()`. The async methods are only needed
for services registered with `async_*`.

#### Mixing sync and async

You can mix sync and async registrations in the same collection. Sync singletons
are validated and initialized alongside async ones during `build_async()`:

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool::new()) }))
    .singleton(|_| Arc::new(AppConfig::new()))       // sync OK
    .scoped(|r| {                                     // sync OK
        let db = r.get::<DbPool>().unwrap();
        Arc::new(UserRepository { db })
    })
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
    .scoped(|r| {
        let db = r.get::<DbPool>().unwrap();
        Arc::new(UserRepository { db })
    })
    .build_async().await.unwrap();

// Each request gets its own scope
fn handle_request(provider: &Arc<ServiceProvider>) {
    let scope = provider.scope();
    let repo: Arc<UserRepository> = scope.get().unwrap();
    // ... process request ...
    // scope dropped → scoped instances released
}
```

---

### Flexible Application Patterns

#### 🔹 Three-layered architecture (Controller → Service → Repository)

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(DbPool::new(&config)))
    .transient(|p| Arc::new(UserRepo::new(p.get::<DbPool>().unwrap())))
    .transient(|p| Arc::new(UserService::new(p.get::<UserRepo>().unwrap())))
    .transient(|p| Arc::new(UserController::new(p.get::<UserService>().unwrap())))
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
│  │  .get()  │ │.get_keyed│ │ .create_scope│ │
│  │ .get_all │ │.get_named│ │              │ │
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
