# 第一章 认识 rust-dix

本章介绍 rust-dix 框架的基本概念、与 Microsoft.Extensions.DependencyInjection 的对照，以及适用场景。

## 本章小节

| 小节 | 内容 |
|------|------|
| [什么是 rust-dix](what-is-rust-dix.md) | 框架定位与核心价值 |
| [核心概念与 MEDI 对照](core-concepts.md) | ServiceCollection、ServiceProvider、生命周期 |
| [适用场景与边界](who-should-use.md) | 推荐使用场景与不推荐场景 |

## rust-dix 是什么

rust-dix 是一个受 Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架，提供：

- **三种服务生命周期**：Singleton、Scoped、Transient
- **键控服务（Keyed Services）**：实现策略模式的多实现注册
- **面向接口开发**：`dyn Trait` 注册与解析
- **过程宏自动注册**：`#[rust_dix::inject]` 编译期收集
- **分层容器**：插件隔离、多租户支持
- **跨 DLL 插件支持**：命名服务注册表

## 核心价值

| 价值 | 说明 |
|------|------|
| **松耦合** | 依赖接口而非具体实现 |
| **可测试** | 测试时轻松替换 mock |
| **可扩展** | 增加新实现无需修改消费方 |
| **开闭原则** | 对扩展开放，对修改关闭 |

## 与 MEDI 对照

| MEDI (.NET) | rust-dix (Rust) |
|-------------|-------------------|
| `IServiceCollection` | `ServiceCollection` |
| `IServiceProvider` | `ServiceProvider` |
| `IServiceScope` | `Scope` / `ServiceScope` |
| `ServiceLifetime.Singleton` | `ServiceLifetime::Singleton` |
| `ServiceLifetime.Scoped` | `ServiceLifetime::Scoped` |
| `ServiceLifetime.Transient` | `ServiceLifetime::Transient` |
| `AddSingleton<T>()` | `.singleton::<T>(f)` |
| `AddScoped<T>()` | `.scoped::<T>(f)` |
| `AddTransient<T>()` | `.transient::<T>(f)` |
| `AddKeyedSingleton<T>(key)` | `.keyed_singleton::<T>(key, f)` |
| `GetRequiredService<T>()` | `.get::<T>()` |
| `GetService<T>()` | `.get_optional::<T>()` |
| `GetServices<T>()` | `.get_all::<T>()` |
| `CreateScope()` | `.scope()` |

## 下一步

从 [什么是 rust-dix](what-is-rust-dix.md) 开始深入了解。