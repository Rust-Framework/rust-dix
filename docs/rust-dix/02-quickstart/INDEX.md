# 第二章 快速上手

本章带你快速入门 rust-dix，从安装到第一个 DI 容器。

## 本章小节

| 小节 | 内容 |
|------|------|
| 安装与依赖配置（规划中） | Cargo.toml 配置 |
| 第一个 DI 容器（规划中） | 基础示例 |
| 生命周期选择（规划中） | Singleton/Scoped/Transient 决策 |

> 本章的独立小节页尚未发布；下方为章节页内置的快速上手内容。

## 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
rust-dix = "0.9"
```

如果使用过程宏：

```toml
[dependencies]
rust-dix = "0.9"
rust-dix-macros = "0.9"
```

> **MSRV**：Rust **1.95**。两个 crate 均 `#![forbid(unsafe_code)]`；CI 跑测试矩阵 +
> RustSec `cargo audit`；安全策略见仓库根目录 `SECURITY.md`。

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
let logger: Arc<dyn ILogger> = provider.get().unwrap();
logger.log("Hello, rust-dix!");
```

## 生命周期快速选择

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池 | Singleton |
| 请求上下文、事务 | Scoped |
| 无状态轻量服务 | Transient |

## 0.9.x 关键语义（先记住这几条）

| 规则 | 说明 |
|------|------|
| **最后注册优先** | `get` / `get_keyed*` / `get_owned` / `get_async` 返回最后注册；`try_add*` = 未注册才添加 |
| **严格 scope 校验** | root 解析 Scoped 返回 `ScopedFromRootProvider`，请求级服务在 `provider.scope()` 内解析；owned/`get_owned` 例外 |
| **类型化键 / AnyKey** | keyed API 接受 `impl Into<ServiceKey>`；`keyed_any_*` 兜底（精确优先、按 (注册/类型, key) 缓存、单值 `any_key()` 非法、字符串键用 `from/into`） |
| **异步解析/枚举** | 同步解析 async 服务 → `AsyncServiceRequiresGetAsync`；枚举含已初始化 async singleton；跨任务传 `AsyncFactoryContext` |
| **释放/诊断** | `dispose()` 逆序释放、幂等；`is_circular()` 为启发式；诊断错误见参考章 |

完整语义与边界（尤其 async 枚举、AnyKey owned/预填充、错误目录）见
[0.9 语义权威页](../09-reference/semantics-0.9.md) 与
[与 MEDI 的差异清单](../01-introduction/medi-differences.md)。

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

混合同步与异步注册（依赖注入用 `#[inject]` 字段完成，闭包内不要调用 `r.get::<T>()`）：

```rust
use rust_dix::*;
use std::sync::Arc;

struct DbPool;
struct AppConfig { port: u16 }

// #[inject(scoped)] 注册为 Scoped；#[inject] 字段自动从容器解析
#[rust_dix::inject(scoped)]
struct UserRepository {
    #[inject]
    db: Arc<DbPool>,
}

let provider = ServiceCollection::from_injected()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool) }))
    .singleton(|_| Arc::new(AppConfig { port: 8080 }))
    .build_async()
    .await
    .unwrap();
```

> **关键规则**：一旦使用 `async_*` 注册，必须调用 `build_async()`。同步 `build()` 会返回
> `Err(RdiError::AsyncServiceRequiresBuildAsync)`；而**同步 `get`/`get_optional` 解析 async 服务**
> 返回 `AsyncServiceRequiresGetAsync`（请用 `get_async`）。async 工厂内可捕获
> `sp.async_context()` 传入 spawned task 以检测跨任务环。

## 下一步

独立小节页规划中；关于导入约定与场景选择，可继续阅读
[用法指南与决策矩阵](../08-best-practices/usage-guide.md)，
或了解 [与 MEDI 的差异清单](../01-introduction/medi-differences.md)。