# rust-dix 开发者手册 · 目录

> 面向使用 rust-dix 框架的开发者 · 面向接口开发 · 设计模式落地

---

## 开篇

| 文档 | 说明 |
|------|------|
| [前言](FOREWORD.md) | 本书定位、读者画像、阅读路径 |

---

## 第一部分 · 入门与认知

### [第一章 认识 rust-dix](01-introduction/INDEX.md)

- [什么是 rust-dix](01-introduction/what-is-rust-dix.md)
- [核心概念与 MEDI 对照](01-introduction/core-concepts.md)
- [适用场景与边界](01-introduction/who-should-use.md)

### [第二章 快速上手](02-quickstart/INDEX.md)

- [安装与依赖配置](02-quickstart/installation.md)
- [第一个 DI 容器](02-quickstart/first-container.md)
- [生命周期选择](02-quickstart/lifetime-choice.md)

---

## 第二部分 · 面向接口开发

### [第三章 面向接口编程范式](03-interface-oriented/INDEX.md)

- [为什么面向接口](03-interface-oriented/why-interface.md)
- [trait 定义与 contracts 层](03-interface-oriented/contracts-layer.md)
- [dyn Trait 注册与解析](03-interface-oriented/dyn-trait-registration.md)
- [多实现与键控服务](03-interface-oriented/multiple-implementations.md)

---

## 第三部分 · DI 核心模式

### [第四章 服务注册与解析](04-di-patterns/INDEX.md)

- [注册 API 详解](04-di-patterns/registration-api.md)
- [解析 API 与 IServiceResolver](04-di-patterns/resolution-api.md)
- [作用域管理](04-di-patterns/scope-management.md)
- [分层容器与插件隔离](04-di-patterns/layered-container.md)

---

## 第四部分 · 设计模式落地

### [第五章 设计模式实践](05-design-patterns/INDEX.md)

- [策略模式：keyed 服务](05-design-patterns/strategy-pattern.md)
- [工厂模式：闭包工厂与 keyed 工厂](05-design-patterns/factory-pattern.md)
- [装饰器模式：链式包装](05-design-patterns/decorator-pattern.md)
- [观察者模式：get_all 广播](05-design-patterns/observer-pattern.md)
- [责任链模式：处理器链](05-design-patterns/chain-pattern.md)
- [更多模式速查](05-design-patterns/pattern-reference.md)

---

## 第五部分 · 测试与质量

### [第六章 测试策略](06-testing/INDEX.md)

- [Mock 替换策略](06-testing/mock-replacement.md)
- [测试容器构建](06-testing/test-container.md)
- [接口隔离测试](06-testing/interface-testing.md)

---

## 第六部分 · 高级特性

### [第七章 高级特性](07-advanced/INDEX.md)

- [过程宏系统](07-advanced/proc-macros.md)
- [跨 DLL 插件支持](07-advanced/cross-dll-plugin.md)
- [编译期模块扫描](07-advanced/module-scanning.md)

---

## 第七部分 · 最佳实践

### [第八章 最佳实践](08-best-practices/INDEX.md)

- [面向接口开发清单](08-best-practices/interface-checklist.md)
- [用法指南与决策矩阵](08-best-practices/usage-guide.md)
- [业务场景实战](08-best-practices/business-scenarios.md)
- [常见陷阱与排查](08-best-practices/common-pitfalls.md)
- [性能优化建议](08-best-practices/performance-tips.md)
- [反模式警示](08-best-practices/anti-patterns.md)