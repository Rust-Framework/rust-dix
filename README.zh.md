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
  <a href="https://gitcode.com/rf2026/rust-dix/blob/main/README.md"><strong>English</strong></a>
</p>

## 中文

**rust-dix** 是一个 Cargo 工作空间（workspace），提供一个受
Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架。

### 工作空间包含的 crate

| Crate | 功能 | 适用人群 |
|-------|------|---------|
| [`rust-dix`](./crates/core/) | 核心 DI 容器 — 注册、构建、解析、作用域 | 任何希望解耦、可测试、可插拔架构的 Rust 应用 |
| [`rust-dix-macros`](./crates/macros/) | 过程宏 — `#[derive(Inject)]`、`#[rust_dix::inject]`、`#[rust_dix::module]` | 使用 `rust-dix` 并希望编译期自动生成代码的用户 |

### 快速开始

```toml
[dependencies]
rust-dix = "0.6"
```

```rust
use rust_dix::*;
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

### 什么时候用 rust-dix？

- **你在构建中等规模以上的 Rust 应用**，手动拼接依赖链变得繁琐且易出错。
- **你需要可测试性** — 改一行注册代码就能用 mock 替换真实实现。
- **你在设计插件系统** — 命名服务和分层容器让宿主和插件各自拥有独立的 DI 容器。
- **你来自 .NET 生态**，希望在 Rust 中使用同样的 `IServiceCollection` / `IServiceProvider` 模式。

### 面向接口编程

rust-dix 的核心原则：**依赖 trait，而非具体类型**。这使得测试中可以替换 mock，增加新实现无需修改消费方。

#### 为什么面向接口

| 不面向接口 | 面向接口 |
|-----------|---------|
| `Arc<WechatPay>` — 锁定单一实现 | `Arc<dyn IPaymentGateway>` — 任意实现 |
| 测试无法替换支付网关 | Mock 替换为 `MockPaymentGateway` |
| 增加支付宝需修改所有消费方 | 只需注册新实现 |

#### 多态：同一 trait 的多个实现

同一 trait 注册多个实现，然后一次性获取全部。

**注册端：**

```rust
use rust_dix::*;
use std::sync::Arc;

pub trait INotificationChannel: Send + Sync {
    fn send(&self, msg: &str);
}

// 同一 trait 注册三个实现
let provider = ServiceCollection::new()
    .singleton::<dyn INotificationChannel>(|_| Arc::new(EmailChannel))
    .keyed_singleton::<dyn INotificationChannel>("sms", |_| Arc::new(SmsChannel))
    .keyed_singleton::<dyn INotificationChannel>("push", |_| Arc::new(PushChannel))
    .build()
    .unwrap();
```

**消费端：**

```rust
// 获取默认（无 key）实现
let default: Arc<dyn INotificationChannel> = provider.get().unwrap();

// 获取指定名称的实现
let sms: Arc<dyn INotificationChannel> = provider.get_keyed("sms").unwrap();

// 一次性获取全部实现（无 key + 所有 keyed）
let all: Vec<Arc<dyn INotificationChannel>> = provider.get_all();

// 向所有渠道广播消息
for channel in &all {
    channel.send("系统告警：磁盘使用率 > 90%");
}
```

#### 键控服务：运行时策略路由

键控服务允许按名称在运行时选择实现——策略模式的基础。

**注册端：**

```rust
pub trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> Result<String, String>;
}

let provider = ServiceCollection::new()
    // 默认（无 key）——用作回退
    .singleton::<dyn IPaymentGateway>(|_| Arc::new(WechatPay))
    // 命名替代方案
    .keyed_singleton::<dyn IPaymentGateway>("alipay", |_| Arc::new(Alipay))
    .keyed_transient::<dyn IPaymentGateway>("credit_card", |_| Arc::new(CreditCardPay))
    .build()
    .unwrap();
```

**消费端：**

```rust
// ── 模式 A：静态分发（编译期已知 key）──
let gateway: Arc<dyn IPaymentGateway> = provider.get_keyed("alipay").unwrap();
gateway.charge(100)?;

// ── 模式 B：动态路由（运行时从请求中获取 key）──
fn process_payment(provider: &ServiceProvider, method: &str, amount: u64) {
    let gateway: Arc<dyn IPaymentGateway> = provider
        .try_get_keyed(method)                          // 返回 Option
        .unwrap_or_else(|| provider.get().unwrap());    // 回退到默认
    gateway.charge(amount).unwrap();
}

// ── 模式 C：注入 Provider 实现延迟路由 ──
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

#### 作用域服务：请求级隔离

Scoped 服务在每个 Scope 内创建一次并缓存。根 `ServiceProvider` 本身就是根 Scope——需要请求级隔离时使用子 Scope。

**注册端：**

```rust
pub trait IUnitOfWork: Send + Sync {
    fn commit(&self) -> Result<(), String>;
    fn rollback(&self);
}

let provider = ServiceCollection::new()
    .singleton::<dyn IDbPool>(|_| Arc::new(DbPool::new()))   // 全局共享
    .scoped::<dyn IUnitOfWork>(|r| {                          // 每个 Scope 独立
        let db: Arc<dyn IDbPool> = r.get().unwrap();
        Arc::new(UnitOfWork::new(db))
    })
    .transient::<dyn IOrderService>(|r| {                     // 每次新建
        let uow: Arc<dyn IUnitOfWork> = r.get().unwrap();
        Arc::new(OrderService::new(uow))
    })
    .build()
    .unwrap();
```

**消费端：**

```rust
// ── 根 Scope：UoW 被缓存，始终返回同一实例 ──
let uow1: Arc<dyn IUnitOfWork> = provider.get().unwrap();
let uow2: Arc<dyn IUnitOfWork> = provider.get().unwrap();
// uow1 与 uow2 是同一实例（根 Scope 缓存）

// ── 子 Scope：每个 Scope 拥有独立的 UoW 实例 ──
let scope_a = provider.scope();
let scope_b = provider.scope();

let uow_a: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
let uow_b: Arc<dyn IUnitOfWork> = scope_b.get().unwrap();
// uow_a != uow_b（不同 Scope，不同实例）

// 同一子 Scope 内，Scoped 被缓存：
let uow_a2: Arc<dyn IUnitOfWork> = scope_a.get().unwrap();
// uow_a == uow_a2（同一 Scope，缓存复用）

// ── 从 Scope 解析 Transient：Scoped 依赖绑定到该 Scope ──
let order_svc: Arc<dyn IOrderService> = scope_a.get().unwrap();
// OrderService 的 IUnitOfWork 是 scope_a 的 UoW，不是根的
```

**关键规则**：Singleton 不得依赖 Scoped（构建期强制拒绝）。Scoped 可依赖 Singleton。Transient 可依赖任何生命周期。

详细模式（合约层、分层架构、插件系统）见 [业务场景实战](docs/rust-dix/08-best-practices/business-scenarios.md)。

### 最佳实践

#### 生命周期选择

| 生命周期 | 使用场景 | 示例 |
|---------|---------|------|
| **Singleton** | 无状态服务、共享配置、连接池 | `AppConfig`、`DbPool`、`Logger` |
| **Scoped** | 请求级状态、工作单元对象 | `DbContext`、`UnitOfWork` |
| **Transient** | 每次新建的轻量级无状态服务 | `OrderService`、`Validator` |

**规则**：Singleton 不得依赖 Scoped（构建期强制拒绝）。Scoped 可依赖 Singleton。Transient 可依赖任何生命周期。

#### 同步 vs 异步注册

```
需要异步初始化（DB、网络、I/O）？
├── 是 → 用 build_async() + async_* 注册
│   ├── 全局共享 → async_singleton
│   ├── 请求 Scope → async_scoped
│   └── 每次新建 → async_transient
└── 否 → 用 build() + 同步注册
    ├── 全局共享 → singleton
    ├── 请求 Scope → scoped
    └── 每次新建 → transient
```

**重要**：一旦使用任何 `async_*` 注册，必须调用 `build_async()` 而非 `build()`。

#### Scope 生命周期（请求级模式）

```rust
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async { Arc::new(DbPool { ... }) }))
    .scoped(|r| { Arc::new(UserRepository { ... }) })
    .build_async().await.unwrap();

fn handle_request(provider: &Arc<ServiceProvider>) {
    let scope = provider.scope();           // 创建隔离 Scope
    let repo = scope.get::<UserRepository>().unwrap();
    // ... 处理请求 ...
    // scope 离开作用域 → scoped 实例释放
}
```

#### 错误处理

- 可选依赖用 `try_get::<T>()` / `get_optional::<T>()`（返回 `Option`）
- 必选依赖用 `get::<T>()`（返回 `Result`）——缺失是可恢复错误，不是 panic

### 真实案例：Web 应用

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
        .async_singleton(|_| Box::pin(async {
            Arc::new(DbPool { conn_str: "pg://localhost".into() })
        }))
        .async_singleton(|_| Box::pin(async {
            Arc::new(RedisCache { url: "redis://localhost".into() })
        }))
        .async_keyed_singleton::<dyn IPaymentGateway>("wechat", |_| {
            Box::pin(async { Arc::new(WechatPay { ... }) as Arc<dyn IPaymentGateway> })
        })
        .singleton(|_| Arc::new(AppConfig { max_retries: 3 }))
        .scoped(|r| { Arc::new(UserRepository { ... }) })
        .transient(|r| { Arc::new(OrderService { ... }) })
        .build_async()
        .await
        .unwrap();

    let scope = provider.scope();
    let order_svc: Arc<OrderService> = scope.get().unwrap();
}
```

可运行完整示例：[`async_integration.rs`](crates/core/tests/async_integration.rs)

### 扩展阅读

- [crates/core — 主库完整文档](./crates/core/README.md)
- [crates/macros — 过程宏文档](./crates/macros/README.md)
- [业务场景实战](docs/rust-dix/08-best-practices/business-scenarios.md) — 真实业务模式
- [集成测试](crates/core/tests/) — 实际用法参考

---

## License

MIT.
