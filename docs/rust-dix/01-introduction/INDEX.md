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

## 与 MEDI 对照（0.10.x 语义）

**单一对照源**：完整的 MEDI ↔ rust-dix API/语义对照、默认值与迁移动作见
[与 MEDI 的差异清单 · §11 API 对照补充表](medi-differences.md#11-api-对照补充表)，以及
[语义权威页（0.10）](../09-reference/semantics.md)。这里只保留三个最易误解的速记：

| 主题 | 速记 |
|------|------|
| scope 校验 | rust-dix 默认**严格**（root 解析 Scoped → `ScopedFromRootProvider`），对齐 ASP.NET Core 开发环境 `ValidateScopes = true`；裸 MEDI 默认关闭。owned 例外见参考页。 |
| `get_all` | 只含非 keyed 注册；keyed 多实现用 `get_all_keyed`（不含 `AnyKey` 兜底）。 |
| keyed / AnyKey | key 可为字符串或类型化 `ServiceKey`；**宏 key 支持表达式**（0.10，含下游 `impl From<LocalKey> for ServiceKey`）；`keyed_any_*` 为兜底，精确 key 优先，缓存维度为 (注册/类型, 请求 key)。扩展边界见 [扩展契约](../09-reference/extension-points.md)。 |

## 下一步

本章已发布的正文是 [与 MEDI 的差异清单](medi-differences.md)；随后可前往
[第二章 快速上手](../02-quickstart/INDEX.md) 或
[第八章 用法指南](../08-best-practices/usage-guide.md)。
