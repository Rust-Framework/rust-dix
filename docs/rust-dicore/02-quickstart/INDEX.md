# 第二章 快速上手

本章带你快速入门 rust-dicore，从安装到第一个 DI 容器。

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
rust-dicore = "0.4"
```

如果使用过程宏：

```toml
[dependencies]
rust-dicore = "0.4"
rust-dicore-macros = "0.4"
```

## 导入约定

> `rust-dicore` 已重新导出全部宏与类型，**无需单独依赖 `rust-dicore-macros`**。

在每个使用 DI 的文件顶部加两行 use，之后所有宏（`#[inject]`、`#[derive(Inject)]`、`#[module]`、`register!()`）与类型（`ServiceCollection`、`ServiceProvider` 等）均可直接短名引用：

```rust
use rust_dicore::*;
use std::sync::Arc;
```

完整导入约定与路径简化对照表详见 [用法指南 · 导入约定](../08-best-practices/usage-guide.md#1-导入约定路径简化)。

## 第一个 DI 容器

```rust
use rust_dicore::*;
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
logger.log("Hello, rust-dicore!");
```

## 生命周期快速选择

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池 | Singleton |
| 请求上下文、事务 | Scoped |
| 无状态轻量服务 | Transient |

## 下一步

从 [安装与依赖配置](installation.md) 开始。