# 第二章 快速上手

本章带你快速入门 rust-dix，从安装到第一个 DI 容器。

## 本章小节

| 小节 | 内容 |
|------|------|
| [安装与依赖配置](installation.md) | Cargo.toml 配置 |
| [第一个 DI 容器](first-container.md) | 基础示例 |
| [生命周期选择](lifetime-choice.md) | Singleton/Scoped/Transient 决策 |

## 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
rust-dix = "0.7"
```

如果使用过程宏：

```toml
[dependencies]
rust-dix = "0.7"
rust-dix-macros = "0.7"
```

## 导入约定

> `rust-dix` 已重新导出全部宏与类型，**无需单独依赖 `rust-dix-macros`**。

在每个使用 DI 的文件顶部加两行 use，之后所有宏（`#[inject]`、`#[derive(Inject)]`、`#[module]`、`register!()`）与类型（`ServiceCollection`、`ServiceProvider` 等）均可直接短名引用：

```rust
use rust_dix::*;
use std::sync::Arc;
```

完整导入约定与路径简化对照表详见 [用法指南 · 导入约定](../08-best-practices/usage-guide.md#1-导入约定路径简化)。

## 第一个 DI 容器

```rust
use rust_dix::*;
use std::sync::Arc;

// 定义接口
trait ILogger: Send + Sync {
    fn log(&self, msg: &str);
}

// 定义实现
struct ConsoleLogger;
impl ILogger for ConsoleLogger {
    fn log(&self, msg: &str) { println!("[LOG] {}", msg); }
}

// 注册并构建容器
let provider = ServiceCollection::new()
    .singleton::<dyn ILogger>(|_| Arc::new(ConsoleLogger))
    .build()
    .unwrap();

// 解析服务
let logger: Arc<dyn ILogger> = provider.get();
logger.log("Hello, rust-dix!");
```

## 生命周期快速选择

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池 | Singleton |
| 请求上下文、事务 | Scoped |
| 无状态轻量服务 | Transient |

## 异步容器（推荐 Web 应用使用）

当服务需要异步初始化（如数据库连接、Redis 客户端），使用 `build_async()`：

```rust
use rust_dix::*;
use std::sync::Arc;

struct DbPool { conn_str: String }

// 异步初始化数据库连接池
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async {
        Arc::new(DbPool { conn_str: "pg://localhost".into() })
    }))
    .build_async()
    .await
    .unwrap();

// 解析异步服务
let pool: Arc<DbPool> = provider.get_async().await.unwrap();
```

混合同步与异步注册：

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool::new()) }))
    .singleton(|_| Arc::new(AppConfig { port: 8080 }))
    .scoped(|r| {
        let db = r.get::<DbPool>().unwrap();
        Arc::new(UserRepository { db })
    })
    .build_async()
    .await
    .unwrap();
```

> **关键规则**：一旦使用 `async_*` 注册，必须调用 `build_async()`。同步 `build()` 会返回 `Err(RdiError::AsyncServiceRequiresBuildAsync)`（0.7+），而不会在后续 sync `get()` 时才 panic。

## 下一步

从 [安装与依赖配置](installation.md) 开始。