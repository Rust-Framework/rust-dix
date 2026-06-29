<p align="center">
  <img src="assets/logo.svg" alt="rust-dicore" width="80">
</p>

<h1 align="center">rust-dicore · Rust 依赖注入框架</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dicore"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dicore.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dicore"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dicore?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
</p>

## 中文

**rust-dicore** 是一个 Cargo 工作空间（workspace），提供一个受
Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架。

### 工作空间包含的 crate

| Crate | 功能 | 适用人群 |
|-------|------|---------|
| [`rust-dicore`](./crates/rust-dicore/) | 核心 DI 容器 — 注册、构建、解析、作用域 | 任何希望解耦、可测试、可插拔架构的 Rust 应用 |
| [`rust-dicore-macros`](./crates/rust-dicore-macros/) | 过程宏 — `#[derive(Inject)]`、`#[rust_dicore::inject]`、`#[rust_dicore::module]` | 使用 `rust-dicore` 并希望编译期自动生成代码的用户 |

### 快速开始

```toml
[dependencies]
rust-dicore = "0.4"
```

```rust
use rust_dicore::*;
use std::sync::Arc;

// 用 #[rust_dicore::inject(...)] 标注你的结构体 —
// 一个属性同时完成构造函数生成和自动注册
#[rust_dicore::inject(singleton)]
struct Config;

#[rust_dicore::inject(transient)]
struct Worker { logger: Arc<Config> }

// 自动发现所有 #[rust_dicore::inject] 标注并构建
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let logger: Arc<Config> = provider.get();
```

### 特性

| 特性 | 描述 |
|------|------|
| `#[rust_dicore::inject]` | 基于属性宏的自动注册 — 标注**结构体或 trait impl 块**后调用 `ServiceCollection::from_injected()` |
| `#[derive(Inject)]` | 派生宏实现构造器注入（高级功能） |
| `#[rust_dicore::module]` | 编译期模块扫描，收集 `rust_dicore::register!(...)` 声明（高级功能） |
| **三种生命周期** | Singleton（默认）、Scoped、Transient |
| **键控服务** | 同一类型的多个实例以字符串 key 区分 |
| **跨 DLL 支持** | 为 cdylib 插件系统提供的命名服务注册表 |
| **分层容器** | 通过 `ServiceProviderWrapper` 实现子优先解析 |

### 什么时候用 rust-dicore？

- **你在构建中等规模以上的 Rust 应用**，手动拼接依赖链变得繁琐且易出错。
- **你需要可测试性** — 改一行注册代码就能用 mock 替换真实实现。
- **你在设计插件系统** — 命名服务和分层容器让宿主和插件各自拥有独立的 DI 容器。
- **你来自 .NET 生态**，希望在 Rust 中使用同样的 `IServiceCollection` / `IServiceProvider` 模式。

### 目录

- [在哪些场景下灵活应用？](./crates/rust-dicore/README.md#-在哪些场景下灵活应用)
- [crates/rust-dicore — 主库完整文档](./crates/rust-dicore/README.md)
- [crates/rust-dicore-macros — 过程宏文档](./crates/rust-dicore-macros/README.md)

---

## License

MIT.
