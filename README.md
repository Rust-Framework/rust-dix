<p align="center">
  <img src="assets/logo.svg" alt="LRDI" width="80">
</p>

<h1 align="center">LRDI · Dependency Injection for Rust</h1>

<p align="center">
  <a href="https://crates.io/crates/lrdi"><img alt="Crates.io" src="https://img.shields.io/crates/v/lrdi.svg?style=flat-square"></a>
  <a href="https://docs.rs/lrdi"><img alt="Docs" src="https://img.shields.io/docsrs/lrdi?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.zh.md"><strong>中文</strong></a>
</p>

**LRDI** is a Cargo workspace delivering a dependency injection framework for
Rust, inspired by Microsoft.Extensions.DependencyInjection (MEDI).

## Crate overview

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`lrdi`](./crates/lrdi/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`lrdi-macros`](./crates/lrdi-macros/) | Procedural macros — `#[derive(Inject)]`, `#[lrdi::inject]`, `#[lrdi::module]` | Anyone using `lrdi` who wants compile-time code generation instead of manual wiring |

## Quick Start

```rust
use lrdi::*;
use std::sync::Arc;

// Annotate your structs with #[lrdi::inject(...)] —
// constructor + auto-registration, one attribute
#[lrdi::inject(singleton)]
struct Config;

#[lrdi::inject(transient)]
struct Worker { logger: Arc<Config> }

// Auto-discover all #[lrdi::inject] annotations and build
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let logger: Arc<Config> = provider.get();
```

```toml
[dependencies]
lrdi = "0.2"
```

## Features

| Feature | Description |
|---------|-------------|
| `#[lrdi::inject]` | Attribute-macro-based auto-registration — annotate structs and call `ServiceCollection::from_injected()` |
| `#[derive(Inject)]` | Derive-macro for constructor injection on structs (advanced) |
| `#[lrdi::module]` | Compile-time module scanning to collect `lrdi::inject!(...)` declarations (advanced) |
| **Three lifetimes** | Singleton, Scoped, Transient |
| **Keyed services** | Multiple instances of the same type distinguished by a string key |
| **Cross-DLL support** | Named service registry for cdylib plugin systems |
| **Layered containers** | Child-first resolution via `ServiceProviderWrapper` |

## When should you use LRDI?

- **You are building a medium-to-large Rust application** where manually
  constructing dependencies becomes tedious and error-prone.
- **You need testability** — swap real implementations with mocks by changing
  one registration line.
- **You are designing a plugin system** — named services and layered containers
  let host and plugin each have their own DI container.
- **You come from the .NET ecosystem** and want the same
  `IServiceCollection` / `IServiceProvider` patterns in Rust.

## Further reading

- [crates/lrdi — core library docs](./crates/lrdi/README.md)
- [crates/lrdi-macros — procedural macro docs](./crates/lrdi-macros/README.md)

## License

MIT.
