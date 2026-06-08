<p align="center">
  <img src="assets/logo.svg" alt="LRDI" width="80">
</p>

<h1 align="center">LRDI · Rust 依赖注入框架</h1>

<p align="center">
  <a href="https://crates.io/crates/lrdi"><img alt="Crates.io" src="https://img.shields.io/crates/v/lrdi.svg?style=flat-square"></a>
  <a href="https://docs.rs/lrdi"><img alt="Docs" src="https://img.shields.io/docsrs/lrdi?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="#english">English</a> ·
  <a href="#chinese">中文</a>
</p>

---

<h2 id="english">English</h2>

**LRDI** is a Cargo workspace delivering a dependency injection framework for
Rust, inspired by Microsoft.Extensions.DependencyInjection (MEDI).

### What's in this workspace

| Crate | What it does | Who needs it |
|-------|-------------|--------------|
| [`lrdi`](./crates/lrdi/) | Core DI container — register, build, resolve, scope | Any Rust application that wants loose coupling, testability, and pluggable architecture |
| [`lrdi-macros`](./crates/lrdi-macros/) | Procedural macros — `#[derive(Inject)]`, `#[lrdi::module]` | Anyone using `lrdi` who wants compile-time code generation instead of manual wiring |

### Quick Start

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

### When should you use LRDI?

- **You are building a medium-to-large Rust application** where manually
  constructing dependencies becomes tedious and error-prone.
- **You need testability** — swap real implementations with mocks by changing
  one registration line.
- **You are designing a plugin system** — named services and layered containers
  let host and plugin each have their own DI container.
- **You come from the .NET ecosystem** and want the same
  `IServiceCollection` / `IServiceProvider` patterns in Rust.

---

<h2 id="chinese">中文</h2>

**LRDI** 是一个 Cargo 工作空间（workspace），提供一个受
Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架。

### 工作空间包含的 crate

| Crate | 功能 | 适用人群 |
|-------|------|---------|
| [`lrdi`](./crates/lrdi/) | 核心 DI 容器 — 注册、构建、解析、作用域 | 任何希望解耦、可测试、可插拔架构的 Rust 应用 |
| [`lrdi-macros`](./crates/lrdi-macros/) | 过程宏 — `#[derive(Inject)]`、`#[lrdi::module]` | 使用 `lrdi` 并希望编译期自动生成代码的用户 |

### 快速开始

```toml
[dependencies]
lrdi = "0.1"
```

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

### 什么时候用 LRDI？

- **你在构建中等规模以上的 Rust 应用**，手动拼接依赖链变得繁琐且易出错。
- **你需要可测试性** — 改一行注册代码就能用 mock 替换真实实现。
- **你在设计插件系统** — 命名服务和分层容器让宿主和插件各自拥有独立的 DI 容器。
- **你来自 .NET 生态**，希望在 Rust 中使用同样的 `IServiceCollection` / `IServiceProvider` 模式。

### 目录

- [在哪些场景下灵活应用？](./crates/lrdi/README.md#-在哪些场景下灵活应用)
- [crates/lrdi — 主库完整文档](./crates/lrdi/README.md)
- [crates/lrdi-macros — 过程宏文档](./crates/lrdi-macros/README.md)

---

## License

MIT.
