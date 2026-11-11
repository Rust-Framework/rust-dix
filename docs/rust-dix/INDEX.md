# rust-dix 开发者手册 · 目录

> 面向使用 rust-dix 框架的开发者 · 面向接口开发 · 设计模式落地
>
> **发布状态说明**：本目录只对**已发布页面**提供链接；标注 **规划中** 的小节/章节尚未发布，
> 因此不带链接。`INDEX.json` 也只登记已发布页面。

---

## 开篇

| 文档 | 说明 |
|------|------|
| [前言](FOREWORD.md) | 本书定位、读者画像、阅读路径 |

---

## 第一部分 · 入门与认知

### [第一章 认识 rust-dix](01-introduction/INDEX.md)

- 什么是 rust-dix（规划中）
- 核心概念与 MEDI 对照（规划中）
- [与 MEDI 的差异清单](01-introduction/medi-differences.md)
- 适用场景与边界（规划中）

### [第二章 快速上手](02-quickstart/INDEX.md)

- 安装与依赖配置（规划中）
- 第一个 DI 容器（规划中）
- 生命周期选择（规划中）

---

## 第二部分 · 面向接口开发

### [第三章 面向接口编程范式](03-interface-oriented/INDEX.md)

- 为什么面向接口（规划中）
- trait 定义与 contracts 层（规划中）
- [dyn Trait 注册与解析](03-interface-oriented/dyn-trait-registration.md)
- [多实现与键控服务](03-interface-oriented/multiple-implementations.md)

---

## 第三部分 · DI 核心模式

### 第四章 服务注册与解析（规划中，尚未发布）

- 注册 API 详解（规划中）
- 解析 API 与 IServiceResolver（规划中）
- 作用域管理（规划中）
- 分层容器与插件隔离（规划中）

> 该章尚未发布；注册/解析/作用域的现行约定可先参考
> [用法指南](08-best-practices/usage-guide.md) 与
> [与 MEDI 的差异清单](01-introduction/medi-differences.md)。

---

## 第四部分 · 设计模式落地

### [第五章 设计模式实践](05-design-patterns/INDEX.md)

- 策略模式：keyed 服务（规划中）
- 工厂模式：闭包工厂与 keyed 工厂（规划中）
- 装饰器模式：链式包装（规划中）
- 观察者模式：get_all 广播（规划中）
- 责任链模式：处理器链（规划中）
- 更多模式速查（规划中）

> 本章章节页已发布（含模式与 0.8.0 语义速查）；具体模式小节规划中。
> 多实现/键控服务的现行用法见
> [多实现与键控服务](03-interface-oriented/multiple-implementations.md)。

---

## 第五部分 · 测试与质量

### 第六章 测试策略（规划中，尚未发布）

- Mock 替换策略（规划中）
- 测试容器构建（规划中）
- 接口隔离测试（规划中）

> 本章尚未发布；测试替换/容器的现行约定可先参考
> [面向接口开发清单](08-best-practices/interface-checklist.md) 与
> [业务场景实战](08-best-practices/business-scenarios.md)。

---

## 第六部分 · 高级特性

### 第七章 高级特性（规划中，尚未发布）

- 过程宏系统（规划中）
- 跨 DLL 插件支持（规划中）
- 编译期模块扫描（规划中）

> 本章尚未发布；过程宏的现行文档见
> [rust-dix-macros README](../../crates/macros/README.md)。

---

## 第七部分 · 最佳实践

### [第八章 最佳实践](08-best-practices/INDEX.md)

- [面向接口开发清单](08-best-practices/interface-checklist.md)
- [用法指南与决策矩阵](08-best-practices/usage-guide.md)
- [业务场景实战](08-best-practices/business-scenarios.md)
- [常见陷阱与排查](08-best-practices/common-pitfalls.md)
- 性能优化建议（规划中）
- [反模式警示](08-best-practices/anti-patterns.md)
