<p align="center">
  <img src="../assets/logo.svg" alt="LRDI" width="80">
</p>

<h1 align="center">lrdi · Rust 依赖注入容器</h1>

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

`lrdi` is the core crate of the LRDI framework — a Rust dependency injection
container inspired by Microsoft.Extensions.DependencyInjection (MEDI).

```toml
[dependencies]
lrdi = "0.1"
```

---

### When should you use this?

**1. You want to decouple service creation from usage**

Instead of `MyService::new(dep1, dep2, dep3)` scattered across your codebase,
you declare once how services are constructed and let the container wire them
up automatically.

```rust
// Without DI: every call site must know how to build everything
let svc = MyService::new(
    Arc::new(Logger::new("app")),
    Arc::new(Cache::new(1024)),
);

// With DI: declare once, resolve anywhere
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger::new("app")))
    .singleton(|_| Arc::new(Cache::new(1024)))
    .transient(|p| Arc::new(MyService::new(p.get(), p.get())))
    .build()
    .unwrap();

let svc: Arc<MyService> = provider.get();
```

**2. You need to swap implementations for testing**

Change one registration line — no need to touch the code that consumes the
service.

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

**3. You have objects with different lifetimes**

Some services should be shared globally (database pool), some per-request
(HTTP context), some created fresh each time (value objects). LRDI gives you
three lifetimes to model this naturally.

| Lifetime | Behavior | Typical use |
|----------|----------|-------------|
| **Singleton** | Created once, shared everywhere | Database pool, config, event bus |
| **Scoped** | Created once per scope, shared within scope | HTTP request context, unit of work, transaction |
| **Transient** | Created every time you resolve it | Value objects, DTOs, lightweight stateless services |

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))           // one pool for the app
    .scoped(|_| Arc::new(RequestContext::new()))     // one per request
    .transient(|_| Arc::new(Query::new()))           // new each time
    .build()
    .unwrap();
```

**4. You want multiple instances of the same type**

Keyed services let you register several implementations of the same trait,
each distinguished by a string key.

```rust
let provider = ServiceCollection::new()
    .keyed::<dyn Strategy>("fast", |_| Arc::new(FastPath))
    .keyed::<dyn Strategy>("safe", |_| Arc::new(SafePath))
    .build()
    .unwrap();

let fast: Arc<dyn Strategy> = provider.get_keyed("fast");
let safe: Arc<dyn Strategy> = provider.get_keyed("safe");
```

**5. You are building a plugin / modular system**

- **Layered containers**: child-first, root-fallback resolution for plugin
  isolation. The plugin sees its own services first; host services are visible
  as fallback.
- **Named services**: register services by string name — critical for cdylib
  plugins where Rust's `TypeId` differs across compilation units.
- **`IServiceLocator`**: pass a unified DI interface to external modules that
  don't depend on LRDI directly.

---

### Quick Reference · API

#### Registration (`ServiceCollection`)

| Method | Lifetime | Use when |
|--------|----------|----------|
| `.singleton(f)` | Singleton | You need exactly one instance |
| `.scoped(f)` | Scoped | You need one instance per scope |
| `.transient(f)` | Transient | You need a new instance every time |
| `.keyed(k, f)` | Singleton | You need multiple named instances |
| `.keyed_scoped(k, f)` | Scoped | Multiple named scoped instances |
| `.keyed_transient(k, f)` | Transient | Multiple named transient instances |
| `.instance(arc)` | Singleton | You already have a built `Arc<T>` |
| `.try_add(f)` | Singleton | Only register if not already present |
| `.singleton_value(v)` | Singleton | Register a plain value (wraps in `Arc`) |
| `.add(lt, f)` | *any* | Specify lifetime explicitly |

The factory closure `f` receives `&dyn IServiceResolver` so you can resolve
dependencies:

```rust
ServiceCollection::new()
    .singleton(|_| Arc::new(Pool::new()))
    .transient(|p| Arc::new(Repo::new(p.get::<Pool>())))
    .build()
    .unwrap();
```

#### Resolution (`ServiceProvider` / `Scope`)

| Method | Returns | Behavior |
|--------|---------|----------|
| `.get::<T>()` | `Arc<T>` | Panics if not registered |
| `.get_service::<T>()` | `Option<Arc<T>>` | Returns `None` if not registered |
| `.get_required_service::<T>()` | `Arc<T>` | Panics with descriptive message |
| `.get_keyed::<T>(key)` | `Arc<T>` | Panics if key not found |
| `.try_get_keyed::<T>(key)` | `Option<Arc<T>>` | Returns `None` if key not found |
| `.get_all::<T>()` | `Vec<Arc<T>>` | All registered instances (keyed + unkeyed) |
| `.get_named::<T>(name)` | `Arc<T>` | Named resolution (cross-DLL) |
| `.get_named_any::<T>(name)` | `Option<Arc<T>>` | Named, returns `None` if missing |
| `.create_scope()` | `Scope` | New scope for scoped services |

`T` can be a concrete type or `dyn Trait`:

```rust
let svc: Arc<MyService> = provider.get();
let plugin: Arc<dyn IPlugin> = provider.get();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

---

### Flexible Application Patterns

#### 🔹 Three-layered architecture (Controller → Service → Repository)

```rust
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(DbPool::new(&config)))
    .transient(|p| Arc::new(UserRepo::new(p.get::<DbPool>())))
    .transient(|p| Arc::new(UserService::new(p.get::<UserRepo>())))
    .transient(|p| Arc::new(UserController::new(p.get::<UserService>())))
    .build()
    .unwrap();
```

#### 🔹 Strategy pattern with keyed services

```rust
let provider = ServiceCollection::new()
    .keyed::<dyn PaymentGateway>("credit", |_| Arc::new(CreditCardGateway))
    .keyed::<dyn PaymentGateway>("alipay", |_| Arc::new(AlipayGateway))
    .keyed::<dyn PaymentGateway>("wechat", |_| Arc::new(WechatGateway))
    .build()
    .unwrap();

fn checkout(provider: &ServiceProvider, method: &str) {
    let gateway: Arc<dyn PaymentGateway> = provider.get_keyed(method);
    gateway.charge(100);
}
```

#### 🔹 Scoped per-request (web server)

```rust
fn handle_request(container: &ServiceProvider) {
    let scope = container.create_scope();
    let ctx: Arc<RequestContext> = scope.get();
    let svc: Arc<MyService> = scope.get();
    // ctx and MyService share the same scope — scoped services are cached
    // Drops when scope goes out of scope
}
```

#### 🔹 Plugin isolation with ServiceProviderWrapper

```rust
let host_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(HostService::new()))
    .build().unwrap();

let plugin_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(PluginService::new()))
    .build().unwrap();

let wrapper = ServiceProviderWrapper::new(plugin_provider, host_provider);
// PluginService resolved from plugin container
// HostService falls back to host container
```

#### 🔹 Cross-DLL plugin with named services

```rust
// Host process
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(EventBus::new()))
    .build().unwrap();
provider.register_named("event_bus", provider.get::<EventBus>());

// Plugin loaded from cdylib (separate compilation unit)
let bus = host.get_named::<EventBus>("event_bus");
```

#### 🔹 External system integration via IServiceLocator

```rust
let locator: Arc<dyn IServiceLocator> = Arc::new(ServiceLocatorBridge::new(
    Arc::new(RdiProvider::Root(provider)),
));
// Pass to third-party code or FFI boundary
```

---

### Architecture

```
┌──────────────────────────────────────────────┐
│               ServiceCollection               │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │singleton │ │  scoped  │ │  transient   │ │
│  │ .keyed() │ │.keyed_*()│ │ .instance()  │ │
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

### Relationship with lrdi-macros

`lrdi` works with or without [`lrdi-macros`](../lrdi-macros/). The macros
provide two compile-time conveniences:

- **`#[derive(Inject)]`** — Generate the factory function automatically from
  struct fields, eliminating boilerplate when defining services.
- **`#[lrdi::module]`** — Collect `lrdi::inject!()` declarations at compile
  time and generate a complete provider builder, eliminating manual
  `ServiceCollection` chaining.

See [`lrdi-macros/README.md`](../lrdi-macros/README.md) for details.

---

### License

MIT.

---

<h2 id="chinese">中文</h2>

`lrdi` 是 LRDI 框架的核心 crate — 一个受 Microsoft.Extensions.DependencyInjection
(MEDI) 启发的 Rust 依赖注入容器。

```toml
[dependencies]
lrdi = "0.1"
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
有些应该每次都是新的（值对象）。LRDI 提供了三种生命周期来自然建模。

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

键控服务让你为同一个 trait 注册多个实现，用字符串键区分。

```rust
let provider = ServiceCollection::new()
    .keyed::<dyn Strategy>("fast", |_| Arc::new(FastPath))
    .keyed::<dyn Strategy>("safe", |_| Arc::new(SafePath))
    .build()
    .unwrap();

let fast: Arc<dyn Strategy> = provider.get_keyed("fast");
let safe: Arc<dyn Strategy> = provider.get_keyed("safe");
```

**5. 你在构建插件 / 模块化系统**

- **分层容器**：子容器优先、根容器回退的解析策略，用于插件隔离。
  插件自己的服务优先可见，宿主服务作为回退。
- **命名服务**：通过字符串名称注册服务 — 对于 cdylib 插件至关重要，
  因为 Rust 的 `TypeId` 跨编译单元不一致。
- **`IServiceLocator`**：将统一的 DI 接口传递给不直接依赖 LRDI 的外部模块。

---

### 快速参考 · API

#### 注册（`ServiceCollection`）

| 方法 | 生命周期 | 使用场景 |
|--------|----------|---------|
| `.singleton(f)` | Singleton | 全局只需要一个实例 |
| `.scoped(f)` | Scoped | 每个作用域一个实例 |
| `.transient(f)` | Transient | 每次解析都新建 |
| `.keyed(k, f)` | Singleton | 同类型多个命名实例 |
| `.keyed_scoped(k, f)` | Scoped | 多个命名 scoped 实例 |
| `.keyed_transient(k, f)` | Transient | 多个命名 transient 实例 |
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
| `.create_scope()` | `Scope` | 新建作用域 |

`T` 支持具体类型和 trait object：

```rust
let svc: Arc<MyService> = provider.get();
let plugin: Arc<dyn IPlugin> = provider.get();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

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

#### 🔹 策略模式 + 键控服务

```rust
let provider = ServiceCollection::new()
    .keyed::<dyn PaymentGateway>("credit", |_| Arc::new(CreditCardGateway))
    .keyed::<dyn PaymentGateway>("alipay", |_| Arc::new(AlipayGateway))
    .keyed::<dyn PaymentGateway>("wechat", |_| Arc::new(WechatGateway))
    .build()
    .unwrap();

fn checkout(provider: &ServiceProvider, method: &str) {
    let gateway: Arc<dyn PaymentGateway> = provider.get_keyed(method);
    gateway.charge(100);
}
```

增加新的支付方式只需注册一个新 key，无需修改 checkout 函数。

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

#### 🔹 通过 IServiceLocator 集成外部系统

```rust
let locator: Arc<dyn IServiceLocator> = Arc::new(ServiceLocatorBridge::new(
    Arc::new(RdiProvider::Root(provider)),
));
// 传递给第三方代码或 FFI 边界
```

---

### 架构

```
┌──────────────────────────────────────────────┐
│               ServiceCollection               │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │
│  │singleton │ │  scoped  │ │  transient   │ │
│  │ .keyed() │ │.keyed_*()│ │ .instance()  │ │
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

### 与 lrdi-macros 的关系

`lrdi` 可以独立使用，也可以配合 [`lrdi-macros`](../lrdi-macros/) 使用。
过程宏提供两个编译期便利：

- **`#[derive(Inject)]`** — 从结构体字段自动生成工厂函数，消除定义服务时的样板代码。
- **`#[lrdi::module]`** — 编译期收集 `lrdi::inject!()` 声明，生成完整的 provider
  构建函数，消除手动拼接 `ServiceCollection` 调用链。

详见 [`lrdi-macros/README.md`](../lrdi-macros/README.md)。

---

### 许可

MIT。
