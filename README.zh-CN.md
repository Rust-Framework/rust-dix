<p align="center">
  <img src="assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix · Rust 依赖注入框架</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

**rust-dix** 是一个 Rust 依赖注入框架，灵感来自
Microsoft.Extensions.DependencyInjection (MEDI)。它提供了带有构造函数注入、
作用域生命周期（Scoped）、异步初始化和编译期校验的服务容器 —— 全程零 `unsafe` 代码。

## Crate 总览

| Crate | 作用 | 适用人群 |
|-------|------|----------|
| [`rust-dix`](./crates/core/) | 核心 DI 容器 —— 注册、构建、解析、作用域 | 任何希望实现松耦合、可测试和可插拔架构的 Rust 应用 |
| [`rust-dix-macros`](./crates/macros/) | 过程宏 —— `#[derive(Inject)]`、`#[rust_dix::inject]`、`#[rust_dix::module]` | 希望用编译期代码生成替代手工装配的 `rust-dix` 使用者 |

## 快速开始

```toml
[dependencies]
rust-dix = "0.7"
```

### 同步应用

```rust
use rust_dix::*;
use std::sync::Arc;

#[rust_dix::inject(singleton)]
struct Config;

#[rust_dix::inject(transient)]
struct Worker { config: Arc<Config> }

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
| **键控服务** | 同一 trait 可通过字符串键注册多个实现 |
| **异步支持** | `build_async()` + `async_*` 注册，用于需要异步初始化的服务 |
| **构建期校验** | 循环依赖检测、受控依赖（captive dependency）拒绝 |
| **容器注入** | `#[inject(provider)]`、`Arc<dyn ScopeFactory>`、`Arc<ServiceProvider>` —— eager singleton 与懒解析服务均可用 |
| **跨 DLL 支持** | 面向 cdylib 插件系统的具名服务注册表 |
| **分层容器** | 通过 `ServiceProviderWrapper` 实现子容器优先解析 |
| **拥有式解析** | `get_owned::<T>()` 为 `&mut self` 返回拥有型 `T` —— 无需内部可变性 |
| **作用域清理** | 清理闭包 + 自动 `Drop`，用于作用域资源释放 |

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

// 注册同一 trait 的三个实现
let provider = ServiceCollection::new()
    .singleton::<dyn INotificationChannel>(|_| Arc::new(EmailChannel))
    .keyed_singleton::<dyn INotificationChannel>("sms", |_| Arc::new(SmsChannel))
    .keyed_singleton::<dyn INotificationChannel>("push", |_| Arc::new(PushChannel))
    .build()
    .unwrap();
```

**消费端：**

```rust
// 获取默认（无键）实现
let default: Arc<dyn INotificationChannel> = provider.get().unwrap();

// 获取特定的具名实现
let sms: Arc<dyn INotificationChannel> = provider.get_keyed("sms").unwrap();

// 一次性获取全部实现（无键 + 所有有键）
let all: Vec<Arc<dyn INotificationChannel>> = provider.get_all();

// 向每个通道广播消息
for channel in &all {
    channel.send("系统告警：磁盘使用率 > 90%");
}
```

### 键控服务：运行时策略路由

键控服务让你可以在运行时按名称选择实现 —— 这正是策略（Strategy）模式的基础。

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

容器注入在 **`build()` 期间**同样成立：eager singleton 可以持有 provider 或内置
scope factory。singleton worker 推荐使用弱引用的 `ScopeFactory` —— 每个任务创建
独立 scope，同时不会因为 `Arc` 环让容器无法回收：

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

作用域服务每个作用域创建一次，并在其中缓存。根
`ServiceProvider` 本身即充当根作用域 —— 如需请求级隔离，请使用子作用域。

**注册端：**

```rust
pub trait IUnitOfWork: Send + Sync {
    fn commit(&self) -> Result<(), String>;
    fn rollback(&self);
}

let provider = ServiceCollection::new()
    .singleton::<dyn IDbPool>(|_| Arc::new(DbPool::new()))   // 全局
    .scoped::<dyn IUnitOfWork>(|r| {                          // 每作用域
        let db: Arc<dyn IDbPool> = r.get().unwrap();
        Arc::new(UnitOfWork::new(db))
    })
    .transient::<dyn IOrderService>(|r| {                     // 每次都新建
        let uow: Arc<dyn IUnitOfWork> = r.get().unwrap();
        Arc::new(OrderService::new(uow))
    })
    .build()
    .unwrap();
```

**消费端：**

```rust
// ── 根作用域：UoW 被缓存，始终返回同一实例 ──
let uow1: Arc<dyn IUnitOfWork> = provider.get().unwrap();
let uow2: Arc<dyn IUnitOfWork> = provider.get().unwrap();
// uow1 和 uow2 是同一实例（根作用域缓存）

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
// OrderService 的 IUnitOfWork 是 scope_a 的 UoW，而非根作用域的
```

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

- [crates/core —— 核心库文档](./crates/core/README.md)
- [crates/macros —— 过程宏文档](./crates/macros/README.md)
- [业务场景教程](docs/rust-dix/08-best-practices/business-scenarios.md) —— 真实模式
- [集成测试](crates/core/tests/) —— 真实用法范例

## 许可证

MIT。