# 前言

## 本书定位

**rust-dicore 开发者手册** 是 rust-dicore 依赖注入框架的权威指南。本书以**面向接口开发**为核心主线，讲解如何利用 DI 容器实现松耦合、可测试、可扩展的 Rust 应用架构。

## rust-dicore 是什么

rust-dicore 是一个受 Microsoft.Extensions.DependencyInjection (MEDI) 启发的 Rust 依赖注入框架。它提供：

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
| Rust 初学者 | 第一、二章 → 第三章 → 第四章 |
| 有 DI 经验的开发者 | 第三章 → 第五章 → 第八章 |
| 架构设计者 | 第三章 → 第五章 → 第七章 → 第八章 |
| .NET 转 Rust 开发者 | 第一章 (MEDI 对照) → 第四章 → 第五章 |
| 测试工程师 | 第三章 → 第六章 → 第八章 |

## 核心概念

### 面向接口开发 = contracts + dyn Trait

rust-dicore 推荐的分层结构：

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
- Service 实现用 `#[inject(singleton, as = dyn IUserService)]` 注册
- 组合根只注册 DI 无法自动构造的类型（配置、DbContext）

### 设计模式 = keyed + get_all + 闭包工厂

| 设计模式 | rust-dicore 实现 |
|---------|-----------------|
| 策略模式 | `keyed::<dyn Trait>("name", \|_| Arc::new(Impl))` + `get_keyed` |
| 工厂模式 | 闭包工厂或 keyed 工厂 |
| 观察者模式 | 多个 keyed 注册 + `get_all::<dyn Trait>()` |
| 裣饰器模式 | 工厂闭包链式包装 |
| 责任链模式 | keyed 处理器 + 配置驱动顺序 |

## 阅读顺序建议

### 快速入门路径（1-2 小时）

1. [什么是 rust-dicore](01-introduction/what-is-rust-dicore.md)
2. [第一个 DI 容器](02-quickstart/first-container.md)
3. [为什么面向接口](03-interface-oriented/why-interface.md)
4. [策略模式实践](05-design-patterns/strategy-pattern.md)

### 深度学习路径（半天）

1. 第一、二章（基础概念）
2. 第三章（面向接口范式）
3. 第四章（DI 核心模式）
4. 第五章（设计模式实践）
5. 第八章（最佳实践）

### 架构设计路径

1. 第三章（contracts 层设计）
2. 第五章（设计模式选型）
3. 第七章（插件系统）
4. 第八章（反模式警示）

## 本书的特色

1. **概念对照表**：每个章节都有 MEDI / rust-dicore 对照，帮助 .NET 开发者快速迁移
2. **代码即文档**：所有示例代码均可编译运行，配合测试验证
3. **反模式警示**：明确指出常见错误做法及其修正方案
4. **渐进式披露**：从简单用法开始，逐步引入高级特性

## 版本说明

本书基于 rust-dicore 0.2.6 版本编写。API 变化时会更新对应章节。

---

开始阅读：[第一章 认识 rust-dicore](01-introduction/INDEX.md)