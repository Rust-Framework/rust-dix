<!-- markdownlint-disable MD033 MD041 -->

<div align="center">

# LRDI

**Rust Dependency Injection Framework** · **Rust 依赖注入框架**

[![Crates.io][crates-badge]][crates-url]
[![MIT License][license-badge]][license-url]

[Documentation][docs-url] · [Crates.io][crates-url] · [Repository][repo-url]

</div>

---

**LRDI** is a Rust dependency injection framework inspired by
**Microsoft.Extensions.DependencyInjection (MEDI)**. It provides three service
lifetimes, keyed services, constructor injection via derive macro, compile-time
module scanning, and cross-DLL / plugin integration.

**LRDI** 是一个受 **Microsoft.Extensions.DependencyInjection** 启发的 Rust
依赖注入框架，支持三种生命周期、键控服务、派生宏构造器注入、编译期模块扫描以及跨 DLL / 插件集成。

---

## Quick Start · 快速开始

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

---

## Installation · 添加依赖

```toml
[dependencies]
lrdi = "0.1"
```

---

## Features · 功能特性

| English | 中文 |
|---------|------|
| Three lifetimes (Singleton, Scoped, Transient) | 三种生命周期（Singleton、Scoped、Transient） |
| Keyed services — multiple instances of the same type | 键控服务——同类型多实例 |
| `#[derive(Inject)]` — auto-generate constructor injection | `#[derive(Inject)]` 自动生成构造器注入 |
| `#[lrdi::module]` — compile-time module scanning | `#[lrdi::module]` 编译期模块扫描 |
| `ServiceProviderWrapper` — child-first layered container | `ServiceProviderWrapper` 子容器优先的分层容器 |
| Named service registry — cross-DLL / cdylib plugins | 命名服务注册——跨 DLL / cdylib 插件 |
| `IServiceLocator` / `ServiceLocatorBridge` — external system integration | `IServiceLocator` / `ServiceLocatorBridge` 外部系统集成 |

---

## API Overview · API 速览

### Registering Services · 注册服务

`ServiceCollection` is the entry point, analogous to MEDI's `IServiceCollection`.
Chain calls and finish with `.build()`.

`ServiceCollection` 是注册入口，类比 MEDI 的 `IServiceCollection`。链式调用，以 `.build()` 结尾。

| Method · 方法 | Lifetime · 生命周期 | Description · 说明 |
|---------------|----------------|-------------------|
| `.singleton(f)` | Singleton | Single instance globally · 全局唯一实例 |
| `.scoped(f)` | Scoped | Shared within one `Scope` · 同一 `Scope` 内复用 |
| `.transient(f)` | Transient | New instance every resolution · 每次解析创建新实例 |
| `.keyed(k, f)` | Singleton | Keyed registration · 键控注册 |
| `.instance(arc)` | Singleton | Inject a pre-built `Arc<T>` · 直接注入已构建的 `Arc<T>` |
| `.try_add(f)` | Singleton | Skip if type already registered · 同类型未注册时才添加 |

The factory closure receives `&dyn IServiceResolver`, which can be used to
resolve dependencies.

工厂闭包接收 `&dyn IServiceResolver` 参数，可用于解析依赖。

### Resolving Services · 解析服务

`ServiceProvider` is the built root container, analogous to MEDI's `IServiceProvider`.

`ServiceProvider` 是构建后的根容器，类比 MEDI 的 `IServiceProvider`。

| Method · 方法 | Returns · 返回 | On missing · 未注册时 |
|---------------|------------|-------------------|
| `.get::<T>()` | `Arc<T>` | panics |
| `.get_service::<T>()` | `Option<Arc<T>>` | returns `None` |
| `.get_required_service::<T>()` | `Arc<T>` | panics |
| `.get_keyed::<T>(key)` | `Arc<T>` | panics |
| `.get_all::<T>()` | `Vec<Arc<T>>` | empty vec · 空数组 |
| `.get_services::<T>()` | `Vec<Arc<T>>` | alias · 别名 |
| `.create_scope()` | `Scope` | — |

`T` can be a concrete type or `dyn Trait`:

`T` 支持具体类型和 `dyn Trait`：

```rust
let svc: Arc<MyService> = provider.get();
let plugin: Arc<dyn IPlugin> = provider.get();
let all: Vec<Arc<dyn IPlugin>> = provider.get_all();
```

### Scopes · 作用域

`Scope` is created by `ServiceProvider::create_scope()`, analogous to MEDI's
`IServiceScope`. Scoped services are cached within the same scope and released
when the scope is dropped.

`Scope` 由 `ServiceProvider::create_scope()` 创建，类比 MEDI 的 `IServiceScope`。
Scoped 生命周期的服务在作用域内缓存复用，作用域销毁后缓存释放。

```rust
let scope = provider.create_scope();
let a = scope.get::<ScopedCounter>();
let b = scope.get::<ScopedCounter>();
assert!(Arc::ptr_eq(&a, &b));
```

### Three Lifetimes · 三种生命周期

| Lifetime · 生命周期 | Behavior · 行为 |
|-----------------|-------------|
| **Singleton** | Single instance at container level, factory runs once · 容器级别全局唯一，工厂仅执行一次 |
| **Scoped** | Unique per `Scope`, independent across scopes · 每个 `Scope` 内唯一，不同作用域各自独立 |
| **Transient** | New instance each time, no caching · 每次解析创建新实例，不做缓存 |

---

## Constructor Injection · 构造器注入

The `#[derive(Inject)]` derive macro auto-generates a constructor function,
saving you from manually wiring up dependencies.

通过派生宏自动生成构造函数，避免手动拼接依赖：

```rust
use lrdi::*;
use lrdi_macros::Inject;

#[derive(Inject)]
struct MyWorker {
    logger: Arc<Logger>,                        // required · 必选注入
    #[inject(optional)]
    cache: Option<Arc<Cache>>,                  // optional · 可选注入
    #[inject(key = "verbose")]
    tracer: Arc<Logger>,                        // keyed · 键控注入
    #[inject(provider)]
    resolver: Arc<dyn IServiceResolver>,        // inject container itself · 注入容器自身
    #[inject(skip)]
    name: String,                               // skip, use Default · 跳过（用 Default）
}
```

| Attribute · 属性 | Description · 说明 |
|--------------|-------------------|
| (none · 无标记) | Required injection, panics if not registered · 必选注入，未注册则 panic |
| `#[inject(skip)]` | Skip injection, use `Default::default()` · 跳过注入，使用 `Default::default()` |
| `#[inject(optional)]` | Optional injection, returns `None` if not registered · 可选注入，未注册返回 `None` |
| `#[inject(key = "...")]` | Keyed injection · 键控注入 |
| `#[inject(provider)]` | Inject `Arc<dyn IServiceResolver>` · 注入容器自身 |

The generated constructor has the signature:

生成的构造函数签名为：

```rust
fn __lrdi_construct_MyWorker(resolver: &dyn IServiceResolver) -> Arc<MyWorker>
```

It works directly with `ServiceProvider` and `Scope`.

可直接与 `ServiceProvider` 或 `Scope` 配合使用。

---

## Compile-time Module Scanning · 编译期模块扫描

The `#[lrdi::module]` attribute macro collects `lrdi::inject!()` declarations
inside a module at compile time and auto-generates a
`__lrdi_build_provider_{module_name}()` function, removing the need to
manually chain `ServiceCollection` calls.

`#[lrdi::module]` 属性宏在编译期收集模块内的 `lrdi::inject!()` 声明，
自动生成一个 `__lrdi_build_provider_{模块名}()` 函数，避免手工拼接 `ServiceCollection` 调用链。

```rust
#[lrdi::module]
mod services {
    lrdi::inject!(singleton: MyService);
    lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
    lrdi::inject!(keyed "verbose": singleton: Logger);
}

let provider = services::__lrdi_build_provider_services().unwrap();
```

The `trait => impl` syntax maps a trait object to its implementation:

支持 `trait => impl` 语法将 trait object 映射到具体实现：

```rust
lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
```

---

## Layered Containers · 分层容器

`ServiceProviderWrapper` combines a child and a root `ServiceProvider`, providing
**child-first, root-fallback** resolution — useful for plugin isolation.

`ServiceProviderWrapper` 组合一个 child 和一个 root `ServiceProvider`，
提供**子容器优先、根容器回退**的分层解析——适用于插件隔离场景。

```rust
let wrapper = ServiceProviderWrapper::new(child_provider, root_provider);
let ps: Arc<PluginService> = wrapper.get();   // child first · 子容器优先
let hs: Arc<HostService> = wrapper.get();     // root fallback · 根容器回退
```

---

## Cross-DLL Named Services · 跨 DLL 命名服务

Rust's `TypeId` is not consistent across compilation units. `ServiceProvider`
provides string-keyed named service registration for cdylib plugin scenarios.

由于 Rust 的 `TypeId` 跨编译单元不一致，`ServiceProvider` 提供基于字符串的命名服务注册，
适用于 cdylib 插件场景。

```rust
// Host side · 主程序侧
provider.register_named("EventBus", event_bus.clone());

// Plugin side · 插件侧
let bus = provider.get_named::<EventBus>("EventBus");
```

---

## External System Integration · 外部系统集成

`IServiceLocator` provides a unified DI access interface for external systems.
`ServiceLocatorBridge` adapts LRDI's provider into an `Arc<dyn IServiceLocator>`,
making it easy to pass to modules that don't directly depend on LRDI.

`IServiceLocator` trait 为外部系统提供了统一的 DI 访问接口。
`ServiceLocatorBridge` 将 LRDI 的 provider 适配为 `Arc<dyn IServiceLocator>`，
方便传递给不直接依赖 LRDI 的模块。

```rust
use lrdi::*;

let bridge = Arc::new(ServiceLocatorBridge::new(
    Arc::new(RdiProvider::Root(provider)),
));
let locator: Arc<dyn IServiceLocator> = bridge;
```

---

## Crate Layout · 项目结构

```
lrdi/
├── Cargo.toml                         # Workspace manifest
├── crates/
│   ├── lrdi/                          # Main crate · 主 crate
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/                       # Source code
│   │   └── tests/                     # Integration tests
│   └── lrdi-macros/                   # Proc-macro crate
│       ├── Cargo.toml
│       └── src/
```

| Module · 模块 | Description · 说明 |
|------------|-------------------|
| [`ServiceCollection`] | Register services with a builder API |
| [`ServiceProvider`] | The root DI container |
| [`Scope`] / [`ServiceScope`] | Scoped container (one per scope) |
| [`IServiceResolver`] | Resolution trait (implemented by `ServiceProvider` & `Scope`) |
| [`ServiceProviderWrapper`] | Child-first layered container |
| [`ServiceLifetime`] | Enum: Singleton, Scoped, Transient |
| [`IServiceLocator`] / [`ServiceLocatorBridge`] | Cross-DLL / plugin integration |
| [`RdiError`] | Error types |

---

## Design Philosophy · 设计理念

- **MEDI-aligned API** — Familiar to developers coming from the .NET ecosystem.
  Key abstractions map directly: `IServiceCollection` → `ServiceCollection`,
  `IServiceProvider` → `ServiceProvider`, `IServiceScope` → `Scope`,
  `IServiceResolver` → resolution trait.

- **Zero-cost abstractions** — Proc-macros do their work at compile time;
  runtime resolution is a simple `Arc` lookup.

- **Plugin-first** — Named service registry and `ServiceLocatorBridge` are
  first-class features, not afterthoughts.

- **MEDI 对齐的 API** — 对 .NET 生态开发者友好。核心抽象直接对应：
  `IServiceCollection` → `ServiceCollection`、`IServiceProvider` → `ServiceProvider`、
  `IServiceScope` → `Scope`、`IServiceResolver` → 解析 trait。

- **零成本抽象** — 过程宏在编译期完成工作，运行时解析就是简单的 `Arc` 查找。

- **插件优先** — 命名服务注册和 `ServiceLocatorBridge` 是一等特性，而非事后补充。

---

## License · 许可

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

MIT 协议授权。详见 [LICENSE](LICENSE)。

<!-- Badges -->

[crates-badge]: https://img.shields.io/crates/v/lrdi.svg
[crates-url]: https://crates.io/crates/lrdi
[license-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[license-url]: https://opensource.org/licenses/MIT
[docs-url]: https://docs.rs/lrdi
[repo-url]: https://gitcode.com/rf2026/RDI
