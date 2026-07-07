<p align="center">
  <img src="../assets/logo.svg" alt="rust-dicore" width="80">
</p>

<h1 align="center">rust-dicore · Rust 依赖注入容器</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dicore"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dicore.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dicore"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dicore?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
</p>

`rust-dicore` 是 rust-dicore 框架的核心 crate — 一个受 Microsoft.Extensions.DependencyInjection
(MEDI) 启发的 Rust 依赖注入容器。

```toml
[dependencies]
rust-dicore = "0.4"
```

---

### 什么时候用这个库？

**1. 你希望将服务的创建与使用解耦**

不需要在代码中到处写 `MyService::new(dep1, dep2, dep3)`，只需声明一次构建方式，
容器自动完成依赖装配。

```rust
// 不用 DI：每个调用方都必须知道如何构建所有依赖
let svc = MyService::new(
    Arc::new(Logger::new("app")),
    Arc::new(Cache::new(1024)),
);

// 用 DI：声明一次，随处解析
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger::new("app")))
    .singleton(|_| Arc::new(Cache::new(1024)))
    .transient(|p| Arc::new(MyService::new(p.get(), p.get())))
    .build()
    .unwrap();

let svc: Arc<MyService> = provider.get();
```

**2. 你需要为测试替换实现**

改一行注册代码，不用动消费服务的代码。

```rust
#[cfg(test)]
fn test_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MockDatabase::new()))
        .transient(|p| Arc::new(MyApi::new(p.get())))
        .build()
        .unwrap()
}
```

**3. 你的对象有不同的生命周期**

有些服务应该全局共享（数据库连接池），有些应该每次请求一个（HTTP 上下文），
有些应该每次都是新的（值对象）。rust-dicore 提供了三种生命周期来自然建模。

| 生命周期 | 行为 | 典型场景 |
|----------|------|---------|
| **Singleton** | 创建一次，全局共享 | 数据库连接池、配置、事件总线 |
| **Scoped** | 每个作用域创建一次，作用域内共享 | HTTP 请求上下文、工作单元、事务 |
| **Transient** | 每次解析都创建新实例 | 值对象、DTO、轻量无状态服务 |

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))           // 整个应用一个连接池
    .scoped(|_| Arc::new(RequestContext::new()))     // 每次请求一个上下文
    .transient(|_| Arc::new(Query::new()))           // 每次都新建
    .build()
    .unwrap();
```

**4. 你需要同类型的多个实例**

键控服务让你为同一个 trait 注册多个实现，用字符串键区分。支持所有三种生命周期：

```rust
// 键控服务支持三种生命周期
let provider = ServiceCollection::new()
    // Singleton keyed - 全局共享
    .keyed_singleton::<dyn Strategy>("fast", |_| Arc::new(FastPath))
    // Scoped keyed - Scope 内共享
    .keyed_scoped::<dyn Strategy>("session", |_| Arc::new(SessionStrategy))
    // Transient keyed - 每次新建
    .keyed_transient::<dyn Strategy>("fresh", |_| Arc::new(FreshStrategy))
    .build()
    .unwrap();

let fast: Arc<dyn Strategy> = provider.get_keyed("fast");
let session: Arc<dyn Strategy> = provider.create_scope().get_keyed("session");
let fresh1: Arc<dyn Strategy> = provider.get_keyed("fresh");
let fresh2: Arc<dyn Strategy> = provider.get_keyed("fresh");  // 不同实例
```

**5. 你在构建插件 / 模块化系统**

- **分层容器**：子容器优先、根容器回退的解析策略，用于插件隔离。
  插件自己的服务优先可见，宿主服务作为回退。
- **命名服务**：通过字符串名称注册服务 — 对于 cdylib 插件至关重要，
  因为 Rust 的 `TypeId` 跨编译单元不一致。
- **`IServiceLocator`**：将统一的 DI 接口传递给不直接依赖 rust-dicore 的外部模块。

---

### 快速参考 · API

#### 注册（`ServiceCollection`）

| 方法 | 生命周期 | 使用场景 |
|--------|----------|---------|
| `.from_injected()` | *混合* | 收集所有 `#[rust_dicore::inject]` 标注（推荐） |
| `.singleton(f)` | Singleton | 全局唯一实例（配置、连接池） |
| `.scoped(f)` | Scoped | 每个 Scope 一个实例（请求上下文、事务） |
| `.transient(f)` | Transient | 每次解析新建（轻量无状态服务） |
| `.keyed_singleton(k, f)` | Singleton | 策略模式命名实例 |
| `.keyed_scoped(k, f)` | Scoped | Scope 内策略实例 |
| `.keyed_transient(k, f)` | Transient | 每次新建策略实例 |
| `.instance(arc)` | Singleton | 已有构建好的 `Arc<T>` |
| `.try_add(f)` | Singleton | 只注册不存在的类型 |
| `.singleton_value(v)` | Singleton | 注册普通值（自动包 `Arc`） |
| `.add(lt, f)` | *任意* | 显式指定生命周期 |

工厂闭包 `f` 接收 `&dyn IServiceResolver`，可以解析依赖：

```rust
ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))
    .transient(|p| Arc::new(Repo::new(p.get::<Pool>())))
    .build()
    .unwrap();
```

#### `#[rust_dicore::inject]` — 基于属性宏的自动注册（推荐）

推荐的方式是直接在结构体（或 trait impl 块）上标注 `#[rust_dicore::inject(...)]`：

```rust
use rust_dicore::*;

// 标注结构体 → 注册为具体类型
// 生命周期参数省略时默认为 singleton
#[rust_dicore::inject]
struct Config { url: String }

#[rust_dicore::inject(transient)]
struct Worker { logger: Arc<Config> }

// 标注 trait impl → 注册为 `dyn Trait`
// trait 类型从 `impl Trait for Type` 自动推断，无需 `as = dyn Trait` 样板
trait UserRepo { /* ... */ }

// 面向接口的服务：struct 上用 #[derive(Inject)] 仅生成构造函数（不注册），
// impl 上用 #[inject] 注册为 trait。struct 与 impl 同时用 #[inject] 会双重注册。
#[derive(Inject)]
struct PgUserRepo { db: Arc<DbPool> }

#[rust_dicore::inject]              // 在 impl 上：trait 注册（默认 singleton）
impl UserRepo for PgUserRepo { /* ... */ }

// 一步收集所有标注并构建：
let provider = ServiceCollection::from_injected().build().unwrap();
```

`#[rust_dicore::inject]` 属性宏自动生成构造函数代码（类似 `#[derive(Inject)]`）
并通过 `inventory` 自动注册到容器。支持命名结构体和单元结构体（零字段）：

```rust
#[rust_dicore::inject]
struct RoleAuthorizer;  // 单元结构体 — 零字段

impl Default for RoleAuthorizer { fn default() -> Self { Self } }
```

**两个放置位置，统一语法：**

| 放置位置 | 注册为 | 是否复用构造函数 |
|---------|--------|----------------|
| `#[inject]` 在 struct 上 | 具体类型 `T` | 自行生成 |
| `#[inject]` 在 `impl Trait for T` 上 | `dyn Trait` | 复用 `__rdi_construct_T` |
| `#[derive(Inject)]` 在 struct 上 | *（不注册，仅生成构造函数）* | — |

**二选一，避免双重注册**：`#[inject]` 一般只放一处——struct 上或 impl 上，不要同时放。
两者同时用会让同一 struct 注册两次（具体类型 + `dyn Trait`），通常非预期——消费者能
绕过 trait 直接 `get::<具体类型>()`。面向接口的服务（Handler / Service / Repository）
推荐 struct 上用 `#[derive(Inject)]`（仅生成构造函数、不注册），impl 上用 `#[inject]`
注册为 trait。这取代了旧的 `as = dyn Trait` / `as = [dyn A, dyn B]` 语法。

#### 解析（`ServiceProvider` / `Scope`）

| 方法 | 返回 | 行为 |
|--------|---------|------|
| `.get::<T>()` | `Arc<T>` | 未注册则 panic |
| `.get_service::<T>()` | `Option<Arc<T>>` | 未注册返回 `None` |
| `.get_required_service::<T>()` | `Arc<T>` | 带描述信息的 panic |
| `.get_keyed::<T>(key)` | `Arc<T>` | 找不到 key 则 panic |
| `.try_get_keyed::<T>(key)` | `Option<Arc<T>>` | 找不到 key 返回 `None` |
| `.get_all::<T>()` | `Vec<Arc<T>>` | 所有已注册实例 |
| `.get_named::<T>(name)` | `Arc<T>` | 命名解析（跨 DLL） |
| `.get_named_any::<T>(name)` | `Option<Arc<T>>` | 命名解析，不 panic |
| `.get_owned::<T>()` | `T` | Owned 实例；Singleton 会 panic |
| `.try_get_owned::<T>()` | `Option<T>` | 未注册或 Singleton 返回 `None` |
| `.get_keyed_owned::<T>(key)` | `T` | Owned 键控实例；Singleton 会 panic |
| `.create_scope()` | `Scope` | 新建作用域 |

`T` 支持具体类型和 trait object：

```rust
let svc: Arc<MyService> = provider.get();
let plugin: Arc<dyn IPlugin> = provider.get();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

---

### Owned 服务解析（`&mut self` 无需 interior mutability）

`get::<T>()` 返回 `Arc<T>`，只能拿到 `&T`，无法调用 `&mut self` 方法。
对于需要可变访问的服务（如带 `save_changes(&mut self)` 的 `DbContext`），
用 `get_owned::<T>()` 获取 owned `T`，无需 `RwLock` + `unsafe`：

```rust
struct DbContext { count: u32 }
impl DbContext {
    fn add(&mut self) { self.count += 1; }
    fn total(&self) -> u32 { self.count }
}

let provider = ServiceCollection::new()
    .transient(|_| Arc::new(DbContext { count: 0 }))
    .build()
    .unwrap();

let mut ctx: DbContext = provider.get_owned();
ctx.add();                 // ✓ &mut self — 无锁无 unsafe
assert_eq!(ctx.total(), 1);
```

| 生命周期 | `get_owned` 行为 |
|----------|----------------|
| **Transient** | 每次调用新建实例 ✓ |
| **Scoped** | 绕过缓存，每次调用新建 ✓ |
| **Singleton** | panic（共享实例无法 owned）；用 `try_get_owned` 返回 `None` |

**配合 `#[derive(Inject)]`**：裸 `T` 字段用 `#[inject(owned)]` 标记按 owned 解析；
`Arc<T>` 字段用 `#[inject]` 保持共享。`Option<T>` → `try_get_owned`，
`Option<Arc<T>>` → `try_get`：

```rust
#[derive(Inject)]
struct Handler {
    #[inject(owned)]
    ctx: DbContext,           // owned  → get_owned
    #[inject]
    logger: Arc<Logger>,      // shared → get
}
// ...
let mut h: Handler = provider.get_owned();
h.ctx.add();                 // ✓ owned 字段可变
```

**限制说明**：
- Singleton 不可 owned —— `get_owned` panic，`try_get_owned` 返回 `None`。
- Scoped 的 `get_owned` 绕过作用域缓存（每次 fresh）。这是有意为之：
  owned `&mut self` 与 scope 共享语义不兼容。
- Trait bound 保持 `T: Send + Sync + 'static`（复用现有工厂，无独立 owned 工厂路径）。

---

### 在哪些场景下灵活应用？

#### 🔹 三层架构（Controller → Service → Repository）

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(DbPool::new(&config)))
    .transient(|p| Arc::new(UserRepo::new(p.get::<DbPool>())))
    .transient(|p| Arc::new(UserService::new(p.get::<UserRepo>())))
    .transient(|p| Arc::new(UserController::new(p.get::<UserService>())))
    .build()
    .unwrap();
```

每层依赖由容器自动注入，单元测试时可以逐层替换 mock。

#### 🔹 策略模式 + 键控服务（面向接口开发）

```rust
// contracts 层定义接口
trait PaymentGateway: Send + Sync {
    fn charge(&self, amount: f64) -> Result<(), String>;
}

// handlers 层实现接口：struct 用 #[derive(Inject)] 仅生成构造函数，impl 用 #[inject] 注册为 trait
#[derive(Inject)]
struct CreditCardGateway;

#[rust_dicore::inject]  // 在 impl 上：注册为默认 dyn PaymentGateway
impl PaymentGateway for CreditCardGateway { /* ... */ }

// keyed 实现不能用 #[inject] 自动注册，在组合根手动注册
// （或用 #[rust_dicore::module]）
let provider = ServiceCollection::from_injected()
    .keyed_singleton::<dyn PaymentGateway>("alipay",  |_| Arc::new(AlipayGateway))
    .keyed_singleton::<dyn PaymentGateway>("wechat", |_| Arc::new(WechatGateway))
    .build()
    .unwrap();

// 消费方依赖接口，不依赖具体实现
#[rust_dicore::inject(transient)]
struct OrderService {
    #[inject(provider)]  // 按需解析策略
    provider: Arc<ServiceProvider>,
}

impl OrderService {
    fn checkout(&self, method: &str, amount: f64) {
        let gateway: Arc<dyn PaymentGateway> = self.resolver
            .try_get_keyed(method)
            .unwrap_or_else(|| self.resolver.get());  // 回退默认
        gateway.charge(amount);
    }
}
```

**关键点**：
- 接口定义在 contracts 层，实现消费接口
- 增加新支付方式只需注册新 key，无需修改 `OrderService`
- 符合开闭原则：对扩展开放，对修改关闭

#### 🔹 请求级别的 Scoped 生命周期（Web 服务）

```rust
fn handle_request(container: &ServiceProvider) {
    let scope = container.create_scope();
    let ctx: Arc<RequestContext> = scope.get();
    let svc: Arc<MyService> = scope.get();
    // ctx 和 MyService 共享同一个 scope，scoped 服务被缓存
    // scope 离开作用域时自动释放
}
```

适用于 Actix-web、Axum 等框架的每个请求上下文。

#### 🔹 插件隔离 — ServiceProviderWrapper

```rust
let host_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(HostService::new()))
    .build().unwrap();

let plugin_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(PluginService::new()))
    .build().unwrap();

let wrapper = ServiceProviderWrapper::new(plugin_provider, host_provider);
// PluginService 从插件容器解析
// HostService 回退到宿主容器
```

插件只能看到自己的服务，宿主的全局服务作为回退——天然的隔离边界。

#### 🔹 跨 DLL 插件的命名服务

```rust
// 宿主进程
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(EventBus::new()))
    .build().unwrap();
provider.register_named("event_bus", provider.get::<EventBus>());

// 插件（独立编译的 cdylib）
let bus = host.get_named::<EventBus>("event_bus");
```

跨编译单元时 `TypeId` 不同，不能用类型解析，命名服务是解决方案。

#### 🔹 通过 IProvider 集成外部系统

```rust
let provider: Arc<dyn IProvider> = provider as Arc<dyn IProvider>;
// 传递给第三方代码或 FFI 边界
```

---

### 架构

```
┌──────────────────────────────────────────────┐
│               ServiceCollection               │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │singleton │ │  scoped  │ │  transient   │ │
│  │.keyed_singleton│ │.keyed_*()│ │ .instance()  │ │
│  └──────────┘ └──────────┘ └──────────────┘ │
│                   │ .build()                  │
│                   ▼                           │
│              ServiceProvider                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │  .get()  │ │.get_keyed│ │ .create_scope│ │
│  │ .get_all │ │.get_named│ │              │ │
│  └──────────┘ └──────────┘ └──────┬───────┘ │
│                                    │         │
└────────────────────────────────────┼─────────┘
                                     │
                                     ▼
                               ┌──────────┐
                               │   Scope  │
                               │ .get()   │
                               │ scoped   │
                               │ cached   │
                               └──────────┘
```

---

### 与 rust-dicore-macros 的关系

`rust-dicore` 可以独立使用，也可以配合 [`rust-dicore-macros`](../macros/) 使用。
过程宏提供三个编译期便利：

- **`#[rust_dicore::inject(...)]`**（推荐）— 属性宏，同时完成构造函数生成 + 自动注册。
  只需在结构体（或 trait impl 块）上标注一行，`ServiceCollection::from_injected()` 自动收集所有注册。
- **`#[derive(Inject)]`** — 从结构体字段自动生成工厂函数，`#[rust_dicore::inject]` 内部使用。
- **`#[rust_dicore::module]`** — 编译期收集 `rust_dicore::register!()` 声明，生成完整的 provider
  构建函数。适用于外部类型注册、条件编译和集中式管理。

详见 [`macros/README.md`](../macros/README.md)。

---

### 许可

MIT。
