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
| [`lrdi-macros`](./crates/lrdi-macros/) | Procedural macros — `#[derive(Inject)]`, `#[lrdi::module]` | Anyone using `lrdi` who wants compile-time code generation instead of manual wiring |

## Quick Start

```rust
use lrdi::*;
use std::sync::Arc;

struct Logger { prefix: String }
struct Worker { logger: Arc<Logger> }

let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger { prefix: "INFO".into() }))
    .transient(|p| Arc::new(Worker { logger: p.get() }))
    .build()
    .unwrap();

let logger: Arc<Logger> = provider.get();
```

```toml
[dependencies]
lrdi = "0.1"
```

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
