# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.1] - 2026-06-29

### Changed

- **文档同步**: 全面校正 0.4.0 行为变更相关文档，消除过期描述
  - `api.md` ServiceProvider 结构体定义更新为 `LazyCache`（singleton_cache + root_scoped_cache），Scope 解析流程同步
  - `architecture.md` 缓存结构、解析流程图、线程安全设计更新为 LazyCache + captive dependency 检测
  - `common-pitfalls.md` 陷阱 5 重写：从"Scope 语义丢失"改为"根 Scope 与子 Scope 语义混淆"，澄清根级 Scoped 缓存行为
  - `anti-patterns.md` 反模式 8 重写：澄清 0.4.0 起根 Provider 解析 Scoped 缓存于 root_scoped_cache
  - `interface-checklist.md` 作用域使用规则更新，区分根 scope 与子 scope 语义
  - 生命周期互引用规则表更新：Singleton → Transient 允许，仅 Singleton → Scoped 在 build 阶段拒绝
  - `build()` 副作用描述补充 captive dependency 检测阶段
- 新增 0.4.1 边界测试：嵌套 Transient 链 `OuterTransient → MidTransient → ScopedCore` 验证 Scoped 绑定到子 scope

### Fixed

- 文档中残留的"根 Provider 解析 Scoped 等价于 Transient"等过期描述全部清除
- `INDEX.json` 版本号同步

## [0.4.0] - 2026-06-29

### Changed (Breaking)

- **Root scope 语义对齐 MEDI**: `ServiceProvider` 本身即 root scope，从根直接解析 Scoped 服务时在根内缓存复用（语义等同于根 scope 内单例化），不再退化为每次新建
  - 新增 `root_scoped_cache` 字段，与 `singleton_cache` 分离
  - 通过 `create_scope()` 创建的子 Scope 拥有独立 `scoped_cache`，不回退到根

- **LazyCache 统一缓存架构**: 新增 `cache.rs`，用 per-key `OnceLock` 模式彻底消除 TOCTOU 竞态
  - `RwLock<HashMap<key, Arc<OnceLock<AnyService>>>>` 结构，工厂在无锁状态下执行，避免递归解析死锁
  - `ServiceProvider`（singleton_cache + root_scoped_cache）与 `Scope`（scoped_cache）统一采用

### Added

- **Captive dependency 检测**: build 阶段拒绝 Singleton 依赖 Scoped（含间接依赖链 Singleton → Transient → Scoped）
  - `ValidationResolver` 解析栈跟踪祖先 lifetime，通过 panic + `catch_unwind` 传播 `RdiError::SingletonDependsOnScoped`
  - `RdiError` 派生 `Clone` 以支持 panic 跨 unwind 边界传递

### Fixed

- **Validation 缓存泄漏**: build 阶段 validation 只缓存 Singleton 实例，Transient/Scoped 实例不进入 `singleton_cache`，运行时解析返回新实例
- **子 scope Transient 依赖绑定**: `Scope` 的 Transient 分支用 `self` 而非 `self.parent` 解析依赖，Transient 工厂内的 Scoped 依赖绑定到当前子 scope

## [0.3.3] - 2026-06-28

### Fixed

- 内部维护版本

## [0.3.2] - 2024-06-26

### Fixed

- **循环依赖检测编译错误**: 修复 `ValidationResolver` 中 `RwLock<HashSet>` 内部可变性问题
  - 使用 `RwLock::write().unwrap()` 替代直接 `insert`/`remove`
  - 确保在 `&self` 方法中可以安全修改解析链状态

## [0.3.1] - 2024-06-26

### Added

- **Complete documentation system**: Added full docs/rust-dix/ best practices guide
  - 01-introduction: Framework overview and MEDI comparison
  - 02-quickstart: Installation and first container
  - 05-design-patterns: Strategy, factory, decorator patterns
  - INDEX.json: Structured documentation index for navigation

- **English/Chinese documentation links**: README.md ↔ README.zh.md cross-links verified

### Changed

- Updated dependency version example to `"0.3"` in README files

## [0.3.0] - 2024-06-26

### Breaking Changes

- **API 命名统一**: 移除了 `.keyed()` 方法，统一使用 `.keyed_singleton()` 替代
  - `.keyed(k, f)` → `.keyed_singleton(k, f)`
  - 此变更使键控服务 API 命名更加明确一致

### Added

- **循环依赖检测**: 在 `build()` 时检测并报告循环依赖
  - 新增 `RdiError::CircularDependency` 错误类型
  - 构建时追踪解析链，检测自引用和间接循环依赖
  - 提供清晰的错误信息，包含依赖链路径

- **生产就绪文档**: 创建了完整的最佳实践文档体系
  - `docs/rust-dix/` - 面向接口开发指南
  - 设计模式实践 (策略、工厂、装饰器等)
  - 常见陷阱与反模式警示

- **增强的文档注释**: `lib.rs` 包含完整的示例和功能说明

### Changed

- **键控服务生命周期明确化**: 三个键控方法现在命名一致
  - `keyed_singleton(key, factory)` - 全局共享
  - `keyed_scoped(key, factory)` - Scope 内共享
  - `keyed_transient(key, factory)` - 每次新建

- **面向接口开发引导**: README 和 Skill 文档强化了面向接口开发模式的引导

### Fixed

- 文档示例代码统一使用 `.keyed_singleton()`
- 所有测试文件已同步更新

## [0.2.6] - 2024-06-25

### Added

- 初始版本发布
- 三种生命周期支持 (Singleton, Scoped, Transient)
- 键控服务 (Keyed Services)
- 过程宏支持 (`#[derive(Inject)]`, `#[lrdi::inject]`, `#[lrdi::module]`)
- 分层容器 (ServiceProviderWrapper)
- 跨 DLL 插件支持 (IServiceLocator)
- 多接口注册支持

[0.4.1]: https://gitcode.com/rf2026/rust-di/compare/v0.4.0...v0.4.1
[0.4.0]: https://gitcode.com/rf2026/rust-di/compare/v0.3.3...v0.4.0
[0.3.3]: https://gitcode.com/rf2026/rust-di/compare/v0.3.2...v0.3.3
[0.3.2]: https://gitcode.com/rf2026/rust-di/compare/v0.3.1...v0.3.2
[0.3.1]: https://gitcode.com/rf2026/rust-di/compare/v0.3.0...v0.3.1
[0.3.0]: https://gitcode.com/rf2026/rust-di/compare/v0.2.6...v0.3.0
[0.2.6]: https://gitcode.com/rf2026/rust-di/releases/tag/v0.2.6
