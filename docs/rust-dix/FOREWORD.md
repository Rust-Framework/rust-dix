# 前言

## 本书定位

**rust-dix 开发者手册** 是 rust-dix 依赖注入（DI）框架的权威指南。本书以**面向接口开发**为核心主线，讲解如何利用 DI 容器实现松耦合、可测试、可扩展的 Rust 应用架构。

## rust-dix 是什么

rust-dix 是一个受 Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架。它提供：

- 三种服务生命周期（Singleton / Scoped / Transient）
- 键控服务（Keyed Services）实现策略模式
- 面向接口的 `dyn Trait` 注册与解析
- 过程宏自动注册
- 分层容器支持插件隔离
- 跨 DLL 命名服务注册

**核心价值**：将面向接口开发模式从理念变为可落地的工程实践，让设计模式在 DI 容器中自然实现。

## 读者画像

| 读者类型 | 推荐阅读路径 |
|---------|-------------|
| Rust 初学者 | 第一、二章 → 第三章 → 第五章 → 第八章（第四章规划中） |
| 有 DI 经验的开发者 | 第三章 → 第五章 → 第八章 |
| 架构设计者 | 第三章 → 第五章 → 第八章（第七章规划中） |
| .NET 转 Rust 开发者 | 第一章 (MEDI 对照) → 第五章 → 第八章（第四章规划中） |
| 测试工程师 | 第三章 → 第八章（第六章规划中） |

## 核心概念

### 面向接口开发 = contracts + dyn Trait

rust-dix 推荐的分层结构：

```
contracts/          # 接口层：trait 定义 + DTO
  ├── user.rs       # trait IUserService + UserDTO
  └── payment.rs    # trait IPaymentGateway + PaymentRequest

handlers/           # 实现层：具体 Service + 自动注册
  ├── user.rs       # UserService impl IUserService + #[inject]
  └── payment.rs    # WechatPay impl IPaymentGateway + keyed

composition_root/   # 组合根：基础设施配置
  └── bootstrap.rs  # AppPaths、DbContext、外部库适配器
```

**规则**：
- Handler 只依赖 `Arc<dyn IUserService>`，不依赖具体类型
- 面向接口时：struct 用 `#[derive(Inject)]` 仅生成构造函数，`#[rust_dix::inject] impl Trait for Type` 注册为 `dyn Trait`（struct 与 impl 不重复用 `#[inject]`）
- 组合根只注册 DI 无法自动构造的类型（配置、DbContext）

### 设计模式 = keyed + get_all + 闭包工厂

| 设计模式 | rust-dix 实现 |
|---------|-----------------|
| 策略模式 | `keyed_singleton::<dyn Trait>("name", \|_| Arc::new(Impl))` + `get_keyed` |
| 工厂模式 | 闭包工厂或 keyed 工厂 |
| 观察者模式 | 非 keyed 多实现 + `get_all::<dyn Trait>()`（keyed 实现用 `get_all_keyed`） |
| 装饰器模式 | 工厂闭包链式包装 |
| 责任链模式 | keyed 处理器 + 配置驱动顺序 |

## 阅读顺序建议

### 快速入门路径（1-2 小时）

1. [第一章 认识 rust-dix](01-introduction/INDEX.md) —— 含已发布的 [与 MEDI 的差异清单](01-introduction/medi-differences.md)
2. [第二章 快速上手](02-quickstart/INDEX.md)（章节页；安装 / 第一个容器 / 生命周期小节规划中）
3. [第三章 面向接口编程范式](03-interface-oriented/INDEX.md) —— 已发布 [dyn Trait 注册与解析](03-interface-oriented/dyn-trait-registration.md)、[多实现与键控服务](03-interface-oriented/multiple-implementations.md)
4. [第八章 最佳实践](08-best-practices/INDEX.md) —— 已发布 [用法指南](08-best-practices/usage-guide.md)、[业务场景实战](08-best-practices/business-scenarios.md)

### 深度学习路径（半天）

1. [第一章 认识 rust-dix](01-introduction/INDEX.md) / [第二章 快速上手](02-quickstart/INDEX.md)（基础概念；部分小节规划中）
2. [第三章 面向接口编程范式](03-interface-oriented/INDEX.md)（面向接口范式）
3. [第五章 设计模式实践](05-design-patterns/INDEX.md)（章节页；策略 / 工厂 / 装饰器等小节规划中）
4. [第八章 最佳实践](08-best-practices/INDEX.md)（最佳实践）
5. [第九章 参考与附录](09-reference/INDEX.md)（语义权威页 / 错误目录 / 生产检查清单）

> 第四章「服务注册与解析」、第六章「测试策略」、第七章「高级特性」整体**规划中**，尚未发布；
> 参考章已发布，遇到语义/错误/上线问题先查参考章。

### 架构设计路径

1. [第三章 面向接口编程范式](03-interface-oriented/INDEX.md)（contracts 层设计；小节规划中）
2. [第五章 设计模式实践](05-design-patterns/INDEX.md)（设计模式选型；小节规划中）
3. [第八章 最佳实践](08-best-practices/INDEX.md)（含 [反模式警示](08-best-practices/anti-patterns.md)）

> 第七章「高级特性（插件系统）」规划中，尚未发布。

## 本书的特色

1. **概念对照表**：每个章节都有 MEDI / rust-dix 对照，帮助 .NET 开发者快速迁移
2. **代码即文档**：核心示例由 `crates/core/tests/` 与 `lib.rs` doctest 验证；书中标注
   `...` 或依赖上下文的片段为示意代码，不保证独立编译
3. **反模式警示**：明确指出常见错误做法及其修正方案
4. **渐进式披露**：从简单用法开始，逐步引入高级特性

## 版本说明

本书基于 rust-dix **0.9.x** 编写（当前 0.9.1 为文档补丁，无行为变更）。0.9 的完整语义
（类型化键 / `AnyKey` / async 链式环检测 / 诊断 / MSRV 1.95 / 安全基线）见
[参考章 · 0.9 语义权威页](09-reference/semantics-0.9.md)，与 MEDI 的逐条差异见
[与 MEDI 的差异清单](01-introduction/medi-differences.md)。

---

开始阅读：[第一章 认识 rust-dix](01-introduction/INDEX.md)