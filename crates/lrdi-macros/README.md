<p align="center">
  <img src="../assets/logo.svg" alt="LRDI" width="80">
</p>

<h1 align="center">lrdi-macros · Compile-time DI Code Generation</h1>

<p align="center">
  <a href="https://crates.io/crates/lrdi-macros"><img alt="Crates.io" src="https://img.shields.io/crates/v/lrdi-macros.svg?style=flat-square"></a>
  <a href="https://docs.rs/lrdi-macros"><img alt="Docs" src="https://img.shields.io/docsrs/lrdi-macros?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.zh.md"><strong>中文</strong></a>
</p>

`lrdi-macros` provides procedural macros for the [`lrdi`](../lrdi/) DI
framework. These macros shift work from runtime to compile time, eliminating
boilerplate and letting you declare DI configuration as close to your types
as possible.

```toml
[dependencies]
lrdi = "0.1"       # re-exports all macros — you usually don't need lrdi-macros directly
```

> **Note**: You rarely need to add `lrdi-macros` as a direct dependency.
> The `lrdi` crate re-exports all three macros (`Inject`, `module`, `inject`).
> Just `use lrdi::*` and you're set.

---

### Macros

#### `#[derive(Inject)]` — Auto-generated constructor

**What it does**: Reads the struct fields and their `#[inject(...)]` attributes,
then generates a factory function `__lrdi_construct_{TypeName}()` that resolves
each field from the container.

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
use lrdi_macros::Inject;

#[derive(Inject)]
struct MyService {
    logger: Arc<Logger>,
    #[inject(key = "main")]
    cache: Arc<Cache>,
}
// Generates: __lrdi_construct_MyService(resolver) -> Arc<MyService>
```

The generated function can be used anywhere you'd write a factory closure:

```rust
let provider = ServiceCollection::new()
    .singleton(__lrdi_construct_MyService)
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

**When to use it**:

- You have many service structs with repetitive wiring
- You want the DI configuration colocated with the struct definition
- You want compile-time validation that all fields are injectable

---

#### `#[lrdi::module]` — Compile-time module scanning

**What it does**: Scans a `mod` block for `lrdi::inject!()` declarations at
compile time, then generates a `__lrdi_build_provider_{module_name}()` function
that constructs a fully configured `ServiceProvider`.

**Without it**, you register services one by one in a central setup function:

```rust
fn build_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MyService::default()))
        .singleton(|_| Arc::new(MyPlugin::default()))
        .keyed("verbose", |_| Arc::new(Logger::default()))
        .build().unwrap()
}
```

**With it**, you declare registrations right inside the module:

```rust
#[lrdi::module]
mod services {
    lrdi::inject!(singleton: MyService);
    lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
    lrdi::inject!(keyed "verbose": singleton: Logger);
}

// Generates: services::__lrdi_build_provider_services() -> Result<Arc<ServiceProvider>>
let provider = services::__lrdi_build_provider_services().unwrap();
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

#### `lrdi::inject!` — The declaration macro

This is the macro used inside `#[lrdi::module]` blocks to declare service
registrations. It expands to nothing at the item level — its only effect is
being collected by the enclosing `#[lrdi::module]` attribute.

You can also use `inject!` standalone as a `macro_export`:

```rust
lrdi::inject!(singleton: MyService);
```

---

### Relationship with lrdi

`lrdi-macros` is a companion to [`lrdi`](../lrdi/). You can use `lrdi`
without any macros — just write factory closures manually — and everything
works. The macros exist to eliminate repetition when you have many services
or want to declare DI configuration declaratively.

| Approach | Pros | Cons |
|----------|------|------|
| Manual factories | Full control, explicit | Boilerplate for many services |
| `#[derive(Inject)]` | Zero boilerplate for constructors | Only works with named fields |
| `#[lrdi::module]` | Declarative, colocated | Less flexible for dynamic registration |

You can mix all three freely in the same project.

---

### License

MIT.
