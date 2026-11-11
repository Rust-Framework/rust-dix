# 第一章 认识 rust-dix

本章介绍 rust-dix 框架的基本概念、与 Microsoft.Extensions.DependencyInjection 的对照，以及适用场景。

## 本章小节

| 小节 | 内容 |
|------|------|
| 什么是 rust-dix（规划中） | 框架定位与核心价值 |
| 核心概念与 MEDI 对照（规划中） | ServiceCollection、ServiceProvider、生命周期 |
| [与 MEDI 的差异清单](medi-differences.md) | 逐条差异：scope 校验、dispose、keyed、get_all 等 |
| 适用场景与边界（规划中） | 推荐使用场景与不推荐场景 |

> 标注"规划中"的小节尚未发布；本章当前仅发布《与 MEDI 的差异清单》。

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
| **松耦合** | 依赖接口而非具体类型 |
| **可测试** | 测试时轻松替换 mock |
| **可扩展** | 增加新实现无需修改消费方 |
| **开闭原则** | 对扩展开放，对修改关闭 |

## 与 MEDI 对照（0.8.x 语义）

> 完整差异（含默认值、边界与迁移动作）见 [与 MEDI 的差异清单](medi-differences.md)。

| MEDI (.NET) | rust-dix 0.8.x | 语义说明 |
|-------------|----------------|----------|
| `IServiceCollection` | `ServiceCollection` | 注册端 |
| `IServiceProvider` | `ServiceProvider` | 解析端（root） |
| `IServiceScope` | `Scope` / `ServiceScope` | 子作用域 |
| `IServiceScopeFactory` | `dyn ScopeFactory` | 内置弱引用实现，无需注册 |
| `CreateScope()` | `.scope()` | 创建独立作用域 |
| `ServiceLifetime.Singleton` | `ServiceLifetime::Singleton` | |
| `ServiceLifetime.Scoped` | `ServiceLifetime::Scoped` | |
| `ServiceLifetime.Transient` | `ServiceLifetime::Transient` | |
| `AddSingleton<T>()` | `.singleton::<T>(f)` | 工厂闭包式 |
| `AddScoped<T>()` | `.scoped::<T>(f)` | |
| `AddTransient<T>()` | `.transient::<T>(f)` | |
| `AddKeyedSingleton<T>(key)` | `.keyed_singleton::<T>(key, f)` | key 为字符串 |
| `GetRequiredService<T>()` | `.get::<T>()` | `Result`；**最后注册优先** |
| `GetService<T>()` | `.get_optional::<T>()` | `Option` |
| `GetServices<T>()` | `.get_all::<T>()` | **仅非 keyed**，按注册顺序 |
| `GetKeyedServices<T>(key)` | `.get_keyed::<T>(key)` / `.get_all_keyed::<T>()` | 按 key 单实例解析；批量枚举全部 keyed（对应 `AnyKey`） |
| `TryAddSingleton` / `TryAddScoped` / `TryAddTransient` | `.try_add_singleton(...)`（别名 `.try_add`）/ `.try_add_scoped(...)` / `.try_add_transient(...)` | 未注册才添加 |
| `ServiceProviderOptions.ValidateScopes` | `ServiceProviderOptions::validate_scopes` | 默认 `true`（严格）；`lenient()` 关闭 |
| `BuildServiceProvider(options)` | `.build_with(options)` / `.build_async_with(options)` | `build()` == `build_with(Default)` |
| `IServiceProviderIsService` | `.is_registered::<T>()` / `.is_registered_keyed::<T>(key)` | 只查询不实例化 |
| `ServiceProvider.Dispose()` | `.dispose()` | 幂等；之后解析 `Err(ProviderDisposed)` |
| `IServiceScope.Dispose()` | `.dispose()`（Drop 自动） | 逆序释放；之后解析 `Err(ScopeDisposed)` |

> 两处最容易被 .NET 经验误导的差异：
> 1. **scope 校验默认开启**：rust-dix 0.8.x 默认拒绝从 root 解析 Scoped
>    （`Err(ScopedFromRootProvider)`），等价 ASP.NET Core 开发环境的 `ValidateScopes = true`；
>    裸 MEDI `BuildServiceProvider()` 默认关闭。例外：`get_owned` / `try_get_owned`
>    可从 root 取 Scoped（owned 不缓存、不跟踪，但其 Scoped 依赖仍受校验）。
> 2. **`get_all` 不含 keyed**：keyed 多实现要用 `get_all_keyed::<T>()`。

## 下一步

本章已发布的正文是 [与 MEDI 的差异清单](medi-differences.md)；随后可前往
[第二章 快速上手](../02-quickstart/INDEX.md) 或
[第八章 用法指南](../08-best-practices/usage-guide.md)。
