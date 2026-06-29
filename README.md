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

**rust-dicore** (formerly LRDI) is a Cargo workspace delivering a dependency injection framework for
Rust, inspired by Microsoft.Extensions.DependencyInjection (MEDI).

## Crate overview

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`rust-dicore`](./crates/rust-dicore/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`rust-dicore-macros`](./crates/rust-dicore-macros/) | Procedural macros — `#[derive(Inject)]`, `#[rust_dicore::inject]`, `#[rust_dicore::module]` | Anyone using `rust-dicore` who wants compile-time code generation instead of manual wiring |

## Quick Start

```rust
use rust_dicore::*;
use std::sync::Arc;

// Annotate your structs with #[rust_dicore::inject(...)] —
// constructor + auto-registration, one attribute
#[rust_dicore::inject(singleton)]
struct Config;

#[rust_dicore::inject(transient)]
struct Worker { logger: Arc<Config> }

// Auto-discover all #[rust_dicore::inject] annotations and build
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let logger: Arc<Config> = provider.get();
```

```toml
[dependencies]
rust-dicore = "0.4"
```

## Features

| Feature | Description |
|---------|-------------|
| `#[rust_dicore::inject]` | Attribute-macro-based auto-registration — annotate structs **or trait impls** and call `ServiceCollection::from_injected()` |
| `#[derive(Inject)]` | Derive-macro for constructor injection on structs (advanced) |
| `#[rust_dicore::module]` | Compile-time module scanning to collect `rust_dicore::register!(...)` declarations (advanced) |
| **Three lifetimes** | Singleton (default), Scoped, Transient |
| **Keyed services** | Multiple instances of the same type distinguished by a string key |
| **Cross-DLL support** | Named service registry for cdylib plugin systems |
| **Layered containers** | Child-first resolution via `ServiceProviderWrapper` |
| **Owned resolution** | `get_owned::<T>()` returns owned `T` for `&mut self` methods — no interior mutability needed |

## When should you use rust-dicore?

- **You are building a medium-to-large Rust application** where manually
  constructing dependencies becomes tedious and error-prone.
- **You need testability** — swap real implementations with mocks by changing
  one registration line.
- **You are designing a plugin system** — named services and layered containers
  let host and plugin each have their own DI container.
- **You come from the .NET ecosystem** and want the same
  `IServiceCollection` / `IServiceProvider` patterns in Rust.

## Further reading

- [crates/rust-dicore — core library docs](./crates/rust-dicore/README.md)
- [crates/rust-dicore-macros — procedural macro docs](./crates/rust-dicore-macros/README.md)

## License

MIT.
