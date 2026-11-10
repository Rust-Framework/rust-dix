# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.1] - 2026-11-10

> **English** · **简体中文**

### English

#### Added
- **Container injection in eager singletons**: `#[inject(provider)]` and
  `#[inject] Arc<dyn ScopeFactory>` now work on services constructed during
  `build()` / `build_async()` (previously `FactoryPanic` / `ServiceNotFound`).
  Build order is now: create provider `Arc` + `self_weak` → validate
  dependencies → eager-init remaining singletons.
- **Built-in container services on every resolver path**: `dyn ScopeFactory`
  and `ServiceProvider` resolve through `get()`, `get_any` / `get_by_type_id`
  (the path used by `#[inject]` fields), including during build-time validation.
  Explicit registrations still take precedence.
- **`#[inject] Arc<ServiceProvider>`**: the container itself is resolvable as a
  built-in service (MEDI `IServiceProvider` parity) in addition to the dedicated
  `#[inject(provider)]` attribute.

#### Changed
- **Built-in `ScopeFactory` is weak-backed**: it holds `Weak<ServiceProvider>`
  instead of `Arc`, so a singleton may store the factory without creating a
  `container → singleton → container` reference cycle. The container must outlive
  services created from it; `create_scope()` panics after the provider is dropped.
- **`get_by_type_id` built-in fallback fixed**: the `ScopeFactory` fallback is no
  longer short-circuited when the built-in type has no store entry.
- Build-time validation runs against the real provider; resolutions performed
  through an injected provider inside a constructor are dynamic and therefore not
  part of the static captive-dependency chain (documented; prefer resolving in
  methods).

#### Migration from 0.7.0
- Additive: no code changes required. Singleton constructors now receive a live
  provider; prefer `Arc<dyn ScopeFactory>` in singletons to avoid the
  strong-reference cycle of `#[inject(provider)]`.

### 简体中文

#### 新增
- **eager singleton 可注入容器自身**：`#[inject(provider)]` 与
  `#[inject] Arc<dyn ScopeFactory>` 在 `build()`/`build_async()` 期间构建的
  服务上可用（此前分别报 `FactoryPanic`/`ServiceNotFound`）。构建顺序调整为：
  创建 provider Arc + `self_weak` → 依赖校验 → eager 初始化剩余 singleton。
- **内建服务全解析路径可达**：`dyn ScopeFactory` 与 `ServiceProvider` 可通过
  `get()`、`get_any`/`get_by_type_id`（`#[inject]` 字段使用的路径）解析，
  构建期校验同样可用；显式注册仍然优先。
- **`#[inject] Arc<ServiceProvider>`**：容器自身作为内建服务可解析
  （对齐 MEDI `IServiceProvider`），与 `#[inject(provider)]` 并存。

#### 变更
- **内置 `ScopeFactory` 改为弱引用**：持有 `Weak<ServiceProvider>`，singleton
  保存它不会形成 `容器 → singleton → 容器` 的 Arc 环；容器必须先于其创建的服务
  存活（provider 释放后 `create_scope()` panic）。
- **修复 `get_by_type_id` 内建回退**：内建类型在 store 无注册时不再被提前 `?`
  短路。
- 构建期校验现在使用真实 provider；在构造函数内经注入 provider 发起的解析属于
  动态解析，不进入静态囚徒依赖链（已在文档说明，推荐在方法内延迟解析）。

#### 从 0.7.0 迁移
- 纯增量，无需改代码；singleton 构造函数现在能拿到可用的 provider。singleton 内
  推荐用 `Arc<dyn ScopeFactory>`，避免 `#[inject(provider)]` 的强引用环。

## [0.7.0] - 2026-09-01

> **English** · **简体中文**

### English

#### Added
- **`__rdi_try_construct_*`**: Result-returning inject constructors with field-level context on `ServiceNotFound` / `KeyedServiceNotFound`.
- **`inject_arc` / `macros_support`**: helpers so generated inject code reports typed errors instead of opaque panics.
- **Built-in `ScopeFactory`**: `provider.get::<dyn ScopeFactory>()` works without manual registration (explicit registration still overrides).
- **`try_add_singleton` / `try_add_scoped` / `try_add_transient`**: lifetime-aware try-add APIs (`try_add` remains singleton).
- **`RdiError::AsyncServiceRequiresBuildAsync`**: `build()` rejects async registrations with a typed error.
- **Runtime cycle detection** for Scoped/Transient resolution (thread-local stack → `CircularDependency`).
- **`RdiError::from_panic_payload`**: recovers `RdiError` panicked via `panic_any` from inject factories.

#### Changed
- **Dispose**: `dispose_scoped` waits on the disposables lock instead of silently skipping under contention.
- **Keyed errors**: `KeyedServiceNotFound` now includes optional `context` (same pattern as `ServiceNotFound`).
- **Branding**: crate docs refer to rust-dix (LRDI naming removed from module docs).

#### Migration from 0.6.x
- Prefer `__rdi_try_construct_T` when you need typed inject failures; `__rdi_construct_T` still returns `Arc<T>` and panics with `RdiError` payload (caught by `get()`).
- Match `KeyedServiceNotFound { key, ty, context }` (new `context` field).
- Call `build_async()` when any `async_*` service is registered — `build()` returns `Err` instead of succeeding then panicking on sync resolve.
- Manual `singleton::<dyn ScopeFactory>(...)` is optional.

### 简体中文

#### 新增
- **`__rdi_try_construct_*`**：返回 `Result` 的注入构造器，缺失依赖时带字段级 `context`。
- **内置 `ScopeFactory`**：无需手动注册即可 `get::<dyn ScopeFactory>()`。
- **`try_add_*` 生命周期变体**；`build()` 对 async 注册返回类型化错误。
- **Scoped/Transient 运行时环依赖检测**；inject 失败经 `panic_any(RdiError)` 由 `get()` 还原为类型错误。

#### 变更
- **dispose** 不再因 `try_lock` 失败而静默跳过；`KeyedServiceNotFound` 增加 `context`。

#### 从 0.6.x 迁移
- 需要类型化注入失败时用 `__rdi_try_construct_T`；原 `__rdi_construct_T` 仍可用。
- 匹配 `KeyedServiceNotFound` 时增加 `context` 字段。
- 含 `async_*` 注册时必须用 `build_async()`。

## [0.6.1] - 2026-08-16

> **English** · **简体中文**

### English

- **crates.io re-publish**: `rust-dix` / `rust-dix-macros` have been re-published to
  crates.io and are now managed under the `Rust-Framework` organization; repository
  metadata (`license`, `repository`) aligned with the GitHub repo.
- **Automated publishing**: new GitHub Actions `publish.yml` publishes crates in
  dependency order (`rust-dix-macros` → `rust-dix`) on `v*` tag push.
- **Docs landing page**: added `docs/README.md` bilingual navigation.

> Maintenance release; the runtime API is unchanged.

### 简体中文

- **crates.io 再发布**：`rust-dix` / `rust-dix-macros` 重新发布到 crates.io，归入
  `Rust-Framework` 组织统一管理；仓库元数据（`license`、`repository`）已对齐 GitHub 仓库。
- **自动化发布**：新增 GitHub Actions `publish.yml`，推送 `v*` tag 时按依赖顺序自动发布
  `rust-dix-macros` → `rust-dix`。
- **文档落地页**：新增 `docs/README.md` 中英文档导航入口。

> 本版本为发布维护迭代，不改变运行时 API。

## [0.6.0] - 2026-07 (migration notes)

> Historical gap filled for crates.io consumers. Runtime changes landed across 0.5→0.6.

### Changed (Breaking)
- Crate rename: `rust-dicore` → `rust-dix` / `rust-dix-macros`
- `ServiceCollection::build()` / `build_async()` return `Arc<ServiceProvider>`
- `get` / `get_owned` / `get_keyed_owned` return `Result<_, RdiError>` (no longer panic on missing)

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

[0.7.1]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.7.1
[0.7.0]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.7.0
[0.6.1]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.6.1
[0.4.1]: https://github.com/Rust-Framework/rust-dix/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/Rust-Framework/rust-dix/compare/v0.3.3...v0.4.0
[0.3.3]: https://github.com/Rust-Framework/rust-dix/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/Rust-Framework/rust-dix/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Rust-Framework/rust-dix/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Rust-Framework/rust-dix/compare/v0.2.6...v0.3.0
[0.2.6]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.2.6
