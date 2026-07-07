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
| [`rust-dicore`](./crates/core/) | 核心 DI 容器 — 注册、构建、解析、作用域 | 任何希望解耦、可测试、可插拔架构的 Rust 应用 |
| [`rust-dicore-macros`](./crates/macros/) | 过程宏 — `#[derive(Inject)]`、`#[rust_dicore::inject]`、`#[rust_dicore::module]` | 使用 `rust-dicore` 并希望编译期自动生成代码的用户 |

### 快速开始

```toml
[dependencies]
rust-dicore = "0.5"
```

```rust
use rust_dicore::*;
use std::sync::Arc;

// 用 #[inject(...)] 标注你的结构体 —
// 一个属性同时完成构造函数生成和自动注册
#[inject(singleton)]
struct Config;

#[inject(transient)]
struct Worker { config: Arc<Config> }

// 自动发现所有 #[inject] 标注并构建
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let worker: Arc<Worker> = provider.get().unwrap();
```

### 异步应用（如 axum / actix-web）

```rust
use rust_dicore::*;
use std::sync::Arc;

struct DbPool { conn: String }

let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async {
        // 异步连接、配置加载等
        Arc::new(DbPool { conn: "pg://localhost".into() })
    }))
    .build_async()
    .await
    .unwrap();

let pool: Arc<DbPool> = provider.get_async().await.unwrap();
```

### 特性

| 特性 | 描述 |
|------|------|
| `#[inject]` | 基于属性宏的自动注册 — 标注**结构体或 trait impl 块**后调用 `ServiceCollection::from_injected()` |
| `#[derive(Inject)]` | 派生宏实现构造器注入 |
| `#[module]` | 编译期模块扫描，收集 `register!(...)` 声明 |
| **三种生命周期** | Singleton、Scoped、Transient |
| **键控服务** | 同一 trait 的多实现以字符串 key 区分 |
| **异步支持** | `build_async()` + `async_*` 注册，异步初始化 |
| **构建期验证** | 循环依赖检测、captive dependency 拒绝 |
| **跨 DLL 支持** | 为 cdylib 插件系统提供的命名服务注册表 |
| **分层容器** | 通过 `ServiceProviderWrapper` 实现子优先解析 |
| **Owned 解析** | `get_owned::<T>()` 返回 owned `T`，支持 `&mut self` — 无需 interior mutability |
| **Scoped 清理** | 清理闭包 + 自动 `Drop`，Scoped 资源释放 |

### 什么时候用 rust-dicore？

- **你在构建中等规模以上的 Rust 应用**，手动拼接依赖链变得繁琐且易出错。
- **你需要可测试性** — 改一行注册代码就能用 mock 替换真实实现。
- **你在设计插件系统** — 命名服务和分层容器让宿主和插件各自拥有独立的 DI 容器。
- **你来自 .NET 生态**，希望在 Rust 中使用同样的 `IServiceCollection` / `IServiceProvider` 模式。

### 目录

- [在哪些场景下灵活应用？](./crates/core/README.md#-在哪些场景下灵活应用)
- [crates/core — 主库完整文档](./crates/core/README.md)
- [crates/macros — 过程宏文档](./crates/macros/README.md)

---

## License

MIT.
