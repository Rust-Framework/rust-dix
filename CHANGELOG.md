# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.1] - 2026-11-13

> **English** · **简体中文**

### English

#### Added
- **`instance_dyn<T: ?Sized>` / `instance<T: ?Sized>`**: `Arc<dyn Trait>` can now be registered
  and resolved directly (`svc.instance_dyn::<dyn IClock>(Arc::new(FakeClock))`), so port
  implementations no longer need a newtype wrapper.
- **Typed resolution inside factory closures**: `IServiceResolver::get_arc(TypeId)` is dyn-safe,
  and `IServiceResolverExt::resolve_arc<T>()` gives a typed, `?Sized`-friendly wrapper. A factory
  can write `let db: Arc<DbContext> = resolver.resolve_arc()?;` instead of `get_any(type_name)`
  plus a downcast.
- **Build-time factory resolution fixed**: `ValidationResolver` now forwards `get_by_type_id`,
  so factories that run during `build()` resolve typed services instead of always returning `None`.

#### Fixed
- **Derive/inject macros resolve the real dependency name** (`proc-macro-crate`): generated paths
  no longer hard-code `::rust_dix::…`, so `dix = { package = "rust-dix" }` works without the
  `extern crate dix as rust_dix;` shim. `#[rust_dix::module]`'s `register!` is matched on the last
  path segment, so renamed paths (`di::register!`) work too.

#### Migration
- Remove the `extern crate dix as rust_dix;` shims added for 0.10 (the `rust_dix` names in docs are
  historical; the macros now emit the resolved name).

### 简体中文

#### 新增
- **`instance_dyn<T: ?Sized>` / `instance<T: ?Sized>`**：`Arc<dyn Trait>` 可直接注册与解析
  （`svc.instance_dyn::<dyn IClock>(Arc::new(FakeClock))`），端口实现不再需要 newtype 包装。
- **工厂闭包内的类型化解析**：`IServiceResolver::get_arc(TypeId)`（dyn 安全）+
  `IServiceResolverExt::resolve_arc<T>()`；工厂内可写
  `let db: Arc<DbContext> = resolver.resolve_arc()?;`，替代 `get_any(type_name)` + downcast。
- **build 期工厂解析修复**：`ValidationResolver` 补齐 `get_by_type_id`，build 期工厂不再恒返回 `None`。

#### 修复
- **派生宏按真实依赖名解析**（`proc-macro-crate`）：不再硬编码 `::rust_dix::…`，
  `dix = { package = "rust-dix" }` 改名后无需 `extern crate` 垫片；`register!` 改为末段匹配，
  `di::register!` 等别名路径可用。

#### 迁移
- 删除 0.10 时期为改名添加的 `extern crate dix as rust_dix;` 垫片。

## [0.10.0] - 2026-11-11

> **English** · **简体中文**

### English

#### Added
- **Macro key expressions (backward compatible)**: `#[inject(key = <expr>)]` and
  `#[inject(owned, key = <expr>)]` accept any `impl Into<ServiceKey>` — string literals
  (still `String` keys), `ServiceKey::new(Kind::A)` typed keys, or downstream key types
  after `impl From<LocalKey> for ServiceKey` (the official extension path; do not copy the
  injection macro). `register!(keyed <expr>: <lifetime>: <Type>)` inside `#[rust_dix::module]`
  also accepts expressions.
- **`register!(keyed <expr>: <lt>: dyn Trait => Impl)` now really registers `Impl`**:
  previously the `=> Impl` part could be silently dropped (the trait's `Default` was used
  instead); 0.10 honors `Impl`. Keyed `=> Impl` syntax is covered by tests.
- **Typed-keyed resolver extension**: `IServiceResolver` adds
  `get_keyed_any_key` / `try_get_keyed_any_result_key` / `get_keyed_owned_any_key` /
  `try_get_keyed_owned_any_key`. Defaults fall back to the string methods only for
  string-like keys (`ServiceKey::as_str()`); `ServiceProvider`, `Scope`,
  `ServiceProviderWrapper` and the build-time validation resolver implement full typed-key
  lookup (AnyKey fallback, owned, captive checks). `ServiceKey::as_str()` is now public.
- **Reference: extension points page** (`09-reference/extension-points.md`): official
  extension contract (`From` + macros), custom `IServiceResolver` guidance, explicit
  registration + custom resolver pattern, and the async AnyKey boundary/workarounds.

#### Changed
- Macro keys are now **evaluated as expressions** instead of literal strings. String
  literals remain source-compatible and still produce `String` keys.
- `rdi-E004` compile-time duplicate-key detection applies to **string-literal keys only**;
  dynamic-expression keys follow runtime "last registration wins".
- Typed-key macro injection and validation participate in the existing captive/AnyKey
  checks (behavior otherwise unchanged).
- **Key-expression evaluation timing**: each consumer field evaluates its key once at
  construction; singletons evaluate once at build/validation; transients evaluate per
  resolution; `register!(keyed <expr>)` evaluates once per build. Key expressions should be
  stable and side-effect free (prefer `const`/locals/explicit registration).
- `ServiceKey::as_str()` returns `Some` for `String` and `&'static str` (including
  `ServiceKey::new("literal")`) and `None` for enum/integer/custom typed keys and `AnyKey`.
  Default typed-key resolver methods treat string-like keys as the legacy **`String` key**,
  which is not equal to an `&str`-typed key in fully typed containers.

#### Fixed
- `register!(keyed <expr>: <lifetime>: dyn Trait => Impl)` no longer silently drops
  `=> Impl` (previously the trait's `Default` could be registered instead of `Impl`).
- `#[inject(provider, key = ...)]` is now a clear compile error (provider fields cannot be
  keyed; inject `Arc<ServiceProvider>` and resolve keyed services on demand).

#### Migration (0.9 → 0.10)
- Source-compatible for existing string-key code. Custom `IServiceResolver` implementations
  keep compiling, but their default typed-key methods only support string-like keys; override
  the four `*_key` methods to support typed keys.
- Downstream frameworks should use `impl From<LocalKey> for ServiceKey` + existing macros
  instead of copying the injection macro.

#### Notes
- **`async_any_*` is still not provided.** `get_keyed_async` falls back to the synchronous
  AnyKey path; use finite `async_keyed_*` registrations, or a sync AnyKey service with an
  internal per-key `tokio::sync::OnceCell`/map. See the extension-points page for the
  re-evaluation criteria.
- No MSRV/behavior breakage; MSRV remains **1.95**.

### 简体中文

#### 新增
- **宏 key 表达式（向后兼容）**：`#[inject(key = <expr>)]` 与
  `#[inject(owned, key = <expr>)]` 接受任意 `impl Into<ServiceKey>`——字符串字面量（仍为
  `String` key）、`ServiceKey::new(Kind::A)` typed key，或下游 key 类型实现
  `impl From<LocalKey> for ServiceKey` 后直接使用（**官方扩展路径，勿复制注入宏**）。
  `#[rust_dix::module]` 内的 `register!(keyed <expr>: <lifetime>: <Type>)` 同样接受表达式。
- **`register!(keyed <expr>: <lt>: dyn Trait => Impl)` 现在真正注册 `Impl`**：此前 `=> Impl`
  可能被静默丢弃（改用 trait 的 `Default`）；0.10 按 `Impl` 注册，并有测试覆盖。
- **typed keyed 解析扩展**：`IServiceResolver` 新增
  `get_keyed_any_key` / `try_get_keyed_any_result_key` / `get_keyed_owned_any_key` /
  `try_get_keyed_owned_any_key`。默认实现仅对 string-like key（`ServiceKey::as_str()`）回退
  字符串方法；`ServiceProvider`、`Scope`、`ServiceProviderWrapper` 与构建期
  ValidationResolver 实现完整 typed key（AnyKey 兜底、owned、captive 检查）。
  `ServiceKey::as_str()` 成为公开 API。
- **参考页：扩展契约与边界**（`09-reference/extension-points.md`）：官方扩展契约
  （`From` + 宏）、自定义 `IServiceResolver`、显式注册 + 自定义 resolver 模式，以及
  async AnyKey 边界/workaround。

#### 变更
- 宏 key 由字面量字符串改为**表达式求值**；字符串字面量源码兼容且仍产生 `String` key。
- `rdi-E004` 编译期重复 key 检测**仅对字符串字面量**生效；动态表达式 key 按运行时
  "最后注册优先"。
- typed key 宏注入与验证期 captive/AnyKey 行为沿袭既有校验，其余行为不变。
- **key 表达式求值时机**：每个消费者字段构造时求值一次；singleton 在 build/validation
  求值一次；transient 每次解析求值一次；`register!(keyed <expr>)` 每次 build 求值一次。
  key 表达式应稳定、无副作用（建议 `const`/局部变量/显式注册）。
- `ServiceKey::as_str()` 对 `String` 与 `&'static str`（含 `ServiceKey::new("literal")`）
  返回 `Some`；enum/整数/自定义 typed key、`AnyKey` 返回 `None`。默认 typed-key resolver
  会把 string-like key 当作旧 **`String` key**，在完整 typed 容器中与 `&str` typed key
  不相等。

#### 修复
- `register!(keyed <expr>: <lifetime>: dyn Trait => Impl)` 不再静默丢弃 `=> Impl`
  （此前可能错误注册 trait 的 `Default` 而非 `Impl`）。
- `#[inject(provider, key = ...)]` 现在是明确编译错误（provider 字段不能带 key；应注入
  `Arc<ServiceProvider>` 并在方法内按需解析）。

#### 迁移（0.9 → 0.10）
- 现有字符串 key 代码源码兼容。自定义 `IServiceResolver` 实现仍可编译，但默认 typed-key
  方法仅支持 string-like key；要支持 typed key 需 override 4 个 `*_key` 方法。
- 下游框架推荐 `impl From<LocalKey> for ServiceKey` + 现有宏，而非复制注入宏。

#### 说明
- **`async_any_*` 仍未提供**：`get_keyed_async` 对 AnyKey 回退同步路径；有限 key 用
  `async_keyed_*`，动态 key 用同步 AnyKey + 内部 per-key `tokio::sync::OnceCell`/map。
  再评估条件见扩展页。
- 无 MSRV/行为破坏；MSRV 仍为 **1.95**。

## [0.9.1] - 2026-11-11

> **English** · **简体中文**

### English

#### Documentation (no behavior change)
- Fixed non-compilable `#[inject]` / `#[derive(Inject)]` examples (missing field attributes
  such as `#[inject]` / `#[inject(owned)]`) across README, `crates/core`, `crates/macros`
  and the interface-oriented/checklist pages. Corrected quick-start snippets were verified
  with a `cargo check` probe crate (Arc dependency, owned bare dependency, trait-impl
  registration).
- Added the **Reference** chapter (docs `09-reference/`): `semantics` (single authority
  for 0.9 semantics at that time), an `RdiError` catalog, and the production deployment checklist
  (moved from `docs/production-deployment-checklist.md` and now linked from the book,
  `docs/README.md` and `INDEX.json`).
- Deduplicated repeated 0.9 semantics into at most 5-line contextual summaries that link to
  the authoritative page; `crates/*` READMEs use absolute GitHub URLs (crates.io-safe).
- Corrected `get_optional` semantics for uninitialized async services (`None`, not
  `AsyncServiceRequiresGetAsync`); completed missing API docs (`ServiceKey::is_any`,
  `ServiceProviderOptions::new`/`with_validate_scopes`/`Default`,
  `IServiceResolver::get_all_any_result`/`try_get_keyed_any_result`,
  `try_get_keyed_owned`, `#[inject(key)]`/`#[inject(owned, key)]`); fixed the double-
  registration "correct way" example, the over-claim that every snippet compiles, the
  `docs/README.md` tree/published-count, `INDEX.json` metadata/summaries and the Chinese
  README links to the Chinese crate READMEs.
- Rewrote the `rust-dix` / `rust-dix-macros` crate rustdoc/docs.rs homepages for the 0.9
  semantics and re-checked links/anchors (`BROKEN=0`, anchor miss=0).

#### Notes
- **No runtime, API, MSRV or behavior change.** `Cargo.toml` version is bumped only so the
  documentation patch can be republished; upgrading from 0.9.0 carries no risk.
- Repository CI/publish workflows now verify that manifests are UTF-8 **without BOM** before
  fmt/clippy/doc/test gates (prevents a release-blocking encoding regression).

### 简体中文

#### 文档（无行为变更）
- 修正 README、`crates/core`、`crates/macros` 与面向接口/清单页中**不能编译**的
  `#[inject]` / `#[derive(Inject)]` 示例（缺字段 `#[inject]` / `#[inject(owned)]` 等）。
  修正后的 quick-start 片段已用临时 `cargo check` probe crate 验证（Arc 依赖、owned 裸依赖、
  trait impl 注册三类）。
- 新增**参考章**（docs `09-reference/`）：语义权威页（当时的 0.9 语义）、
  `RdiError` 错误目录、生产部署检查清单（自 `docs/production-deployment-checklist.md`
  迁入，并登记到目录、`docs/README.md` 与 `INDEX.json`，修掉孤儿页）。
- 去重重复的 0.9 语义表：各页仅保留 **≤5 行**情境摘要并链接权威页；`crates/*` README
  对 docs 的链接改用**绝对 GitHub URL**（crates.io 可点）。
- 修正未初始化 async 服务的 `get_optional` 语义（返回 `None`，非
  `AsyncServiceRequiresGetAsync`）；补齐 API 文档（`ServiceKey::is_any`、
  `ServiceProviderOptions::new`/`with_validate_scopes`/`Default`、
  `IServiceResolver::get_all_any_result`/`try_get_keyed_any_result`、
  `try_get_keyed_owned`、`#[inject(key)]`/`#[inject(owned, key)]`）；修正双重注册"正确做法"
  错误范式、`所有示例可编译` 过度宣称、`docs/README.md` 目录树/已发布章数、`INDEX.json`
  元数据与 summaries、中文 README 指向中文 crate README。
- 重写 `rust-dix` / `rust-dix-macros` 的 crate rustdoc/docs.rs 首页以匹配 0.9 语义，
  并复核链接/锚点（`BROKEN=0`、锚点 miss=0）。

#### 说明
- **无运行时、API、MSRV 或行为变更**。`Cargo.toml` 版本号提升仅用于重新发布文档补丁；
  从 0.9.0 升级无风险。
- 仓库 CI/发布工作流新增 **manifest 无 UTF-8 BOM** 校验（在 fmt/clippy/doc/test 门禁前执行），
  防止编码回归阻塞发布。

## [0.9.0] - 2026-11-11

> **English** · **简体中文**

### English

#### Added
- **Typed service keys (`ServiceKey`)**: keyed registrations/resolution now accept
  `impl Into<ServiceKey>`. `ServiceKey::new(value)` wraps any
  `Any + Send + Sync + Eq + Hash + Debug` value (enum, integer, …); `From<&str>` /
  `From<String>` / `From<&String>` keep string keys source-compatible.
  `ServiceKey::any_key()` / `KeyedService::any_key()` provide the `AnyKey` sentinel, and
  `ServiceKey::is_any()` detects it. Key equality includes the value's type
  (`ServiceKey::new(1u32) != ServiceKey::from("1")`).
- **`AnyKey` fallback registrations (MEDI `KeyedService.AnyKey`)**:
  `keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`. The factory receives
  the requested key (`Fn(&dyn IServiceResolver, &ServiceKey) -> Arc<T>`). Exact-key
  registrations take precedence; otherwise the fallback is used. Singleton/scoped
  fallback instances are cached **per (registration/type, requested key)** — the same
  registration + key reuses one instance, different keys or different `AnyKey`
  registrations get independent instances; scoped: one per scope per (registration, key).
  Transient instances are created per resolution. `get_all_keyed()` does **not** include
  `AnyKey` fallbacks (MEDI `GetKeyedServices(AnyKey)` parity). Resolving a single service
  **with** the `AnyKey` sentinel is invalid (`get_keyed(any_key())` → `Err`;
  `is_registered_keyed(any_key())` → `false`). `AnyKey` registrations are not eagerly
  initialized/validated at build (they need a requested key); async resolution of a
  fallback falls back to the synchronous path.
  - **Validation cache prefill**: when a sync singleton depends on an `AnyKey` fallback
    during build-time validation, the resolved singleton is written into the
    `(registration, requested key)` cache, so runtime resolution with the same key reuses
    the **same instance** (single initialization; multiple consumers share one
    construction — no double construction). It is not re-created for that key.
  - **Owned resolution**: `get_keyed_owned` / `try_get_keyed_owned` for an `AnyKey`
    scoped/transient fallback is allowed at the strict root (same caller-owned exception as
    exact-key owned; the instance is not cached or tracked, while its transitive
    dependencies still obey scope validation).
  - **Nested keys**: an `AnyKey` factory may nested-resolve a **different** requested key
    (legal, no false cycle); only re-entering the same registration **with the same
    requested key** is a dependency cycle (`CircularDependency`).
  - **Key access**: the factory receives `&ServiceKey`, which exposes `Debug` / `Display`
    / `Eq` / `Hash` (the erased representation is private). There is intentionally **no**
    typed downcast accessor; branch with equality/hash or capture type information in the
    closure.
- **Enumeration and async services**: `get_all` / `get_all_keyed` / `#[inject]
  Vec<Arc<T>>` now include **already-initialized** async singletons (cached by
  `build_async`). Synchronously enumerating an **uninitialized** async service yields
  `RdiError::AsyncServiceRequiresGetAsync`: `get_all` / `get_all_keyed` (infallible
  signatures) panic with a typed `RdiError` payload for async singleton/transient
  services (an async **scoped** service enumerated at the root is instead omitted by
  strict scope validation; inside a `Scope` it panics), while `get_all_any_result` /
  `Vec<Arc<T>>` injection return `Err` for any uninitialized async service. Resolve
  async scoped/transient services item by item with `get_async`.
- **String-key footgun**: `ServiceKey::new("x")` is a `&str`-typed key and is **not**
  equal to the `String` key stored by `keyed_singleton("x", …)`. For string keys use
  `"x".into()` / `ServiceKey::from("x")`; `ServiceKey::new` is only for typed
  (`&'static str` / enum / integer) keys.
- **`AsyncFactoryContext` for cross-task async cycle detection**:
  `ServiceProvider::async_context()` / `Scope::async_context()` return a `Clone` handle
  (`get_async`, `get_keyed_async`, `provider()`). Capture it inside an async factory and
  move clones into `tokio::spawn`-like tasks: self/cross cycles across tasks return
  `RdiError::CircularDependency` instead of dead-locking. Async cycle detection is now
  **chain-based**: each `get_async`/`get_keyed_async` future captures the current
  resolution chain when constructed (`.await` usage unchanged), covering direct
  self/cross recursion and context-propagated cross-task recursion without misreporting
  legitimate concurrent resolution.
- **Diagnostics**: `RdiError::SyncSingletonDependsOnAsyncService { singleton,
  async_service }` for a synchronous singleton depending on an async service at build
  time, `RdiError::AsyncServiceRequiresGetAsync { service }` for synchronous resolution of
  an async service, and `RdiError::is_circular()` to recognise both a direct
  `CircularDependency` and one wrapped in a `FactoryPanic` payload. `is_circular()` is a
  **heuristic** (it also matches a `FactoryPanic` payload containing the text
  `CircularDependency`, so custom payloads can false-positive); prefer matching the typed
  variant when the exact error shape matters.
- **Engineering/security baseline**: MSRV **1.95** (`rust-version` in `Cargo.toml`,
  verified in CI); new CI workflow (`.github/workflows/ci.yml`) runs
  ubuntu + windows × Rust 1.95/stable tests, with fmt / clippy `-D warnings` / rustdoc
  `-D warnings` on stable and a RustSec `cargo audit` job; both crates opt into
  `#![forbid(unsafe_code)]`; new `SECURITY.md` documenting vulnerability reporting,
  supported versions and security design/boundaries.

#### Changed
- Keyed APIs (`keyed_*` builders, `get_keyed*`, `is_registered_keyed`, and the matching
  `Scope` / `ServiceProviderWrapper` methods) now take `impl Into<ServiceKey>`; existing
  `&str` / `String` calls are source-compatible. Explicit function pointers or type
  annotations over keyed APIs may need adjustment.
- `get_async` / `get_keyed_async` now return `impl Future` instead of `async fn` (the
  captured resolution chain is built at call time); `.await` usage is unchanged.
- Async cycle detection upgraded from "single poll" markers to a per-resolution chain
  (including `AsyncFactoryContext` across tasks); self/cross cycles still return
  `CircularDependency` and legitimate concurrency is still not misreported.
- `AnyKey` fallbacks add "cache per (registration, requested key)" and "not part of eager
  initialization/enumeration" semantics.

#### Fixed
- Sync-singleton → async dependency is now reported as a precise
  `SyncSingletonDependsOnAsyncService` diagnostic (instead of a generic panic/typed error),
  with hints to make the dependent async or resolve lazily via `ScopeFactory` /
  `get_async`.
- Synchronous resolution of an uninitialized async service now reports
  `AsyncServiceRequiresGetAsync` (previously described as
  `AsyncServiceRequiresBuildAsync`); `build()` still reports
  `AsyncServiceRequiresBuildAsync` for collections containing async registrations.
- Cross-task async dependency cycles no longer rely on single-poll markers: propagate an
  `AsyncFactoryContext` into detached tasks to get `CircularDependency` instead of a
  possible deadlock.

#### Migration from 0.8.x
- Source-compatible for typical code: string keyed calls and `.await` on async resolution
  keep working. Recompile against MSRV 1.95.
- If you pass keyed APIs as function pointers or annotate their parameter types, update
  them for the new `impl Into<ServiceKey>` signatures. `RdiError` remains
  `#[non_exhaustive]`; the new variants need no match changes.
- Async factories that resolve services from detached tasks should capture
  `sp.async_context()` and use the context (otherwise cross-task cycles may still
  deadlock). Sync singletons must not depend on async services; make the dependent async
  or resolve lazily.

#### Notes
- **No `try_get_all` series was added on purpose.** The API surface stays at
  `get_all` / `get_all_keyed` (infallible signatures; MEDI enumeration parity), with
  `get_optional` / `try_get` / `try_get_keyed` for probing. Adding fallible enumeration
  would duplicate `get_all` without a clear MEDI counterpart.

### 简体中文

#### 新增
- **类型化服务键（`ServiceKey`）**：keyed 注册/解析现在接受 `impl Into<ServiceKey>`。
  `ServiceKey::new(value)` 可包装任意 `Any + Send + Sync + Eq + Hash + Debug` 值
  （enum、整数等）；`From<&str>` / `From<String>` / `From<&String>` 使字符串键源码兼容。
  `ServiceKey::any_key()` / `KeyedService::any_key()` 提供 `AnyKey` 哨兵，
  `ServiceKey::is_any()` 可识别。键相等包含值的类型
  （`ServiceKey::new(1u32) != ServiceKey::from("1")`）。
- **`AnyKey` 兜底注册（对齐 MEDI `KeyedService.AnyKey`）**：
  `keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`。工厂签名接收被请求的
  key（`Fn(&dyn IServiceResolver, &ServiceKey) -> Arc<T>`）。精确 key 注册优先，否则使用
  兜底；singleton/scoped 的兜底实例**按 (服务注册/类型, 被请求 key) 分别缓存**（同一注册 + 同一
  key 复用、不同 key 或不同 AnyKey 注册各自独立；scoped 为每 scope 每 (注册, key) 一份），
  transient 每次解析新建。
  `get_all_keyed()` **不包含** `AnyKey` 兜底注册（对齐 MEDI
  `GetKeyedServices(AnyKey)`）。用 `AnyKey` 哨兵做**单值解析**非法
  （`get_keyed(any_key())` → `Err`；`is_registered_keyed(any_key())` → `false`）。
  `AnyKey` 注册在 build 期不做 eager 初始化/单独校验（需要请求 key）；异步解析兜底会回退
  到同步路径。
  - **validation 预填充缓存**：build 期同步 singleton 依赖 `AnyKey` 兜底并按某请求 key
    解析时，该 singleton 实例会写入 **(注册, 请求 key) 缓存**；运行时同一注册 + 同一请求 key
    复用**同一实例**（单次初始化；多消费者共享一次构造，不会双构建）。
  - **owned 解析**：`AnyKey` 兜底（scoped/transient）的 `get_keyed_owned` /
    `try_get_keyed_owned` 在 strict root 下**允许**（与 exact-key owned 相同的调用方持有
    例外；不进缓存/不跟踪，但其传递依赖仍受 scope 校验）。
  - **嵌套 key**：`AnyKey` 工厂内嵌套解析**不同请求 key** 合法（不会误报环）；只有同一注册
    + 同一请求 key 的重入才是环（`CircularDependency`）。
  - **key 读取**：工厂接收 `&ServiceKey`，可 `Debug` / `Display` / `Eq` / `Hash`（擦除表示
    私有，**有意不提供** typed downcast 访问器）；需要按 key 分支时用相等/hash，或在闭包中
    自行捕获类型信息。
- **枚举与 async 服务**：`get_all` / `get_all_keyed` / `#[inject] Vec<Arc<T>>` 现在**包含
  已初始化**的 async singleton（`build_async` 已构建并缓存）。同步枚举**未初始化**的
  async 服务会得到 `RdiError::AsyncServiceRequiresGetAsync`：`get_all`/`get_all_keyed`
  （签名不可失败）对 async singleton/transient 以 typed `RdiError` payload panic；async
  **scoped** 在 root provider 上会被严格 scope 校验静默省略（在 `Scope` 内枚举才 panic）；
  `get_all_any_result` / `Vec<Arc<T>>` 注入对任何未初始化 async 服务返回 `Err`。
  async scoped/transient 请用 `get_async` 逐项解析。
- **字符串 key 易错点**：`ServiceKey::new("x")` 是 `&str` typed key，与
  `keyed_singleton("x", …)` 存储的 `String` key **不相等**。字符串键请用 `"x".into()` /
  `ServiceKey::from("x")`；`ServiceKey::new` 只用于有意使用 typed（`&'static str` / enum /
  整数）键的场景。
- **`AsyncFactoryContext`（跨任务异步环检测）**：
  `ServiceProvider::async_context()` / `Scope::async_context()` 返回可 `Clone` 的句柄
  （`get_async`、`get_keyed_async`、`provider()`）。在 async 工厂内捕获后 clone 进
  `tokio::spawn` 等任务，自/交叉环会返回 `RdiError::CircularDependency` 而不是死锁。
  async 环检测升级为**链式**：`get_async`/`get_keyed_async` 返回的 future 在构造时捕获当前
  解析链（`.await` 用法不变），覆盖自引用/交叉递归与 ctx 跨任务递归，且不误报合法并发。
- **诊断**：`RdiError::SyncSingletonDependsOnAsyncService { singleton, async_service }`
  （build 期同步 singleton 依赖 async 服务）、
  `RdiError::AsyncServiceRequiresGetAsync { service }`（同步解析 async 服务）、
  `RdiError::is_circular()`（识别直接 `CircularDependency` 与 `FactoryPanic` payload 中
  包裹的环）。`is_circular()` 是**启发式**（`FactoryPanic` payload 含该文本也会命中，
  用户自定义 payload 可能误判）；需要精确错误形态时优先匹配 typed 变体。
- **工程/安全基线**：MSRV **1.95**（`Cargo.toml` 的 `rust-version`，CI 实测）；新增 CI
  workflow（`.github/workflows/ci.yml`）：ubuntu + windows × Rust 1.95/stable 跑测试，
  stable 跑 fmt / clippy `-D warnings` / rustdoc `-D warnings`，另有 RustSec `cargo audit`
  job；两个 crate 均 `#![forbid(unsafe_code)]`；新增 `SECURITY.md`（漏洞上报、支持版本、
  安全设计与边界）。

#### 变更
- keyed API（`keyed_*` 注册、`get_keyed*`、`is_registered_keyed` 及 `Scope` /
  `ServiceProviderWrapper` 同名方法）改为接受 `impl Into<ServiceKey>`；现有 `&str` /
  `String` 调用源码兼容。若把 keyed API 当作函数指针或显式标注参数类型，可能需要调整。
- `get_async` / `get_keyed_async` 由 `async fn` 改为返回 `impl Future`（在调用时捕获解析
  链）；`.await` 用法不变。
- async 环检测由"单次 poll"标记升级为**解析链**（含 `AsyncFactoryContext` 跨任务）；
  自引用/交叉环仍返回 `CircularDependency`，合法并发仍不误报。
- **`AnyKey` 兜底**引入"按 (服务注册/类型, 请求 key) 缓存"与"不参与 eager 初始化/枚举"语义。

#### 修复
- 同步 singleton 依赖 async 服务现在给出精确
  `SyncSingletonDependsOnAsyncService` 诊断（而非泛化 panic/typed 错误），并提示把依赖方
  改为 async 或在方法内用 `ScopeFactory` / `get_async` 延迟解析。
- 同步解析未初始化的 async 服务现在报 `AsyncServiceRequiresGetAsync`（此前文档常写成
  `AsyncServiceRequiresBuildAsync`）；含 async 注册的集合调用 `build()` 仍报
  `AsyncServiceRequiresBuildAsync`。
- 跨任务 async 依赖环不再依赖单 poll 标记：把 `AsyncFactoryContext` 传入脱离任务可获得
  `CircularDependency`，而不是潜在死锁。

#### 从 0.8.x 迁移
- 典型代码源码兼容：字符串 keyed 调用与 `.await` 异步解析保持可用；按 MSRV 1.95 重新编译。
- 若把 keyed API 作为函数指针传递或标注其参数类型，请按新的 `impl Into<ServiceKey>` 签名
  调整。`RdiError` 仍为 `#[non_exhaustive]`，新变体无需改 match 结构。
- 在脱离任务中解析服务的 async 工厂应捕获 `sp.async_context()` 并经 ctx 解析（否则跨任务环
  仍可能死锁）。同步 singleton 不能依赖 async 服务；把依赖方改为 async 或延迟解析。

#### 说明
- **刻意未新增 `try_get_all` 系列**：API 面保持 `get_all` / `get_all_keyed`（签名不可失败，
  对齐 MEDI 枚举语义），探测用 `get_optional` / `try_get` / `try_get_keyed`。增加可失败枚举
  会与 `get_all` 重复且缺少清晰 MEDI 对应。

## [0.8.1] - 2026-11-11

> **English** · **简体中文**

### English

#### Fixed
- **Async singleton initialization is dependency-first and registration-order independent**:
  `build_async` now recursively initializes an async singleton's dependencies before the
  dependent, regardless of registration order. 0.8.0 required dependencies to be registered
  before dependents and returned a `FactoryPanic` otherwise; that restriction is gone.
  Recursive dependency initialization is single-flight and cycle-safe — a self/cross
  dependency cycle returns `RdiError::CircularDependency` instead of dead-locking.
- Async singleton initialization remains eager and fail-fast: when `build_async` returns, all
  async singletons are initialized (initialization is not deferred to the first `get_async`).

#### Behavior note
- Async singleton creation order is now a deterministic **dependency-first topological order**;
  factory side effects may observe dependency-before-dependent instead of registration order.

#### Migration from 0.8.0
- None required — previously failing valid configurations now build; no API change (patch
  release). If a test asserted the 0.8.0 `FactoryPanic` for a later-registered dependency,
  update it to expect successful initialization.

### 简体中文

#### 修复
- **async singleton 改为依赖驱动初始化，注册顺序无关**：`build_async` 会先递归初始化 async
  singleton 的依赖，与注册先后无关。0.8.0 要求依赖先注册、否则返回 `FactoryPanic`；该限制
  已移除。递归初始化为单飞（single-flight）且环安全——自引用/交叉依赖返回
  `RdiError::CircularDependency`，而不是死锁。
- async singleton 仍为 eager 且 fail-fast：`build_async` 返回时所有 async singleton 均已
  初始化（不会推迟到首次 `get_async` 才初始化）。

#### 行为说明
- async singleton 的创建顺序现在是确定性的**依赖优先拓扑序**；工厂副作用可能观察到
  "依赖先于依赖方"，而不再按注册顺序。

#### 从 0.8.0 迁移
- 无需迁移——原先会失败的合法配置现在可正常构建；无 API 变更（patch 版本）。若测试断言了
  0.8.0 对"后注册依赖"返回 `FactoryPanic`，请改为断言初始化成功。

## [0.8.0] - 2026-11-11

> **English** · **简体中文**

### English

#### Breaking changes
- **Last-registration-wins resolution (MEDI parity)**: `get` / `get_optional` /
  `get_owned` / `get_keyed*` / `get_async` now return the **last** registration of a
  service (same key for keyed registrations). Previously the first registration won.
  `try_add*` keeps the MEDI `TryAdd` meaning: add only if not already registered.
- **`get_all` excludes keyed registrations (MEDI parity)**: `get_all::<T>()` and
  `#[inject] Vec<Arc<T>>` now return only **non-keyed** registrations, in registration
  order, matching MEDI `GetServices<T>()`. Use the new `get_all_keyed::<T>()` for keyed
  implementations.
- **Strict scope validation by default**: resolving a Scoped service from the root
  `ServiceProvider` (directly or through a transient chain) returns
  `Err(RdiError::ScopedFromRootProvider { service })`. APIs that
  cannot return an error (`get_optional`, `get_all`, `get_all_keyed`, `try_*`) treat the
  service as unresolvable (`None`/empty). Resolving inside `provider.scope()` is
  unaffected. `get_owned` / `try_get_owned` (and keyed owned) are the **exception**:
  they may resolve a Scoped service from the root — the instance is caller-owned, never
  cached or tracked — while Scoped dependencies resolved inside its constructor still
  obey validation. This matches MEDI/ASP.NET Core development `ValidateScopes = true`; a bare
  MEDI `BuildServiceProvider()` defaults to `false`. Opt back into the 0.7.x root-scope
  behavior with `build_with(ServiceProviderOptions::lenient())`
  (`validate_scopes: false`), where the root provider resolves and caches Scoped
  services as root-local singletons.
- **Dispose contracts**: `Scope` now tracks the Scoped instances created in it; its
  `dispose()`/`Drop` releases them in **reverse creation order**, then runs
  `register_disposable` cleanup closures in **reverse registration order**. After
  disposal, resolution returns `Err(RdiError::ScopeDisposed)` (non-fallible APIs return
  `None`/empty). New `ServiceProvider::dispose()` releases all singletons and root-scoped
  instances in reverse creation order, runs cleanups in reverse order, clears the named
  registry, and marks the provider disposed; afterwards resolution returns
  `Err(RdiError::ProviderDisposed)`. Both `dispose()` methods are idempotent. `get_owned`
  instances are caller-owned and never enter container tracking/disposal.

#### Added
- `ServiceProviderOptions` (crate root): `#[non_exhaustive]`, field
  `pub validate_scopes: bool`, `Default` = strict (`true`), `new()`,
  `.with_validate_scopes(bool)`, convenience `lenient()` (= `false`).
- `ServiceCollection::build_with(options)` / `build_async_with(options)`; `build()` and
  `build_async()` are equivalent to the `Default` (strict) variants.
- New `RdiError` variants `ScopedFromRootProvider { service }`, `ProviderDisposed`,
  `ScopeDisposed`; `RdiError` is now `#[non_exhaustive]`.
- `ServiceProvider::dispose()` and `Scope::dispose()` (idempotent; `Scope::Drop` still
  auto-disposes).
- `is_registered::<T>()` / `is_registered_keyed::<T>(key)` on `ServiceProvider`, `Scope`
  and `ServiceProviderWrapper`.
- `get_all_keyed::<T>()` on `ServiceProvider`, `Scope` and `ServiceProviderWrapper`;
  `IServiceResolver::get_all_keyed_any`.
- `get_async` / `get_keyed_async` now fall back to the synchronous path for services
  registered with the synchronous `scoped` / `transient` builders (previously
  `ServiceNotFound`).
- Async Scoped/Transient resolution detects self/cross recursion per-poll and returns
  `RdiError::CircularDependency` instead of dead-locking on the per-key async cache lock;
  legitimate concurrent resolution is not misreported. *(0.9.0 supersedes the per-poll
  scheme with chain-based detection, including cross-task via `AsyncFactoryContext`.)*
- New infallible trait helpers `IServiceResolver::try_get_any_result` /
  `try_get_keyed_any_result` (used by `try_get` / `try_get_keyed`, so strict-root scope
  violations and disposed containers return `None` instead of panicking); plus
  `get_all_keyed_any` / `get_all_any_result`.
- `build_async` initializes async singletons in registration order; resolving a
  not-yet-initialized async singleton reports an actionable `FactoryPanic` (register
  dependencies before their dependents). *(Superseded by 0.8.1: initialization is now
  dependency-first and registration-order independent.)*

#### Fixed
- Registration-order semantics now match the documentation and MEDI: single-service
  resolution is last-registration-wins (0.7.x implementations resolved first-registration
  despite documenting otherwise in places).
- Singleton creation order during `build()` is now deterministic (`HashMap` iteration
  order no longer matters), so construction and disposal order are reproducible.
  *(0.8.1+: async singletons follow a dependency-first topological order instead of
  registration order.)*
- `try_add*` semantics clarified and documented: "only add if absent", unaffected by the
  last-registration rule.
- Dispose ordering is deterministic and documented (reverse creation order for
  instances, reverse registration order for cleanup closures).

#### Migration from 0.7.x
- **Root Scoped resolution**: replace `provider.get::<Scoped>()` with
  `provider.scope()` + `scope.get::<Scoped>()`. `get_owned` / `try_get_owned` remain
  callable for Scoped services at the root (caller-owned, not cached); only their
  constructor's Scoped dependencies obey validation. If you truly need the old root-scope
  behavior, build with `ServiceCollection::build_with(ServiceProviderOptions::lenient())`.
- **First-registration reliance**: code that registered a default first and relied on it
  winning must switch to `try_add*` (add only if absent) or reorder registrations so the
  desired implementation is registered last.
- **Keyed multi-implementation collection**: replace `get_all()` over keyed registrations
  with `get_all_keyed::<T>()` (or register broadcast targets as non-keyed).
- **Dispose**: call `provider.dispose()` on application shutdown; never resolve from a
  provider/scope after disposal (typed `ProviderDisposed` / `ScopeDisposed` now).
- **Error matching**: add a wildcard arm where `RdiError` is matched exhaustively —
  `RdiError` is now `#[non_exhaustive]`.

### 简体中文

#### 破坏性变更
- **单服务解析改为"最后注册优先"（对齐 MEDI）**：`get` / `get_optional` / `get_owned` /
  `get_keyed*` / `get_async` 返回该服务**最后注册**的实现（keyed 同 key 也是后注册生效）。
  0.7.x 为"先注册优先"。`try_add*` 保持 MEDI `TryAdd` 语义：未注册才添加。
- **`get_all` 不再包含 keyed 注册（对齐 MEDI）**：`get_all::<T>()` 与
  `#[inject] Vec<Arc<T>>` 只返回**非 keyed** 注册（按注册顺序），对齐 MEDI
  `GetServices<T>()`；keyed 实现改用新增的 `get_all_keyed::<T>()`。
- **默认严格 scope 校验**：从 root `ServiceProvider` 解析 Scoped 服务（直接、或经 Transient
  链间接）返回 `Err(RdiError::ScopedFromRootProvider { service })`；不可返回错误的 API
  （`get_optional` / `get_all` / `get_all_keyed` / `try_*`）视为不可解析（`None`/空）。
  在 `provider.scope()` 内解析不受影响。**例外**：`get_owned` / `try_get_owned`
  （含 keyed owned）允许从 root 解析 Scoped —— 实例归调用方、不进缓存/不被跟踪，
  但其构造函数内部解析的 Scoped 依赖仍受校验。默认严格对应 MEDI/ASP.NET Core 开发环境的
  `ValidateScopes = true`；裸 MEDI `BuildServiceProvider()` 默认关闭。需要 0.7.x 的
  root-scope 旧行为时用 `build_with(ServiceProviderOptions::lenient())`
  （`validate_scopes: false`），root 会直接解析 Scoped 并缓存为根内单例。
- **dispose 契约**：`Scope` 现在跟踪该 scope 创建的 Scoped 实例，`dispose()`/Drop 按
  **创建逆序**释放实例，再按**注册逆序**执行 `register_disposable` 清理闭包；dispose
  后解析返回 `Err(RdiError::ScopeDisposed)`（不可返回错误的 API 为 `None`/空）。新增
  `ServiceProvider::dispose()`：按创建逆序释放全部 singleton 与 root scoped 实例、逆序
  执行清理、清空 named 注册并标记 provider 已释放，之后解析返回
  `Err(RdiError::ProviderDisposed)`。两个 `dispose()` 均幂等。`get_owned` 实例归调用方
  所有，不进入容器跟踪/释放。

#### 新增
- `ServiceProviderOptions`（crate 根导出）：`#[non_exhaustive]`，字段
  `pub validate_scopes: bool`，`Default` 为严格（`true`），`new()`、
  `.with_validate_scopes(bool)`、便捷 `lenient()`（= `false`）。
- `ServiceCollection::build_with(options)` / `build_async_with(options)`；`build()` 与
  `build_async()` 等价于 `Default`（严格）版本。
- `RdiError` 新变体 `ScopedFromRootProvider { service }`、`ProviderDisposed`、
  `ScopeDisposed`；`RdiError` 标记 `#[non_exhaustive]`。
- `ServiceProvider::dispose()` 与 `Scope::dispose()`（幂等；`Scope` 的 Drop 仍自动 dispose）。
- `ServiceProvider` / `Scope` / `ServiceProviderWrapper` 新增
  `is_registered::<T>()` / `is_registered_keyed::<T>(key)`。
- `ServiceProvider` / `Scope` / `ServiceProviderWrapper` 新增
  `get_all_keyed::<T>()`；`IServiceResolver::get_all_keyed_any`。
- **`get_async` / `get_keyed_async` 回退同步路径**：对用同步 `scoped` / `transient`
  注册的服务，异步解析会回退到同步解析路径（此前返回 `ServiceNotFound`）。
- **async 递归检测**：Scoped/Transient 的异步解析按"单次 poll"检测自引用/交叉递归，
  返回 `RdiError::CircularDependency`，而不是在 per-key async 缓存锁上死锁；合法的并发
  解析不会被误报。*（0.9.0 已升级为解析链检测，跨任务经 `AsyncFactoryContext`。）*
- **新增不失败的 trait 辅助方法**：`IServiceResolver::try_get_any_result` /
  `try_get_keyed_any_result`（供 `try_get` / `try_get_keyed` 使用，使 strict-root
  scope 校验与已 dispose 容器返回 `None` 而不 panic）；以及 `get_all_keyed_any` /
  `get_all_any_result`。
- **async singleton 初始化顺序（0.8.0，已于 0.8.1 修复）**：0.8.0 的 `build_async`
  对注册顺序敏感——解析尚未初始化的 async singleton 会返回可操作的 `FactoryPanic`
  （依赖需先注册/先初始化）。*0.8.1 已改为依赖驱动、注册顺序无关。*

#### 修复
- 注册顺序语义与文档、MEDI 对齐：单服务解析为最后注册优先（0.7.x 实现为首注册优先，
  与部分文档表述不符）。
- `build()` 期 singleton 创建顺序确定（0.8.0 时同步与 async 均按注册顺序；0.8.1 起
  async singleton 改为依赖优先拓扑序），不再依赖 `HashMap` 迭代顺序，构建与释放顺序可复现。
- `try_add*` 语义澄清并写入文档："未注册才添加"，不受最后注册优先规则影响。
- dispose 顺序确定并文档化（实例按创建逆序、清理闭包按注册逆序）。

#### 从 0.7.x 迁移
- **root 解析 Scoped**：`provider.get::<Scoped>()` 改为 `provider.scope()` +
  `scope.get::<Scoped>()`。`get_owned` / `try_get_owned` 对 Scoped 仍可从 root 调用
  （实例归调用方、不缓存），只有其构造函数内的 Scoped 依赖受校验。确需旧行为时用
  `ServiceCollection::build_with(ServiceProviderOptions::lenient())`。
- **依赖首注册优先**：先注册默认实现并依赖其生效的代码，改用 `try_add*`（未注册才添加）
  或调整注册顺序，让期望的实现最后注册。
- **keyed 多实现收集**：用 `get_all()` 收集 keyed 注册的代码改为 `get_all_keyed::<T>()`
  （或把广播目标注册为非 keyed）。
- **释放**：应用关闭时调用 `provider.dispose()`；dispose 后不要再解析（类型化
  `ProviderDisposed` / `ScopeDisposed`）。
- **错误匹配**：穷举匹配 `RdiError` 的地方补 `_ =>` 分支——`RdiError` 已标记
  `#[non_exhaustive]`。

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
- **Constructor-time resolution is rejected**: resolving services through an
  injected provider *inside* a build-time constructor returns a typed
  `FactoryPanic` instead of silently building dependencies outside the
  validation graph. Store the handle and resolve lazily in methods.

#### Changed
- **Built-in `ScopeFactory` is weak-backed**: it holds `Weak<ServiceProvider>`
  instead of `Arc`, so a singleton may store the factory without creating a
  `container → singleton → container` reference cycle. The container must outlive
  services created from it; `create_scope()` panics after the provider is dropped.
- **Built-in `ScopeFactory` is a singleton**: one factory instance is cached per
  provider (MEDI parity); repeated `get::<dyn ScopeFactory>()` returns the same
  `Arc`.
- **`get_by_type_id` built-in fallback fixed**: the `ScopeFactory` / provider
  fallback is no longer short-circuited when the built-in type has no store
  entry.

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
- **构造函数内动态解析被拒绝**：在 build 期的构造函数里通过注入的 provider
  解析服务会返回类型化 `FactoryPanic`，而不是绕过校验图悄悄构建依赖；请保存
  句柄、在方法中延迟解析。

#### 变更
- **内置 `ScopeFactory` 改为弱引用**：持有 `Weak<ServiceProvider>`，singleton
  保存它不会形成 `容器 → singleton → 容器` 的 Arc 环；容器必须先于其创建的服务
  存活（provider 释放后 `create_scope()` panic）。
- **内置 `ScopeFactory` 为单例**：每个 provider 缓存一份实例（对齐 MEDI），
  多次 `get::<dyn ScopeFactory>()` 返回同一 `Arc`。
- **修复 `get_by_type_id` 内建回退**：内建类型在 store 无注册时不再被提前 `?`
  短路。

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

[0.10.0]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.10.0
[0.9.1]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.9.1
[0.9.0]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.9.0
[0.8.1]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.8.1
[0.8.0]: https://github.com/Rust-Framework/rust-dix/releases/tag/v0.8.0
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
