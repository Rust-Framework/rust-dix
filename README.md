# LRDI — Rust Dependency Injection Framework

LRDI 是一个面向接口的 Rust 依赖注入框架，受到 **Microsoft.Extensions.DependencyInjection** 启发，支持三种生命周期、键控服务、构造器注入和编译期模块扫描。

- `IServiceResolver` trait 统一了 `ServiceProvider`、`Scope`、`ServiceProviderWrapper` 的解析接口
- `#[derive(Inject)]` 自动生成构造器注入代码
- `#[lrdi::module]` 编译期收集模块内的服务注册声明

## 快速开始

```rust
use lrdi::*;
use std::sync::Arc;

struct Logger { prefix: String }
struct Worker { logger: Arc<Logger> }

let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger { prefix: "INFO".into() }))
    .transient(|p| Arc::new(Worker { logger: p.get() }))
    .build()
    .unwrap();

let logger: Arc<Logger> = provider.get();
```

## API 速览

### 注册服务 — ServiceCollection

`ServiceCollection` 是注册入口，类比 MEDI 的 `IServiceCollection`。链式调用，以 `.build()` 结尾。

| 方法 | 生命周期 | 说明 |
|------|----------|------|
| `.singleton(f)` | Singleton | 全局唯一实例 |
| `.scoped(f)` | Scoped | 同一 `Scope` 内复用 |
| `.transient(f)` | Transient | 每次解析创建新实例 |
| `.keyed(k, f)` | Singleton | 键控注册 |
| `.instance(arc)` | Singleton | 直接注入已构建的 `Arc<T>` |
| `.try_add(f)` | Singleton | 同类型未注册时才添加 |

工厂闭包接收 `&dyn IServiceResolver` 参数，可用于解析依赖。

### 解析服务 — ServiceProvider

`ServiceProvider` 是构建后的根容器，类比 MEDI 的 `IServiceProvider`。

| 方法 | 返回 | 未注册时 |
|------|------|----------|
| `.get::<T>()` | `Arc<T>` | panic |
| `.get_service::<T>()` | `Option<Arc<T>>` | 返回 `None` |
| `.get_required_service::<T>()` | `Arc<T>` | panic |
| `.get_keyed::<T>(key)` | `Arc<T>` | panic |
| `.get_all::<T>()` | `Vec<Arc<T>>` | 空数组 |
| `.get_services::<T>()` | `Vec<Arc<T>>` | 别名 |
| `.create_scope()` | `Scope` | — |

`T` 支持具体类型和 `dyn Trait`：

```rust
let svc: Arc<MyService> = provider.get();
let plugin: Arc<dyn IPlugin> = provider.get();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

### 作用域 — Scope / ServiceScope

`Scope` 由 `ServiceProvider::create_scope()` 创建，类比 MEDI 的 `IServiceScope`。`Scoped` 生命周期的服务在作用域内缓存复用，作用域销毁后缓存释放。

```rust
let scope = provider.create_scope();
let a = scope.get::<ScopedCounter>();
let b = scope.get::<ScopedCounter>();
assert!(Arc::ptr_eq(&a, &b));
```

### 三种生命周期

| 生命周期 | 行为 |
|----------|------|
| **Singleton** | 容器级别全局唯一，工厂仅执行一次 |
| **Scoped** | 每个 `Scope` 内唯一，不同作用域各自独立 |
| **Transient** | 每次解析创建新实例，不做缓存 |

## 构造器注入 — #[derive(Inject)]

通过派生宏自动生成构造函数，避免手动拼接依赖：

```rust
use lrdi::*;
use lrdi_macros::Inject;

#[derive(Inject)]
struct MyWorker {
    logger: Arc<Logger>,                    // 必选注入
    #[inject(optional)]
    cache: Option<Arc<Cache>>,              // 可选注入
    #[inject(key = "verbose")]
    tracer: Arc<Logger>,                    // 键控注入
    #[inject(provider)]
    resolver: Arc<dyn IServiceResolver>,     // 注入容器自身
    #[inject(skip)]
    name: String,                           // 跳过（用 Default）
}
```

| 属性 | 说明 |
|------|------|
| 无标记 | 必选注入，未注册则 panic |
| `#[inject(skip)]` | 跳过注入，使用 `Default::default()` |
| `#[inject(optional)]` | 可选注入，未注册返回 `None` |
| `#[inject(key = "...")]` | 键控注入 |
| `#[inject(provider)]` | 注入 `Arc<dyn IServiceResolver>` |

生成的构造函数签名为 `__lrdi_construct_MyWorker(resolver: &dyn IServiceResolver) -> Arc<MyWorker>`，可直接与 `ServiceProvider` 或 `Scope` 配合使用。

## 编译期模块扫描 — #[lrdi::module]

`#[lrdi::module]` 属性宏在编译期收集模块内的 `lrdi::inject!()` 声明，自动生成一个 `__lrdi_build_provider_{模块名}()` 函数，避免手工拼接 `ServiceCollection` 调用链。

```rust
#[lrdi::module]
mod services {
    lrdi::inject!(singleton: MyService);
    lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
    lrdi::inject!(keyed "verbose": singleton: Logger);
}

let provider = services::__lrdi_build_provider_services().unwrap();
```

支持 `trait => impl` 语法将 trait object 映射到具体实现：

```rust
lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
```

## 多容器桥接 — ServiceProviderWrapper

`ServiceProviderWrapper` 组合一个 child 和一个 root `ServiceProvider`，提供**子容器优先、根容器回退**的分层解析——适用于插件隔离场景。

```rust
let wrapper = ServiceProviderWrapper::new(child_provider, root_provider);
// 子容器可见，根容器不可见
let ps: Arc<PluginService> = wrapper.get();
// 根容器服务可回退
let hs: Arc<HostService> = wrapper.get();
```

## 跨 DLL 命名服务

由于 Rust 的 `TypeId` 跨编译单元不一致，`ServiceProvider` 提供基于字符串的命名服务注册，适用于 cdylib 插件场景。

```rust
// 主程序侧
provider.register_named("EventBus", event_bus.clone());
// 插件侧
let bus = provider.get_named::<EventBus>("EventBus");
```

## 插件 / 外部系统集成 — IServiceLocator / ServiceLocatorBridge

`IServiceLocator` trait 为外部系统提供了统一的 DI 访问接口。`ServiceLocatorBridge` 将 LRDI 的 provider 适配为 `Arc<dyn IServiceLocator>`，方便传递给不直接依赖 LRDI 的模块。

```rust
use lrdi::*;

let bridge = Arc::new(ServiceLocatorBridge::new(
    Arc::new(RdiProvider::Root(provider)),
));
let locator: Arc<dyn IServiceLocator> = bridge;
```
