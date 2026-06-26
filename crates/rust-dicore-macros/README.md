<p align="center">
  <img src="../assets/logo.svg" alt="rust-dicore" width="80">
</p>

<h1 align="center">rust-dicore-macros · Compile-time DI Code Generation</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dicore-macros"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dicore-macros.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dicore-macros"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dicore-macros?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.zh.md"><strong>中文</strong></a>
</p>

`rust-dicore-macros` provides procedural macros for the [`rust-dicore`](../rust-dicore/) DI
framework. These macros shift work from runtime to compile time, eliminating
boilerplate and letting you declare DI configuration as close to your types
as possible.

```toml
[dependencies]
rust-dicore = "0.1"       # re-exports all macros — you usually don't need rust-dicore-macros directly
```

> **Note**: You rarely need to add `rust-dicore-macros` as a direct dependency.
> The `rust-dicore` crate re-exports all three macros (`Inject`, `module`, `inject`).
> Just `use rust_dicore::*` and you're set.

---

### Macros

#### `#[rust_dicore::inject(...)]` — Attribute-based auto-registration (recommended)

**What it does**: Combines constructor generation and service registration in
one attribute. Annotate a struct with a lifetime and optional interface binding,
and the macro generates the constructor function AND an `inventory` submission
entry. `ServiceCollection::from_injected()` collects all entries at startup.

```rust
use rust_dicore::*;

#[rust_dicore::inject(singleton)]
struct Config;

#[rust_dicore::inject(transient, as = dyn UserRepo)]
struct PgUserRepo { db: Arc<DbPool> }

let provider = ServiceCollection::from_injected().build().unwrap();
```

**Supported syntax**:

| Attribute | Registers as |
|-----------|-------------|
| `#[rust_dicore::inject(singleton)]` | Concrete type, Singleton |
| `#[rust_dicore::inject(transient)]` | Concrete type, Transient |
| `#[rust_dicore::inject(scoped)]` | Concrete type, Scoped |
| `#[rust_dicore::inject(transient, as = dyn Trait)]` | `dyn Trait`, Transient |
| `#[rust_dicore::inject(singleton, as = [dyn A, dyn B])]` | Multiple traits, Singleton |

Supports both named-field structs and unit structs (zero fields):

```rust
#[rust_dicore::inject(singleton)]
struct UnitService;  // unit struct — works!

#[rust_dicore::inject(transient)]
struct NamedService {
    dep: Arc<UnitService>,
}
```

This macro subsumes `#[derive(Inject)]` — it internally generates the same
constructor function (`__rdi_construct_{Type}`) using the same field attribute
syntax (`#[inject(skip)]`, `#[inject(optional)]`, `#[inject(key = "...")]`,
etc.). You do NOT need both.

---

#### `#[derive(Inject)]` — Auto-generated constructor

**What it does**: Reads the struct fields and their `#[inject(...)]` attributes,
then generates a factory function `__rdi_construct_{TypeName}()` that resolves
each field from the container.

> **Note**: This derive macro is subsumed by `#[rust_dicore::inject(...)]`. You only
> need `#[derive(Inject)]` if you want to generate the constructor without
> automatic registration.

**Without it**, you write factory closures manually:

```rust
let provider = ServiceCollection::new()
    .singleton(|p| {
        Arc::new(MyService::new(
            p.get::<Logger>(),
            p.get_keyed::<Cache>("main"),
        ))
    })
    .build().unwrap();
```

**With it**, the macro generates the exact same code:

```rust
#[derive(Inject)]
struct MyService {
    logger: Arc<Logger>,
    #[inject(key = "main")]
    cache: Arc<Cache>,
}
// Generates: __rdi_construct_MyService(resolver) -> Arc<MyService>
```

The generated function can be used anywhere you'd write a factory closure:

```rust
let provider = ServiceCollection::new()
    .singleton(__rdi_construct_MyService)
    .build().unwrap();
```

**Field attributes**:

| Attribute | Field type | Behavior |
|-----------|-----------|----------|
| *(none)* | `Arc<T>` | Required — panics if `T` not registered |
| `#[inject(skip)]` | any | Uses `Default::default()` |
| `#[inject(optional)]` | `Option<Arc<T>>` | Returns `None` if `T` not registered |
| `#[inject(key = "k")]` | `Arc<T>` | Keyed resolution, panics if key missing |
| `#[inject(optional, key = "k")]` | `Option<Arc<T>>` | Optional keyed resolution |
| `#[inject(provider)]` | `Arc<dyn IServiceResolver>` | Injects the container itself |

---

#### `#[rust_dicore::module]` — Compile-time module scanning

**What it does**: Scans a `mod` block for `rust_dicore::inject!()` declarations at
compile time, then generates a `__rdi_build_provider_{module_name}()` function
that constructs a fully configured `ServiceProvider`.

**Without it**, you register services one by one in a central setup function:

```rust
fn build_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MyService::default()))
        .singleton(|_| Arc::new(MyPlugin::default()))
        .keyed_singleton("verbose", |_| Arc::new(Logger::default()))
        .build().unwrap()
}
```

**With it**, you declare registrations right inside the module:

```rust
#[rust_dicore::module]
mod services {
    rust_dicore::inject!(singleton: MyService);
    rust_dicore::inject!(singleton: dyn IPlugin => MyPlugin);
    rust_dicore::inject!(keyed "verbose": singleton: Logger);
}

// Generates: services::__rdi_build_provider_services() -> Result<Arc<ServiceProvider>>
let provider = services::__rdi_build_provider_services().unwrap();
```

**Supported declarations**:

| Syntax | Meaning |
|--------|---------|
| `inject!(singleton: T)` | Register `T` as singleton using `Default` |
| `inject!(scoped: T)` | Register `T` as scoped using `Default` |
| `inject!(transient: T)` | Register `T` as transient using `Default` |
| `inject!(singleton: dyn Trait => Impl)` | Register `Impl` as singleton implementing `Trait` |
| `inject!(keyed "k": singleton: T)` | Register `T` as keyed singleton |
| `inject!(keyed "k": scoped: T)` | Register `T` as keyed scoped |
| `inject!(factory singleton: T => expr)` | Register with a custom factory expression |

**When to use it**:

- You want service registrations physically close to the module that owns them
- You are building a library or plugin and want to ship a pre-configured
  provider builder
- You prefer a declarative DSL over method chaining

---

#### `rust_dicore::inject!` — The declaration macro

This is the macro used inside `#[rust_dicore::module]` blocks to declare service
registrations. It expands to nothing at the item level — its only effect is
being collected by the enclosing `#[rust_dicore::module]` attribute.

You can also use `inject!` standalone as a `macro_export`:

```rust
rust_dicore::inject!(singleton: MyService);
```

---

### Relationship with rust-dicore

`rust-dicore-macros` is a companion to [`rust-dicore`](../rust-dicore/). You can use `rust-dicore`
without any macros — just write factory closures manually — and everything
works. The macros exist to eliminate repetition when you have many services
or want to declare DI configuration declaratively.

| Approach | Pros | Cons |
|----------|------|------|
| `#[rust_dicore::inject]` (recommended) | Zero boilerplate, auto-registration | Requires `inventory`; `String` fields need `#[inject(skip)]` |
| Manual factories | Full control, explicit | Boilerplate for many services |
| `#[derive(Inject)]` | Zero boilerplate for constructors | Manual registration still needed |
| `#[rust_dicore::module]` | Declarative, centralized | Less flexible for dynamic registration |

All approaches can be mixed freely in the same project.

---

### License

MIT.
