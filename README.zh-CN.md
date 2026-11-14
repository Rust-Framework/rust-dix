<p align="center">
  <img src="assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix · Rust 依赖注入框架</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix?style=flat-square"></a>
  <a href="https://rust-framework.github.io/rust-dix/"><img alt="Documentation" src="https://img.shields.io/badge/docs-mdBook-blue.svg?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

**rust-dix** 是一个 Rust 依赖注入框架，灵感来自
Microsoft.Extensions.DependencyInjection (MEDI)。它提供了带有构造函数注入、
作用域生命周期（Scoped）、异步初始化和编译期校验的服务容器 —— 全程零 `unsafe` 代码。

**[在线文档](https://rust-framework.github.io/rust-dix/)** —— 完整开发者手册（mdBook，中文），由 `docs/rust-dix/` 构建。

## Crate 总览

| Crate | 作用 | 适用人群 |
|-------|------|----------|
| [`rust-dix`](./crates/core/README.zh.md) | 核心 DI 容器 —— 注册、构建、解析、作用域 | 任何希望实现松耦合、可测试和可插拔架构的 Rust 应用 |
| [`rust-dix-macros`](./crates/macros/README.zh.md) | 过程宏 —— `#[derive(Inject)]`、`#[rust_dix::inject]`、`#[rust_dix::module]` | 希望用编译期代码生成替代手工装配的 `rust-dix` 使用者 |

## 快速开始

```toml
[dependencies]
rust-dix = "1.0"
```

### 同步应用

```rust
use rust_dix::*;
use std::sync::Arc;

#[rust_dix::inject(singleton)]
struct Config;

#[rust_dix::inject(transient)]
struct Worker { #[inject] config: Arc<Config> }

let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let worker: Arc<Worker> = provider.get().unwrap();
```

### 异步应用（例如 axum / actix-web）

```rust
use rust_dix::*;
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

## 特性

| 特性 | 说明 |
|------|------|
| `#[rust_dix::inject]` | 基于属性宏的自动注册，可标注在结构体或 trait 实现上 |
| `#[derive(Inject)]` | 结构体构造函数注入的派生宏 |
| `#[rust_dix::module]` | 通过 `register!(...)` 实现编译期模块扫描 |
| **三种生命周期** | Singleton、Scoped、Transient |
| **键控服务** | 同一 trait 可通过字符串键或**类型化键**（`ServiceKey`）注册多个实现 |
| **`AnyKey` 兜底** | `keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`：对齐 MEDI `KeyedService.AnyKey`，按 (服务注册/类型, 请求 key) 缓存 |
| **异步支持** | `build_async()` + `async_*` 注册，用于需要异步初始化的服务 |
| **异步上下文** | `provider.async_context()` 把解析链传入 spawn 任务 → 跨任务环检测 |
| **精确诊断** | `SyncSingletonDependsOnAsyncService`、`AsyncServiceRequiresGetAsync`、`RdiError::is_circular()` |
| **构建期校验** | 循环依赖检测、受控依赖（captive dependency）拒绝、构造期解析拒绝 |
| **容器注入** | `#[inject(provider)]`、`Arc<dyn ScopeFactory>`、`Arc<ServiceProvider>` —— eager singleton 与懒解析服务均可用 |
| **跨 DLL 支持** | 面向 cdylib 插件系统的具名服务注册表 |
| **分层容器** | 通过 `ServiceProviderWrapper` 实现子容器优先解析 |
| **拥有式解析** | `get_owned::<T>()` 为 `&mut self` 返回拥有型 `T` —— 无需内部可变性 |
| **确定性释放** | `Scope::dispose()` / `ServiceProvider::dispose()` 按创建逆序释放实例 |
| **内存安全** | 两个 crate 均 `#![forbid(unsafe_code)]`；MSRV **1.95**；CI matrix + RustSec `cargo audit`；见 [SECURITY.md](SECURITY.md) |

## 容器语义（0.10.x）

rust-dix 0.10.x 将单服务解析对齐 MEDI，同时保留构建期安全。以下五条最易踩坑（完整语义：
[docs/rust-dix/09-reference/semantics.md](docs/rust-dix/09-reference/semantics.md)）：

| 规则 | 行为 |
|------|------|
| **最后注册优先** | `get` / `get_keyed*` / `get_owned` / `get_async` 返回**最后注册**的实现（keyed 同 key 后注册生效）；`try_add*` = "未注册才添加"。 |
| **默认严格 scope 校验** | root 解析 Scoped 返回 `ScopedFromRootProvider`，应在 `provider.scope()` 内解析；`get_owned`/keyed owned 是调用方持有例外（不进缓存/不跟踪，传递 Scoped 依赖仍校验）。 |
| **类型化键与 `AnyKey`** | keyed API 接受 `impl Into<ServiceKey>`；精确 key 优先于 `keyed_any_*`；AnyKey 按 **(注册/类型, 请求 key)** 缓存；`get_all_keyed` 不含兜底；`any_key()` 不能做单值解析。 |
| **异步解析与枚举** | async 服务用 `get_async`（同步 `get` → `AsyncServiceRequiresGetAsync`，`get_optional` → `None`）；跨任务环需 `AsyncFactoryContext`；枚举只含已初始化 async singleton（未初始化 → typed panic/`Err`）。 |
| **诊断与基线** | `is_circular()` 为启发式；MSRV 1.95、`#![forbid(unsafe_code)]`、CI matrix + `cargo audit`，见 [SECURITY.md](SECURITY.md)。 |

0.10 新增：宏 key **表达式**（`#[inject(key = <expr>)]` / `#[inject(owned, key = <expr>)]`、
`register!(keyed <expr>: ...)`，接受字符串字面量、typed `ServiceKey::new(...)` 或下游
`impl From<LocalKey> for ServiceKey` 的 key 类型）、`IServiceResolver` typed keyed 方法
（默认字符串回退）、`ServiceKey::as_str()`；参考章含
[扩展契约](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md)。
**async AnyKey 仍未提供**（workaround 见扩展契约）。与 MEDI 的差异见
[docs/rust-dix/01-introduction/medi-differences.md](docs/rust-dix/01-introduction/medi-differences.md)。

## 什么时候应该使用 rust-dix？

- **你在构建一个中型到大型的 Rust 应用**，手动构造依赖会变得繁琐且容易出错。
- **你需要可测试性** —— 只需改一行注册代码，即可把真实实现替换为 mock。
- **你在设计插件系统** —— 具名服务与分层容器让宿主和插件各自拥有独立的 DI 容器。
- **你来自 .NET 生态**，希望在 Rust 中沿用相同的
  `IServiceCollection` / `IServiceProvider` 模式。

## 面向接口编程

rust-dix 的核心原则：**依赖 trait，而非具体类型**。这实现了在测试中替换 mock，
以及在不改动使用方的前提下替换实现。

### 为什么接口很重要

| 不用接口 | 使用接口 |
|---------|---------|
| `Arc<WechatPay>` —— 锁定到单一实现 | `Arc<dyn IPaymentGateway>` —— 任意实现均可 |
| 测试无法替换支付网关 | 用 `MockPaymentGateway` 模拟 |
| 接入支付宝需要改动所有使用方 | 只需注册新的实现 |

### 多态：同一 trait 的多个实现

将多个服务注册在同一 trait 下，然后一次性解析全部。

**注册端：**

```rust
use rust_dix::*;
use std::sync::Arc;

pub trait INotificationChannel: Send + Sync {
    fn send(&self, msg: &str);
}

// 同一 trait 注册两个非 keyed 实现 + 一个 keyed 实现
let provider = ServiceCollection::new()
    .singleton::<dyn INotificationChannel>(|_| Arc::new(EmailChannel))
    .singleton::<dyn INotificationChannel>(|_| Arc::new(SmsChannel))
    .keyed_singleton::<dyn INotificationChannel>("push", |_| Arc::new(PushChannel))
    .build()
    .unwrap();
```

**消费端：**

```rust
// 获取默认（无键）实现 —— 最后注册优先
let default: Arc<dyn INotificationChannel> = provider.get().unwrap(); // SmsChannel

// 获取特定的具名实现
let push: Arc<dyn INotificationChannel> = provider.get_keyed("push").unwrap();

// 一次性获取全部“非 keyed”实现（不包含 keyed）
let all: Vec<Arc<dyn INotificationChannel>> = provider.get_all();
assert_eq!(all.len(), 2);

// keyed 实现需显式取全部
let keyed: Vec<Arc<dyn INotificationChannel>> = provider.get_all_keyed();

// 向每个通道广播消息（非 keyed + keyed）
for channel in all.iter().chain(keyed.iter()) {
    channel.send("系统告警：磁盘使用率 > 90%");
}
```

### 键控服务：运行时策略路由

键控服务让你可以在运行时按 key（字符串或类型化 `ServiceKey` 值）选择实现 —— 这正是
策略（Strategy）模式的基础。

**注册端：**

```rust
pub trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> Result<String, String>;
}

let provider = ServiceCollection::new()
    // 默认（无键）—— 作为回退
    .singleton::<dyn IPaymentGateway>(|_| Arc::new(WechatPay))
    // 具名备选方案
    .keyed_singleton::<dyn IPaymentGateway>("alipay", |_| Arc::new(Alipay))
    .keyed_transient::<dyn IPaymentGateway>("credit_card", |_| Arc::new(CreditCardPay))
    .build()
    .unwrap();
```

**消费端：**

```rust
// ── 模式 A：静态分发（编译期已知键）──
let gateway: Arc<dyn IPaymentGateway> = provider.get_keyed("alipay").unwrap();
gateway.charge(100)?;

// ── 模式 B：动态路由（运行时从请求取键）──
fn process_payment(provider: &ServiceProvider, method: &str, amount: u64) {
    let gateway: Arc<dyn IPaymentGateway> = provider
        .try_get_keyed(method)                          // 返回 Option
        .unwrap_or_else(|| provider.get().unwrap());    // 回退到默认值
    gateway.charge(amount).unwrap();
}

// ── 模式 C：注入 provider 以实现惰性路由 ──
#[derive(Inject)]
struct OrderService {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,  // 按需解析策略
}

impl OrderService {
    fn checkout(&self, method: &str, amount: u64) {
        let gateway: Arc<dyn IPaymentGateway> = self.provider
            .try_get_keyed(method)
            .unwrap_or_else(|| self.provider.get().unwrap());
        gateway.charge(amount).unwrap();
    }
}
```

#### 类型化键（`ServiceKey`）

keyed API 接受 `impl Into<ServiceKey>`，因此除字符串外还可用 enum/整数等类型化键
（字符串调用源码兼容）。键相等包含值的类型。**字符串键易错点**：`ServiceKey::new("x")`
是 `&str` typed key，与 `keyed_singleton("x", …)` 存储的 `String` key **不相等**；
字符串键请用 `"x".into()` / `ServiceKey::from("x")`。

```rust
use rust_dix::*;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Hash)]
enum DbKey { Read, Write }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IDbConnection>(ServiceKey::new(DbKey::Read), |_| Arc::new(ReadPool))
    .keyed_singleton::<dyn IDbConnection>(ServiceKey::new(DbKey::Write), |_| Arc::new(WritePool))
    .build()
    .unwrap();

let read: Arc<dyn IDbConnection> =
    provider.get_keyed(ServiceKey::new(DbKey::Read)).unwrap();

// 值的类型参与比较：1u32 不是字符串 "1"。
assert!(!provider.is_registered_keyed::<dyn IDbConnection>(ServiceKey::new(1u32)));
assert!(provider.get_keyed::<dyn IDbConnection>(ServiceKey::from("1")).is_err());
```

##### 宏 key 表达式（0.10）

`#[inject(key = <expr>)]` / `#[inject(owned, key = <expr>)]` 接受任意
`impl Into<ServiceKey>`。下游 key 类型实现 `impl From<LocalKey> for ServiceKey` 后可直接用
——这是官方扩展路径（**不要复制注入宏**）：

```rust
impl From<DbKey> for ServiceKey {
    fn from(k: DbKey) -> Self { ServiceKey::new(k) }
}

#[derive(Inject)]
struct ReadService {
    #[inject(key = DbKey::Read)]                 // typed key 表达式
    db: Arc<dyn IDbConnection>,
    #[inject(key = "audit")]                     // 字符串字面量仍可用（String key）
    audit: Arc<dyn IAuditLog>,
}
```

`register!(keyed <expr>: <lifetime>: <Type>)` 同样接受表达式；编译期重复检测（`rdi-E004`）
**仅覆盖字符串字面量**，动态 key 按运行时"最后注册优先"。字符串键须用
`ServiceKey::from("x")` / `"x".into()`（`ServiceKey::new("x")` 是 `&str` typed key，不相等）。
0.10 **不提供** async AnyKey —— 见
[扩展契约](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md)。

#### `AnyKey` 兜底（对齐 MEDI `KeyedService.AnyKey`）

`keyed_any_*` 注册一个"无精确 key 时使用"的兜底实现；工厂接收被请求的 key，且
singleton/scoped 兜底实例**按 (服务注册/类型, 请求 key) 分别缓存**——同一注册 + 同一 key 复用
同一实例，不同 key 或不同 AnyKey 注册各自独立（scoped：每 scope 每 (注册, key) 一份）：

```rust
let provider = ServiceCollection::new()
    // 精确 key 优先
    .keyed_singleton::<dyn IDbConnection>("admin", |_| Arc::new(AdminPool))
    // 其它 key 使用兜底；`key` 是请求的 ServiceKey
    .keyed_any_singleton::<dyn IDbConnection>(|_, key| {
        // `key: &ServiceKey` —— 可用于选择/记录租户
        Arc::new(Pool::connect(&format!("{key:?}")))
    })
    .build()
    .unwrap();

let admin: Arc<dyn IDbConnection> = provider.get_keyed("admin").unwrap();   // 精确
let t1: Arc<dyn IDbConnection> = provider.get_keyed("tenant-1").unwrap();  // 兜底，按注册+key 缓存
let t1_again = provider.get_keyed::<dyn IDbConnection>("tenant-1").unwrap();
assert!(Arc::ptr_eq(&t1, &t1_again));

// AnyKey 兜底不进入 get_all_keyed；哨兵本身不能做单值解析：
assert!(provider.get_keyed::<dyn IDbConnection>(ServiceKey::any_key()).is_err());
let explicit = provider.get_all_keyed::<dyn IDbConnection>(); // 只有 "admin"
```

> `keyed_any_*` 仅支持同步注册（异步解析会回退同步路径）；build 期不做 eager 初始化/
> 单独校验（需要请求 key）；`keyed_any_scoped` 被同步 singleton 依赖时仍会按 captive
> dependency 拒绝。build 期被同步 singleton 按某请求 key 解析的 AnyKey singleton 会写入
> **(注册, 请求 key) 缓存**（运行时同一注册 + 同一 key 复用**同一实例**；多消费者共享一次
> 构造）；`AnyKey` 的 scoped/transient 兜底支持
> strict root 下的 `get_keyed_owned`（owned 例外）；工厂内嵌套解析**不同请求 key** 合法
> （只有同一注册 + 同一 key 才是环）；`&ServiceKey` 无 typed downcast 访问器（用
> `Debug`/`Display`/`Eq`/`Hash`，擦除表示私有）。

#### 跨任务异步解析（`AsyncFactoryContext`）

在 async 工厂内捕获 `sp.async_context()`，clone 进 spawned task，可让**跨任务**依赖环
返回 `CircularDependency` 而不是死锁：

```rust
let provider = ServiceCollection::new()
    .async_singleton(|sp| Box::pin(async move {
        let ctx = sp.async_context().expect("built provider");
        let dep = tokio::spawn({
            let ctx = ctx.clone();
            async move { ctx.get_async::<DbPool>().await }
        })
        .await
        .expect("task")
        .expect("DbPool must resolve");
        Arc::new(Service { dep })
    }))
    .build_async()
    .await
    .unwrap();
```

容器注入在 **`build()` 期间**同样成立：eager singleton 可以持有 provider 或内置
scope factory。singleton worker 推荐使用弱引用且单例缓存的 `ScopeFactory` —— 每个
任务创建独立 scope，同时不会因为 `Arc` 环让容器无法回收。singleton 内应优先用它
而非 `#[inject(provider)]`（持有 `Arc<ServiceProvider>` 会形成强引用环），并在方法中
延迟解析：在 build 期构造函数内经注入 provider 解析服务会被拒绝。

```rust
#[derive(Inject)]
struct OutboxWorker {
    #[inject]
    scope_factory: Arc<dyn ScopeFactory>, // 无需手动注册
}

impl OutboxWorker {
    fn tick(&self) {
        let scope = self.scope_factory.create_scope();
        let db: Arc<DbContext> = scope.get().unwrap();
        // ... 在当前 scope 内投递一批消息 ...
    } // scope 释放时清理 scoped 服务
}
```

### 作用域服务：按请求隔离

作用域服务每个作用域创建一次，并在其中缓存。Scoped 服务必须从
`provider.scope()` 创建的子作用域解析 —— 默认严格 scope 校验下，root provider
不解析 Scoped 服务（`Err(RdiError::ScopedFromRootProvider)`）。例外是
`get_owned` / `try_get_owned`：owned 实例归调用方、不进缓存，可从 root 取；
但其 Scoped 依赖仍受校验。

**注册端：**

```rust
pub trait IUnitOfWork: Send + Sync {
    fn commit(&self) -> Result<(), String>;
    fn rollback(&self);
}

#[derive(Inject)]
struct UnitOfWork {
    #[inject]
    db: Arc<dyn IDbPool>,
}

#[rust_dix::inject(scoped)]           // 将 dyn IUnitOfWork 注册为 Scoped
impl IUnitOfWork for UnitOfWork { /* ... */ }

#[derive(Inject)]
struct OrderService {
    #[inject]
    uow: Arc<dyn IUnitOfWork>,
}

#[rust_dix::inject(transient)]        // 将 dyn IOrderService 注册为 Transient
impl IOrderService for OrderService { /* ... */ }

let provider = ServiceCollection::from_injected()
    .singleton::<dyn IDbPool>(|_| Arc::new(DbPool::new()))    // 全局
    .build()
    .unwrap();
```

**消费端：**

```rust
// ── 根 provider：默认严格 options 下无法解析 Scoped ──
let err = provider.get::<dyn IUnitOfWork>(); // Err(RdiError::ScopedFromRootProvider)

// ── 子作用域：每个作用域获得自己的 UoW 实例 ──
let scope_a = provider.scope();
let scope_b = provider.scope();

let uow_a: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
let uow_b: Arc<dyn IUnitOfWork> = scope_b.get().unwrap();
// uow_a != uow_b（不同作用域，不同实例）

// 在同一个子作用域内，Scoped 会被缓存：
let uow_a2: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
// uow_a == uow_a2（同一作用域，已缓存）

// ── 从作用域解析 Transient：在该作用域内解析其 Scoped 依赖 ──
let order_svc: Arc<dyn IOrderService> = scope_a.get().unwrap();
// OrderService 的 IUnitOfWork 是 scope_a 的 UoW

// 作用域 Drop（或显式 dispose）时，按创建逆序释放其 scoped 实例。
```

需要 0.7.x 的 root-scope 旧行为？用 `ServiceProviderOptions::lenient()` 构建 —— 见
[与 MEDI 的差异清单](docs/rust-dix/01-introduction/medi-differences.md#2-scope-校验validatescopes)。

**关键规则**：Singleton 不能依赖 Scoped（在构建时强制执行）。
Scoped 可以依赖 Singleton。Transient 可以依赖任何东西。

有关详细模式（契约层、分层架构、插件系统），请参阅
[业务场景教程](docs/rust-dix/08-best-practices/business-scenarios.md)。

## 最佳实践

### 生命周期选择

| 生命周期 | 使用时机 | 示例 |
|----------|---------|------|
| **Singleton** | 无状态服务、共享配置、连接池 | `AppConfig`、`DbPool`、`Logger` |
| **Scoped** | 按请求状态、工作单元对象 | `DbContext`、`UserRepository`、`UnitOfWork` |
| **Transient** | 每次全新创建的轻量、无状态工作者 | `OrderService`、`Validator`、`EmailSender` |

### 同步与异步注册

```
需要异步初始化（数据库、网络、I/O）？
├── 是 → 使用 build_async() + async_* 注册
│   ├── 全应用共享 → async_singleton
│   ├── 按请求作用域 → async_scoped
│   └── 每次调用全新 → async_transient
└── 否 → 使用 build() + 同步注册
    ├── 全应用共享 → singleton
    ├── 按请求作用域 → scoped
    └── 每次调用全新 → transient
```

**重要**：一旦使用了任何 `async_*` 注册，就必须调用
`build_async()` 而不是 `build()`。在同一集合中混合使用是受支持的 ——
同步单例会与异步单例一起被校验并初始化。

### 跨同步/异步边界的依赖链

同步服务可以依赖异步单例，因为异步单例会在 `build_async()` 期间被缓存。
在同步工厂中使用 `get_any()`：

```rust
// 同步作用域服务依赖异步单例 DbPool
.scoped(|r| {
    let db = r.get_any(std::any::type_name::<DbPool>())
        .and_then(|a| a.downcast::<Arc<DbPool>>().ok().map(|d| Arc::clone(&*d)))
        .unwrap();
    Arc::new(UserRepository { db })
})
```

异步服务可以通过 `Arc<ServiceProvider>`
参数依赖同步单例：

```rust
.async_singleton(|p: Arc<ServiceProvider>| {
    Box::pin(async move {
        let config: Arc<AppConfig> = p.get().unwrap();
        Arc::new(DbPool { conn_str: config.conn_str.clone() })
    })
})
```

### 作用域生命周期（按请求模式）

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool { ... }) }))
    .scoped(|r| { /* UserRepository 依赖 DbPool */ Arc::new(...) })
    .build_async().await.unwrap();

// 每个请求获得自己的作用域
fn handle_request(provider: &Arc<ServiceProvider>) {
    let scope = provider.scope();           // 创建隔离的作用域
    let repo = scope.get::<UserRepository>().unwrap();
    // ... 处理请求 ...
    // scope 在此处被丢弃 → 作用域实例被释放
}
```

### 错误处理

- 对可选依赖，优先使用 `try_get::<T>()` / `get_optional::<T>()`（返回 `Option`）
- 当依赖是必需项时，使用 `get::<T>()`（返回 `Result`）——
  缺失是可恢复的错误，而不是 panic

### 架构模式

有关常见设计模式（Strategy、Factory、Decorator、Observer、Chain of
Responsibility、Pipeline、Plugin、Composite、Proxy、Feature Toggle），请参阅
[模式测试套件](crates/core/tests/patterns.rs)。

## 真实示例：Web 应用

```rust
use rust_dix::*;
use std::sync::Arc;

// ── 基础设施（异步）──
struct DbPool { conn_str: String }
struct RedisCache { url: String }

// ── 业务服务 ──
struct AppConfig { max_retries: u32 }
struct UserRepository { db: Arc<DbPool>, cache: Arc<RedisCache>, config: Arc<AppConfig> }
struct OrderService { repo: Arc<UserRepository> }

trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> bool;
}

#[tokio::main]
async fn main() {
    let provider = ServiceCollection::new()
        // 异步基础设施 —— 在构建时初始化
        .async_singleton(|_| Box::pin(async {
            Arc::new(DbPool { conn_str: "pg://localhost".into() })
        }))
        .async_singleton(|_| Box::pin(async {
            Arc::new(RedisCache { url: "redis://localhost".into() })
        }))
        .async_keyed_singleton::<dyn IPaymentGateway>("wechat", |_| {
            Box::pin(async { Arc::new(WechatPay { ... }) as Arc<dyn IPaymentGateway> })
        })
        // 同步服务
        .singleton(|_| Arc::new(AppConfig { max_retries: 3 }))
        .scoped(|r| {
            let db = /* 通过 get_any 解析 DbPool */;
            Arc::new(UserRepository { db, ... })
        })
        .transient(|r| {
            let repo = /* 解析 UserRepository */;
            Arc::new(OrderService { repo })
        })
        .build_async()
        .await
        .unwrap();

    // 按请求作用域
    let scope = provider.scope();
    let order_svc: Arc<OrderService> = scope.get().unwrap();
    // ... 处理请求 ...
}
```

完整可运行示例：[`async_integration.rs`](crates/core/tests/async_integration.rs)

## 延伸阅读

- [crates/core —— 核心库文档（中文）](./crates/core/README.zh.md)
- [crates/macros —— 过程宏文档（中文）](./crates/macros/README.zh.md)
- [与 MEDI 的差异清单（scope 校验、dispose、keyed、get_all）](docs/rust-dix/01-introduction/medi-differences.md) —— .NET 迁移必读
- [参考章：当前版本语义 / 错误目录 / 生产检查清单](docs/rust-dix/09-reference/INDEX.md)
- [扩展契约与边界（宏 key From / 自定义 resolver / async AnyKey）](https://github.com/Rust-Framework/rust-dix/blob/main/docs/rust-dix/09-reference/extension-points.md)
- [业务场景教程](docs/rust-dix/08-best-practices/business-scenarios.md) —— 真实模式
- [安全策略](./SECURITY.md) —— 漏洞上报、支持版本与安全边界
- [变更日志](./CHANGELOG.md) —— 发布历史
- [集成测试](crates/core/tests/) —— 真实用法范例

## 许可证

MIT。