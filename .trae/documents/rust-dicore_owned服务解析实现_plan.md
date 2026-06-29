# rust-dicore Owned 服务解析 — 实现计划

> 本计划基于需求文档 `.trae/documents/rust-dicore_owned解析需求_plan.md`，并**综合 rust-dicore（LRDI）框架的设计初衷与目标**进行方案抉择。需求文档提供了"做什么"，框架设计哲学决定"怎么做最契合"。

---

## 一、Summary（摘要）

为 rust-dicore 增加 **owned 服务解析**能力：`get_owned::<T>() -> T`、`try_get_owned`、keyed 版本，以及 `#[derive(Inject)]` 宏对裸 `T` 字段的自动 owned 注入。

**方案路线**：方案 A — 复用现有 `Arc` 工厂 + `Arc::try_unwrap`，不新增 `OwnedServiceFactory` / `owned_factory` 字段。理由见第三节"框架设计初衷驱动的方案抉择"。

**当前实现状态**：核心代码（entry.rs / provider.rs / scope.rs / 宏 lib.rs）**已实现并编译通过**（上一会话 Tasks 1-5 完成，134 个既有测试通过）。本计划聚焦剩余两项：编写 `tests/owned.rs`（11 个测试）+ 全量 `cargo test` 验证。

---

## 二、Current State Analysis（现状分析）

### 2.1 框架设计初衷与目标（抉择基准）

rust-dicore（LRDI）是以 MEDI（Microsoft.Extensions.DependencyInjection）为蓝本的 Rust DI 框架，其核心设计目标：

| 设计维度 | 框架既有约定 | 来源 |
|---------|------------|------|
| Type erasure | `ServiceFactory = Arc<dyn Fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync>>`；具体类型 `Arc<Arc<T>>` 擦除为 `Arc<dyn Any>` | entry.rs |
| 三生命周期 | Singleton / Scoped / Transient，`ServiceLifetime` 枚举 | lifetime.rs |
| 缓存安全 | `LazyCache` + per-key `OnceLock`，杜绝 TOCTOU 竞态 | cache.rs, project_memory |
| 缓存范围约定 | **仅缓存 Singleton**（ValidationResolver 不缓存 Transient/Scoped，避免泄漏） | project_memory |
| 根即 root scope | ServiceProvider 本身即 MEDI root scope，Scoped 从根解析时缓存于 `root_scoped_cache` | provider.rs |
| 子 scope 隔离 | 子 Scope 拥有独立 `scoped_cache`，Transient 依赖 Scoped 时绑定到子 scope（非根） | scope.rs, project_memory |
| Captive 依赖检测 | Singleton 依赖 Scoped 在 build 阶段拒绝（`RdiError::SingletonDependsOnScoped`） | provider.rs, project_memory |
| 过程宏 | `#[derive(Inject)]` 生成 `__rdi_construct_<Name>(&dyn IServiceResolver) -> Arc<Name>` | lib.rs (macros) |

### 2.2 需求与框架既有架构的张力

需求文档提出两条路线：

- **方案 A（最小改动）**：复用现有 `Arc` 工厂，`get_owned` 调工厂得 `Arc<Arc<T>>` → downcast → `Arc::try_unwrap` → `T`
- **方案 B（完整 owned 注册）**：新增 `OwnedServiceFactory` 类型 + `ServiceEntry.owned_factory: Option<...>` + `scoped_owned`/`transient_owned` 注册方法

### 2.3 当前实现已落地（Tasks 1-5 已完成）

经代码核查确认，以下文件均已包含 owned 解析实现：

1. **`crates/rust-dicore/src/entry.rs`** — `IServiceResolver` trait 新增 6 方法：
   - `get_owned_any` / `get_keyed_owned_any`（默认 `None`，类型擦除入口，返回 `Arc<dyn Any + Send + Sync>`）
   - `get_owned` / `try_get_owned` / `get_keyed_owned` / `try_get_keyed_owned`（泛型默认实现，`Self: Sized` 约束，内部 downcast + `Arc::try_unwrap`）

2. **`crates/rust-dicore/src/provider.rs`** — ServiceProvider 新增 inherent `get_owned`/`try_get_owned`/`get_keyed_owned`/`try_get_keyed_owned` + `pub(crate) fn create_owned_any_by_entry`（Singleton→`None`，Scoped/Transient→直接调工厂绕过 `singleton_cache`/`root_scoped_cache`）；ValidationResolver 新增 `get_owned_any`/`get_keyed_owned_any`（captive + cycle 检测，**不缓存**）

3. **`crates/rust-dicore/src/scope.rs`** — Scope 新增 inherent `get_owned` 系列 + 私有 `create_owned_any_by_entry`（工厂用 `self`（Scope），保证 Scoped 依赖绑定到子 scope）

4. **`crates/rust-dicore-macros/src/lib.rs`** — 重构字段分类：`classify_field` → `FieldKind`（`Arc`/`OptionArc`/`Owned`/`OptionOwned`）+ 共享 `gen_field_init` helper + `owned_extract` 片段（downcast → clone inner Arc → `try_unwrap`）

### 2.4 尚未完成

- **Task 6**：`crates/rust-dicore/tests/owned.rs` **不存在** — 需新建，含 11 个测试
- **Task 7**：全量 `cargo test -p rust-dicore` 验证（含新测试 + 既有测试无回归）

---

## 三、框架设计初衷驱动的方案抉择

用户明确要求"综合框架设计初衷和目标为基础进行分析"。以下逐项说明框架哲学如何裁决需求文档的开放问题。

### 3.1 方案 A vs 方案 B → 选 A

**框架设计初衷裁决**：

1. **Type erasure 统一性**：框架核心是单一 `ServiceFactory -> Arc<dyn Any>` 路径，所有服务走同一工厂。方案 B 引入第二条 `OwnedServiceFactory` 路径，破坏统一性，使 `ServiceEntry` 出现"双工厂"概念，违背"单一工厂 + 类型擦除"哲学。
2. **`ServiceEntry` 简洁性**：现有 `ServiceEntry` 仅 5 字段。方案 B 加 `owned_factory: Option<...>` 使每个非 owned 注册都背负 `None` 字段，污染数据模型。
3. **`try_unwrap` 可行性已验证**：工厂每次调用创建**新** `Arc<T>`（refcount=1）；`extract` 后内层 `Arc<T>` refcount 回到 1；`try_unwrap` 成功。方案 A 无功能损失。
4. **向后兼容**：方案 A 零破坏现有 `get::<T>()` / `Arc<T>` 注册；方案 B 需同步改 `ServiceCollection`/`descriptor`/`store`。

**结论**：方案 A 完全契合框架"单一工厂 + type erasure"架构，方案 B 是过度设计。

### 3.2 Scoped owned 语义 → 绕过缓存每次 fresh

需求文档开放问题 1 提出"绕过缓存"vs"one-shot panic"。**框架设计初衷裁决**：

- **"仅缓存 Singleton"约定**（project_memory 工程约定）：owned 永不缓存，与"Transient/Scoped 不进 runtime cache"一致。
- **MEDI scope-shared 与 Rust `&mut` 别名规则冲突**：Scoped 的 MEDI 语义是"scope 内共享"，但 owned 要求独占 `&mut self`。若 owned Scoped 仍走缓存，同 scope 内两次 `get_owned` 返回同一实例，违背 owned 独占语义。绕过缓存是**唯一 sound** 选择。
- **DbContext 用例契合**：每请求一个 owned DbContext，`get_owned` 每次创建新实例恰好满足。

**结论**：`get_owned` 对 Scoped 绕过 `scoped_cache`/`root_scoped_cache`，每次调工厂 fresh。这与已实现的 `create_owned_any_by_entry`（Scoped/Transient 直接调工厂）一致。

### 3.3 Singleton owned → panic / try 返回 None

**框架设计初衷裁决**：

- Singleton 是**全局共享**实例，refcount 恒 >1（singleton_cache 持有），`try_unwrap` 必失败。
- `get_owned` 对 Singleton 直接 `None`（`create_owned_any_by_entry` Singleton 分支返回 `None`），不调工厂——避免无意义工厂执行。
- `get_owned::<Singleton>()` → panic（与 `get()` 未注册 panic 风格一致）；`try_get_owned::<Singleton>()` → `None`。

### 3.4 Trait bound → 保持 `T: Send + Sync + 'static`（非 `T: Send + 'static`）

需求文档建议放宽到 `T: Send + 'static`，但**框架设计初衷裁决保持 `Send + Sync`**：

- `ServiceFactory` 签名是 `Arc<dyn Fn(...) -> Arc<dyn Any + Send + Sync> + Send + Sync>`，返回值要求 `Send + Sync`。owned 解析复用同一工厂，无法绕过此约束。
- 放宽 bound 需另建非 `Sync` 工厂路径 → 等于方案 B 的双工厂，已被否决。
- 务实妥协：owned `T: Send + Sync + 'static` 仍满足 DbContext 等用例（DbContext 本就是 `Send + Sync`）。

### 3.5 ValidationResolver 不对称性（sound）

**框架设计初衷裁决**：ValidationResolver 的 `get_owned_any` 对 Singleton **也调工厂**（遍历依赖图检测 cycle/captive），而 runtime `get_owned_any` 对 Singleton 返回 `None`。

- **不对称是 sound 的**：validation 是**静态分析**（遍历图找环/找 captive），runtime 是**强制 owned 禁令**。
- validation 不缓存 owned 结果（符合"仅缓存 Singleton"约定，且 owned 永不缓存）。

### 3.6 Scope 内 owned 的依赖绑定

**框架设计初衷裁决**：Scope 的 `create_owned_any_by_entry` 工厂用 `self`（Scope）作 resolver，而非 `self.parent`。

- 与既有"Transient 在子 scope 内依赖 Scoped 应绑定到子 scope"fix 一致（project_memory 教训）。
- 保证 owned Transient handler 的 `Arc<ScopedCore>` 字段走 `get_any`（缓存于子 scope `scoped_cache`），而 handler 本身每次 fresh。

---

## 四、Proposed Changes（变更明细）

### 4.1 已完成（Tasks 1-5，仅供溯源，不再改动）

| 文件 | 变更 | 状态 |
|------|------|------|
| `crates/rust-dicore/src/entry.rs` | `IServiceResolver` 新增 6 owned 方法 | ✅ 已实现 |
| `crates/rust-dicore/src/provider.rs` | ServiceProvider inherent owned 方法 + `create_owned_any_by_entry`；ValidationResolver owned 方法 | ✅ 已实现 |
| `crates/rust-dicore/src/scope.rs` | Scope inherent owned 方法 + 私有 `create_owned_any_by_entry` | ✅ 已实现 |
| `crates/rust-dicore-macros/src/lib.rs` | `classify_field`/`FieldKind`/`gen_field_init`/`owned_extract` | ✅ 已实现 |

### 4.2 待完成 — Task 6：新建 `crates/rust-dicore/tests/owned.rs`

**文件**：`d:\GitCode\RF\rust-di\crates\rust-dicore\tests\owned.rs`（新建）

**文件头**：
```rust
//! Tests for owned service resolution (`get_owned::<T>() -> T`).
//!
//! Covers: Transient/Scoped/Singleton owned semantics, scope dependency
//! binding, keyed owned, #[derive(Inject)] owned/Option/keyed fields,
//! mixed handler &mut self e2e, captive dependency rejection.

#![allow(non_snake_case)]

mod common;
use rust_dicore::*;
use rust_dicore_macros::Inject;
use std::sync::Arc;
```

**11 个测试**（逐项设计）：

#### Test 1: `transient_get_owned_returns_fresh_owned`
- 注册 transient `MyService`（用 `common::MyService`，`value` 字段区分实例）
- `get_owned::<MyService>()` 两次 → 两个独立 owned 实例（`assert_ne!` 指针/值）
- 验证 `Arc::try_unwrap` 成功（owned 即 refcount=1）

#### Test 2: `scoped_get_owned_bypasses_cache`
- 注册 scoped `MyService`
- `get::<MyService>()` 两次 → `Arc::ptr_eq`（缓存复用）
- `get_owned::<MyService>()` 两次 → 不同实例（绕过 `root_scoped_cache`）
- 再 `get::<MyService>()` → 仍与首次 `get()` `ptr_eq`（owned 未污染缓存）

#### Test 3: `singleton_get_owned_panics`（`#[should_panic]`）
- 注册 singleton `MyService`
- `get_owned::<MyService>()` → panic（Singleton 共享，无法 owned）

#### Test 4: `singleton_try_get_owned_returns_none`
- 注册 singleton `MyService`
- `try_get_owned::<MyService>()` → `None`（不 panic）

#### Test 5: `try_get_owned_none_when_unregistered`
- 空集合 build
- `try_get_owned::<MyService>()` → `None`

#### Test 6: `scope_owned_transient_arc_scoped_dep_binds_to_child`
- 注册 scoped `ScopedCore`（atomic 计数区分实例）
- 注册 transient `OwnedHandler { core: Arc<ScopedCore> }`（Arc 字段走 `get_any`）
- `provider.scope()` 两个：scope1、scope2
- scope1 `get_owned::<OwnedHandler>()` 两次 → handler fresh（owned），但其 `core` 字段 `Arc::ptr_eq`（scope1 `scoped_cache` 复用）
- scope2 `get_owned::<OwnedHandler>()` → `core` 与 scope1 不 `ptr_eq`（子 scope 隔离）
- 验证 ScopedCore 工厂执行次数 = 2（scope1、scope2 各一次，owned 绕过不额外触发）

#### Test 7: `keyed_transient_get_owned`
- `keyed_transient("primary", |_| Arc::new(MyService{value:1}))`
- `get_keyed_owned::<MyService>("primary")` → owned，value=1

#### Test 8: `keyed_singleton_get_owned_panics`（`#[should_panic]`）
- `keyed_singleton("primary", ...)`
- `get_keyed_owned::<MyService>("primary")` → panic

#### Test 9: `derive_inject_owned_field`
- `#[derive(Inject)] struct MixedHandler { svc: common::MyService, logger: Arc<common::Logger> }`
- 手动注册 `.transient(|r| __rdi_construct_MixedHandler(r))` + singleton Logger
- `get_owned::<MixedHandler>()` → `svc` 可 `&mut`（owned），`logger` 是 `Arc`
- 验证 `svc.value` 可写（证明 owned）

#### Test 10: `derive_inject_option_owned_field_present` + `derive_inject_option_owned_field_absent`
- `#[derive(Inject)] struct OptHandler { maybe: Option<common::MyService> }`
- present: 注册 transient MyService → `maybe.is_some()`
- absent: 不注册 → `maybe.is_none()`（`Option<T>` 字段走 `try_get_owned` 路径，不 panic）

#### Test 11: `derive_inject_keyed_owned_field`
- `#[derive(Inject)] struct KeyedOwnedHandler { #[rdi(key="primary")] svc: common::MyService }`
- `keyed_transient("primary", ...)` + 手动注册 handler
- `get_owned::<KeyedOwnedHandler>()` → `svc.value` 正确

#### Test 12: `mixed_handler_mutable_self_e2e`
- 模拟 rust-ef 核心用例：`DbContext { count: u32 }` + `add(&mut self)` / `count(&self)` 方法 + `Arc<Logger>` 字段
- `#[derive(Inject)] struct BlogHandler { ctx: DbContext, logger: Arc<Logger> }`
- `get_owned::<BlogHandler>()` → `handler.ctx.add()`（`&mut self`，无锁无 unsafe）→ `count` 正确

#### Test 13: `captive_singleton_depends_on_scoped_owned_rejected`
- Singleton handler 含 owned Scoped 字段（`ctx: ScopedCore`，裸 T → `get_owned_any`）
- `build()` → `Err(RdiError::SingletonDependsOnScoped)`
- 验证 ValidationResolver 的 `get_owned_any` 仍触发 captive 检测（即使 owned 绕过缓存，依赖图遍历不绕过）

> 注：原计划 11 个测试，实际细化为 13 个（Option owned 拆为 present/absent，captive 独立成测）。测试编号沿用设计稿，数量微调以提升覆盖度。

### 4.3 待完成 — Task 7：全量验证

**命令**：
```powershell
cargo test -p rust-dicore
cargo test -p rust-dicore-macros
```

**预期**：
- 既有 134 个测试继续通过（无回归）
- 新增 `tests/owned.rs` 约 13 个测试全部通过
- 已知环境问题：lib.rs:61 doctest 在 Windows 沙箱下 "os error 5 拒绝访问"（执行期非编译期），与本次变更无关，可忽略

---

## 五、Assumptions & Decisions（假设与决策）

| 决策点 | 选择 | 依据 |
|--------|------|------|
| 方案路线 | 方案 A（复用 Arc 工厂 + `try_unwrap`） | 框架"单一工厂 + type erasure"哲学；`try_unwrap` 可行性已验证 |
| Scoped owned 语义 | 绕过缓存每次 fresh | "仅缓存 Singleton"约定 + owned 独占语义 + MEDI scope-shared 与 `&mut` 冲突 |
| Singleton owned | `get_owned` panic / `try_get_owned` 返 None | Singleton 共享 refcount>1，`try_unwrap` 必败 |
| Trait bound | `T: Send + Sync + 'static`（非 `Send + 'static`） | 复用现有 `ServiceFactory` 签名，避免双工厂 |
| ValidationResolver 不对称 | validation 调 Singleton 工厂（遍历图），runtime 拒绝 | 静态分析 vs 运行时禁令，sound |
| Scope owned 依赖绑定 | 工厂用 `self`（Scope） | 与"Transient 在子 scope 依赖 Scoped 绑定子 scope"fix 一致 |
| 计划范围 | 仅 rust-dicore + 宏 + 测试 | 下游 rust-ef / rust-webapp 另行处理 |
| 宏字段分类 | `Arc`/`OptionArc`/`Owned`/`OptionOwned` 四类 + 共享 `gen_field_init` | 消除 `expand_inject_derive`/`expand_inject_struct` 重复 |
| 构造函数签名 | 仍返回 `Arc<#name>`（不变） | handler 顶层 Arc 包，`get_owned::<Handler>()` `try_unwrap` 解包 |

---

## 六、Verification（验证步骤）

1. **编译验证**：`cargo build -p rust-dicore -p rust-dicore-macros` 无错误
2. **新测试验证**：`cargo test -p rust-dicore --test owned` 全绿
3. **回归验证**：`cargo test -p rust-dicore` 既有 134 测试无回归
4. **关键场景手验**：
   - Test 6（scope 依赖绑定）通过 → 证明 owned Transient 的 Arc<Scoped> 字段绑定子 scope
   - Test 12（`&mut self` e2e）通过 → 证明核心用例（DbContext 无锁无 unsafe）达成
   - Test 13（captive 拒绝）通过 → 证明 ValidationResolver `get_owned_any` 仍做 captive 检测

---

## 七、Implementation Order（执行顺序）

1. ✅ Task 1: entry.rs — `IServiceResolver` owned 方法（已完成）
2. ✅ Task 2: provider.rs — ServiceProvider owned 方法 + `create_owned_any_by_entry`（已完成）
3. ✅ Task 3: provider.rs — ValidationResolver owned 方法（已完成）
4. ✅ Task 4: scope.rs — Scope owned 方法（已完成）
5. ✅ Task 5: lib.rs (macros) — `classify_field`/`gen_field_init`/`owned_extract`（已完成）
6. ⏳ **Task 6: 新建 `tests/owned.rs`（13 个测试）** ← 当前聚焦
7. ⏳ **Task 7: `cargo test` 全量验证** ← 收尾
