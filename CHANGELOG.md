# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.2] - 2024-06-26

### Fixed

- **循环依赖检测编译错误**: 修复 `ValidationResolver` 中 `RwLock<HashSet>` 内部可变性问题
  - 使用 `RwLock::write().unwrap()` 替代直接 `insert`/`remove`
  - 确保在 `&self` 方法中可以安全修改解析链状态

## [0.3.1] - 2024-06-26

### Added

- **Complete documentation system**: Added full docs/rust-dicore/ best practices guide
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
  - `docs/rust-dicore/` - 面向接口开发指南
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

[0.3.3]: https://gitcode.com/rf2026/rust-di/compare/v0.3.2...v0.3.3
[0.3.2]: https://gitcode.com/rf2026/rust-di/compare/v0.3.1...v0.3.2
[0.3.1]: https://gitcode.com/rf2026/rust-di/compare/v0.3.0...v0.3.1
[0.3.0]: https://gitcode.com/rf2026/rust-di/compare/v0.2.6...v0.3.0
[0.2.6]: https://gitcode.com/rf2026/rust-di/releases/tag/v0.2.6
