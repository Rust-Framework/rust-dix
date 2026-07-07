# 迭代 2 & 3 执行计划：API 统一与架构完善

## Summary

迭代 1（P0 阻塞）已完成，184 个测试全部通过。本计划聚焦迭代 2（P1 API 统一与性能）和迭代 3（P2 架构完善），目标是消除冗余 API、统一返回类型、修复并发竞态、清理重叠 trait，使框架达到生产水平。

**核心原则**：拒绝冗余代码、拒绝妥协设计、拒绝兼容代码；架构简单清晰、逻辑严谨、功能全面；严格遵循软件架构设计原则（SRP/OCP/LSP/ISP/DIP）。

**迭代节奏**：
- 迭代 2：4 个任务（build 类型统一、MEDI 别名删除、Scope Drop 竞态修复、TypeId 直查）
- 迭代 3：5 个任务（ScopeFactory trait、冗余 trait 清理、RdiError context、API 对称化、tracing 集成）

---

## Current State Analysis（基于 Phase 1 探索）

### 关键代码现状

| 文件 | 关键点 | 行号 |
|------|--------|------|
| `provider.rs` | `new()` 返回 `Result<Self, RdiError>`，`self_weak` 未初始化 | L48-95 |
| `provider.rs` | `into_arc()` 是唯一初始化 `self_weak` 的同步路径 | L515-520 |
| `provider.rs` | `get_service`/`get_required_service`/`get_services`/`create_scope` MEDI 别名 | L522-542 |
| `provider.rs` | `try_get`/`try_get_keyed` 已用 `TypeId::of::<T>()` 直查 `store`，O(1) | L544-560 |
| `provider.rs` | `IServiceResolver::get_any` 走 `type_map.get(key)` 字符串路径 | L669-673 |
| `provider.rs` | `dispose_scoped()` 用 `mem::take` + `lock()`（阻塞式） | L220-226 |
| `collection.rs` | `build()` 返回 `Result<ServiceProvider, RdiError>` | L193-207 |
| `collection.rs` | `build_async()` 返回 `Result<Arc<ServiceProvider>, RdiError>` | L220-224 |
| `entry.rs` | `IServiceResolver::get<T>()` 默认实现走 `type_name::<T>()` 字符串路径 | L76-84 |
| `scope.rs` | `Scope::drop` 调用 `dispose()` → `dispose_scoped()` → `lock()` | L151-155 |
| `scope.rs` | `get_service`/`get_required_service`/`get_services` MEDI 别名 | L76-89 |
| `bridge.rs` | `IProvider` trait = `IServiceResolver` + 3 named 方法 | L22-26 |
| `service_locator.rs` | `IServiceLocator`（4 方法）+ `INamedRegistrar`（2 方法）与 `IProvider` 重叠 | L5-16 |
| `bridge.rs` | `ServiceLocatorBridge` 薄包装 `IProvider`，`impl_service_locator!` 宏 | L71-139 |
| `wrapper.rs` | 缺 `get_owned_any`/`get_keyed_owned_any` trait 实现 | L70-88 |
| `wrapper.rs` | 缺 `get_owned`/`get_async`/`get_keyed_async` inherent 方法 | — |
| `error.rs` | `ServiceNotFound(&'static str)` 无 context | L4-5 |
| `field_init.rs` | `provider` 字段 `.expect()` 消息提到 `into_arc()` | L88-91 |

### 关键洞察

1. **TypeId 直查实际收益低**：ServiceProvider 的 inherent `get`/`try_get`/`get_owned` 已是 O(1) TypeId 直查，trait 默认实现只对 ValidationResolver 等外部实现生效。任务 2.4 降级为代码清洁度改进。
2. **`into_arc()` 删除可行**：`new()` 内部可构造 `Arc<Self>` 并初始化 `self_weak`，与 `new_async()` 模式一致。
3. **`IServiceLocator`/`INamedRegistrar` 确实冗余**：`IProvider` 已统一覆盖解析 + named 注册，`ServiceLocatorBridge` 只是薄包装。
4. **Wrapper 不对称**：缺 owned/async 方法，违反 ISP（接口隔离原则）。

---

## Iteration 2: P1 API 统一与性能

### 2.1 统一 build() 返回类型为 `Arc<ServiceProvider>`

**问题**：`build()` 返回 `ServiceProvider`，`build_async()` 返回 `Arc<ServiceProvider>`。`build()` 结果需手动 `into_arc()` 才能用 `provider_arc()`，API 不对称。

**方案**：`build()` 也返回 `Arc<ServiceProvider>`，`new()` 内部初始化 `self_weak`。删除 `into_arc()` 方法。

**文件与改动**：

1. **`crates/core/src/provider.rs`** — `new()` 改为返回 `Result<Arc<Self>, RdiError>`
   ```rust
   pub(crate) fn new(store: ServiceStore) -> Result<Arc<Self>, RdiError> {
       // ... validation + type_map 构建 ...
       let sp_arc = Arc::new(Self {
           store: Arc::new(store),
           type_map: Arc::new(type_map),
           singleton_cache,
           root_scoped_cache: LazyCache::new(),
           named: Arc::new(RwLock::new(HashMap::new())),
           self_weak: OnceLock::new(),
           disposables: Mutex::new(Vec::new()),
       });
       sp_arc.self_weak.get_or_init(|| Arc::downgrade(&sp_arc));
       
       // Phase 2: eagerly execute singleton factories（用 sp_arc 作 resolver）
       let singleton_entries: Vec<(usize, ServiceFactory)> = /* ... */;
       for (ck, factory) in &singleton_entries {
           let resolver = sp_arc.as_ref() as &dyn IServiceResolver;
           let factory = factory.clone();
           sp_arc.singleton_cache.get_or_init_with(*ck, move || factory(resolver));
       }
       Ok(sp_arc)
   }
   ```

2. **`crates/core/src/provider.rs`** — 删除 `into_arc()` 方法（L510-520）

3. **`crates/core/src/collection.rs`** — `build()` 返回 `Result<Arc<ServiceProvider>, RdiError>`
   ```rust
   pub fn build(self) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
       // ... 构建 store ...
       crate::provider::ServiceProvider::new(s)  // 已返回 Arc<ServiceProvider>
   }
   ```

4. **`crates/macros/src/field_init.rs`** — 更新 `provider` 字段 `.expect()` 消息（L88-91）
   ```rust
   #fnm: resolver.provider_arc().expect(
       "ServiceProvider not available; build() returns Arc<ServiceProvider> which enables #[inject(provider)]"
   )
   ```

5. **全局搜索替换**：`into_arc()` 调用（约 8 处，主要在测试中）→ 直接用 `build()` 返回值
   - `crates/core/tests/provider_inject.rs` — 4 处 `.build().unwrap().into_arc()` → `.build().unwrap()`
   - `crates/core/src/scope.rs` 测试 — `Arc::new(...build().unwrap())` → `...build().unwrap()`（避免双重 Arc）
   - `crates/core/src/wrapper.rs` 测试 — 同上
   - 其他测试文件中的 `into_arc()` 调用

**影响评估**：
- 所有 `.build().unwrap()` 现在返回 `Arc<ServiceProvider>`
- `provider.scope()` 直接可用（已有 `self: &Arc<Self>` 签名）
- `provider.get()` 通过 `Arc::deref` 调用（已支持）
- 测试中 `Arc::new(ServiceCollection::new()...build().unwrap())` 需改为 `ServiceCollection::new()...build().unwrap()`（避免 `Arc<Arc<ServiceProvider>>`）

**验证**：全量测试通过。重点验证 `provider_inject_resolves_via_into_arc` 重命名为 `provider_inject_resolves_via_build`。

---

### 2.2 删除 MEDI 命名别名（错误处理统一）

**问题**：`get_service`/`get_required_service`/`get_services`/`create_scope` 是 MEDI 命名别名，与 Rust 习惯（`get`/`get_optional`/`get_all`/`scope`）冲突，增加 API 表面积。`get_required_service` 是 panic-based，与 `get()` Result-based 语义冲突。

**方案**：删除所有 MEDI 命名别名，统一用 Rust 习惯命名。

**文件与改动**：

1. **`crates/core/src/provider.rs`** — 删除 4 个方法（L522-542）
   - `create_scope()` — 删除（用 `scope()` 替代）
   - `get_service()` — 删除（用 `get_optional()` 替代）
   - `get_required_service()` — 删除（用 `get().unwrap()` 或 `get()` 替代）
   - `get_services()` — 删除（用 `get_all()` 替代）

2. **`crates/core/src/scope.rs`** — 删除 3 个方法（L75-89）
   - `get_service()` / `get_required_service()` / `get_services()`

3. **全局搜索替换**（约 22 处）：
   - `get_required_service::<T>()` → `get::<T>().unwrap()` 或 `get::<T>()?`
   - `get_service::<T>()` → `get_optional::<T>()`
   - `get_services::<T>()` → `get_all::<T>()`
   - `create_scope()` → `scope()`

**验证**：全量测试通过。`cargo clippy -- -D warnings` 无警告。

---

### 2.3 修复 Scope Drop 并发竞态

**问题**：`Scope::drop` → `dispose()` → `dispose_scoped()` → `mem::take(&mut *disposables.lock())`。若另一线程正在 `get::<ScopedX>()` 解析（工厂内 `register_disposable` 持有锁），`drop` 的 `lock()` 会阻塞；若 `drop` 与 `register` 交错，`mem::take` 后注册的 disposable 丢失。

**方案**：`dispose_scoped()` 改用 `try_lock`，锁不上时跳过（表示有并发解析正在进行）。`Drop` 作为安全网，不保证执行所有 disposable。

**文件与改动**：

1. **`crates/core/src/provider.rs`** — `dispose_scoped()` 改用 `try_lock`
   ```rust
   pub(crate) fn dispose_scoped(&self) {
       // try_lock 避免与并发 register_disposable 死锁。
       // 锁不上时跳过——并发解析仍在进行，disposable 留待下次 dispose 或 Drop。
       if let Ok(mut disposables) = self.disposables.try_lock() {
           let to_run = std::mem::take(&mut *disposables);
           drop(disposables); // 释放锁后再执行闭包（避免持锁调用用户代码）
           for cleanup in to_run {
               cleanup();
           }
       }
   }
   ```

2. **`crates/core/src/scope.rs`** — `Scope::dispose()` 文档更新，说明 Drop 是安全网
   ```rust
   /// Drop 时自动调用作为安全网。若 Drop 时有并发解析正在进行（锁竞争），
   /// 部分 disposable 可能延迟到下次 dispose 或被丢弃。
   /// 需要确定性清理时，应在 scope 结束前显式调用此方法。
   ```

**验证**：新增测试 `scope_drop_with_concurrent_resolution` — 多线程并发解析 + drop scope，验证无死锁。

---

### 2.4 TypeId 直查优化（代码清洁度）

**问题**：`IServiceResolver::get<T>()` 默认实现走 `type_name::<T>()` 字符串路径，与 ServiceProvider 的 inherent `get`（已用 TypeId 直查）不一致。虽然热路径已最优，但 trait 默认实现与 inherent 方法的不一致违反 DRY 原则。

**方案**：`IServiceResolver` 新增 `get_by_type_id` 默认方法（走字符串路径兼容外部实现），ServiceProvider 覆盖为 O(1) 直查。trait 默认 `get<T>()` 改用 `get_by_type_id`。

**文件与改动**：

1. **`crates/core/src/entry.rs`** — `IServiceResolver` 新增 `get_by_type_id` 并重写 `get<T>()` 默认实现
   ```rust
   pub trait IServiceResolver: Send + Sync {
       /// TypeId 直查（默认走字符串路径，ServiceProvider 覆盖为 O(1)）
       fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
           // 默认实现：TypeId → type_name 反查（无 std API 直接 TypeId→name）
           // 退化为 None，外部实现者按需覆盖
           None
       }
       
       fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
       where Self: Sized,
       {
           let tid = TypeId::of::<T>();
           let type_name = std::any::type_name::<T>();
           self.get_by_type_id(tid)
               .or_else(|| self.get_any(type_name))  // fallback 字符串路径
               .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
               .ok_or(RdiError::ServiceNotFound(type_name))
       }
       // try_get / get_keyed / try_get_keyed / get_all 同理改用 get_by_type_id
   }
   ```

2. **`crates/core/src/provider.rs`** — `IServiceResolver for ServiceProvider` 覆盖 `get_by_type_id`
   ```rust
   fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
       let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
       self.get_any_by_entry(entry)
   }
   ```
   - 同时覆盖 `get_keyed_by_type_id`（keyed 版本）

3. **`crates/core/src/scope.rs`** — `IServiceResolver for Scope` 覆盖 `get_by_type_id`（委托 scope_provider）

**技术约束**：`TypeId::of::<T>()` 支持 `?Sized`（含 `dyn Trait`），可用。但 `get_by_type_id` 默认实现无法做 TypeId→type_name 反查（无 std API），故默认返回 `None` 并 fallback 到 `get_any` 字符串路径。ServiceProvider 覆盖后走 O(1)。

**实际收益**：消除 trait 默认与 inherent 方法的不一致；ValidationResolver 等外部实现无变化（仍走字符串路径）。

**验证**：全量测试通过。性能无退化（热路径未变）。

---

## Iteration 3: P2 架构完善

### 3.1 引入 ScopeFactory trait

**问题**：`scope()` 直接挂在 `ServiceProvider` 上，无法注入"创建 scope 的能力"给 singleton 服务（如后台 worker 按需创建 request scope）。

**方案**：新增 `ScopeFactory` trait，`ServiceProvider` 实现。singleton 服务可注入 `Arc<dyn ScopeFactory>` 按需创建 scope。

**文件与改动**：

1. **`crates/core/src/scope.rs`** — 新增 `ScopeFactory` trait
   ```rust
   /// 工厂 trait：创建独立 scope。可注入 singleton 服务按需创建 scope。
   pub trait ScopeFactory: Send + Sync {
       fn create_scope(&self) -> Scope;
   }
   ```

2. **`crates/core/src/provider.rs`** — `ServiceProvider` 实现 `ScopeFactory`
   ```rust
   impl ScopeFactory for ServiceProvider {
       fn create_scope(&self) -> Scope {
           // 通过 self_weak 获取 Arc<Self>
           self.self_weak.get()
               .and_then(|w| w.upgrade())
               .map(|arc| Scope::new(arc))
               .expect("ScopeFactory::create_scope called before self_weak initialization")
       }
   }
   ```
   - 注：迭代 2.1 已在 `new()` 中初始化 `self_weak`，此处可安全 upgrade

3. **`crates/core/src/lib.rs`** — re-export `ScopeFactory`

4. **`crates/macros/src/field_init.rs`** — 支持 `#[inject(scope_factory)]` 字段注入 `Arc<dyn ScopeFactory>`
   - 或复用现有 trait 注入机制（`Arc<dyn ScopeFactory>` 走 `get_any(type_name)`）

**验证**：新增测试 `scope_factory_injected_into_singleton` — singleton 服务注入 `Arc<dyn ScopeFactory>`，按需创建 scope，验证 scope 独立性。

---

### 3.2 清理冗余 Trait

**问题**：`IServiceLocator`（4 方法）与 `IServiceResolver` 的 `get_any` 重叠；`INamedRegistrar`（2 方法）与 `IProvider` 的 `rdi_register_named`/`rdi_remove_named` 重叠；`ServiceLocatorBridge` 只是 `IProvider` 的薄包装，无附加价值。

**方案**：删除 `IServiceLocator`、`INamedRegistrar`、`ServiceLocatorBridge`、`impl_service_locator!` 宏。统一用 `IProvider`（已覆盖解析 + named 注册）。

**文件与改动**：

1. **删除 `crates/core/src/service_locator.rs`** — 整个文件删除

2. **`crates/core/src/bridge.rs`** — 删除 `ServiceLocatorBridge` 和 `impl_service_locator!` 宏，保留 `IProvider` trait 和 3 个 impl
   ```rust
   // 保留：
   pub trait IProvider: IServiceResolver {
       fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>>;
       fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>);
       fn rdi_remove_named(&self, name: &str);
   }
   impl IProvider for ServiceProvider { /* ... */ }
   impl IProvider for ServiceProviderWrapper { /* ... */ }
   impl IProvider for Scope { /* ... */ }
   // 删除：ServiceLocatorBridge、impl_service_locator!
   ```

3. **`crates/core/src/lib.rs`** — 更新 re-exports
   - 删除 `pub mod service_locator;`
   - 删除 `pub use service_locator::{IServiceLocator, INamedRegistrar};`
   - 删除 `pub use bridge::ServiceLocatorBridge;`
   - 保留 `pub use bridge::IProvider;`

4. **`crates/core/src/scope.rs`** — 删除 `rdi_register_named`/`rdi_remove_named` 的文档注释提及 `impl_service_locator!` 宏（L140-148），改为提及 `IProvider` trait

5. **`crates/core/src/wrapper.rs`** — 同上更新文档注释（L91-99）

6. **全局搜索**：`IServiceLocator`/`INamedRegistrar`/`ServiceLocatorBridge`/`impl_service_locator` 的使用点（测试 + 文档），替换或删除

**影响评估**：
- 外部使用 `IServiceLocator` 的代码需迁移到 `IServiceResolver`（功能等价）
- 外部使用 `INamedRegistrar` 的代码需迁移到 `IProvider`
- `ServiceLocatorBridge` 用户需直接用 `Arc<dyn IProvider>`（更简单）

**验证**：全量测试通过。`cargo clippy -- -D warnings` 无警告。

---

### 3.3 RdiError 增强 context

**问题**：`ServiceNotFound(&'static str)` 只有类型名，无解析链上下文，调试困难。

**方案**：`ServiceNotFound` 改为结构体变体，增加 `context: Option<String>` 字段。

**文件与改动**：

1. **`crates/core/src/error.rs`** — 重构 `ServiceNotFound`
   ```rust
   #[derive(Error, Debug, Clone)]
   pub enum RdiError {
       #[error("service not registered: {ty}{}", context.as_ref().map(|c| format!(" (while resolving {c})")).unwrap_or_default())]
       ServiceNotFound { ty: &'static str, context: Option<String> },
       // KeyedServiceNotFound 同理加 context
       // 其他变体不变
   }
   ```

2. **全局搜索替换**：`RdiError::ServiceNotFound("xxx")` → `RdiError::ServiceNotFound { ty: "xxx", context: None }`
   - `crates/core/src/entry.rs` — trait 默认实现（约 4 处）
   - `crates/core/src/provider.rs` — inherent 方法（约 5 处）
   - `crates/core/src/validation.rs` — 验证错误
   - 测试中的 `match RdiError::ServiceNotFound(ty)` 模式匹配更新

3. **`crates/core/src/provider.rs`** — `get_any_by_entry` 等方法在解析失败时构造带 context 的错误（可选增强）
   - 暂不实现深层 context 传播（需重构返回类型为 `Result`，影响大），仅保留 `context: None` 占位

**验证**：全量测试通过。错误消息格式更新。

---

### 3.4 tracing 集成（可选）

**问题**：解析失败、工厂 panic、scope dispose 全部无结构化日志，生产环境难以诊断。

**方案**：`tracing` 作为 optional dependency，默认不启用。关键路径加 `tracing::debug!`/`tracing::warn!`。

**文件与改动**：

1. **`crates/core/Cargo.toml`** — 加 optional tracing
   ```toml
   [dependencies]
   tracing = { version = "0.1", optional = true }
   
   [features]
   default = []
   tracing = ["dep:tracing"]
   ```

2. **`crates/core/src/provider.rs`** — 关键路径加日志
   ```rust
   pub(crate) fn get_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
       #[cfg(feature = "tracing")]
       tracing::debug!(service = entry.type_name, lifetime = ?entry.lifetime, "resolving service");
       // ... 现有逻辑 ...
   }
   ```
   - `run_async_factory_safely` panic 时 `tracing::error!`
   - `dispose_scoped` 时 `tracing::debug!`

3. **`crates/core/src/validation.rs`** — 循环依赖/captive dependency 检测时 `tracing::warn!`

**验证**：`cargo test --features tracing` 通过。`cargo test`（默认）通过。

---

### 3.5 API 对称化（ServiceProviderWrapper）

**问题**：`ServiceProviderWrapper` 缺 `get_owned_any`/`get_keyed_owned_any` trait 实现，缺 `get_owned`/`get_async`/`get_keyed_async` inherent 方法，与 `ServiceProvider`/`Scope` API 不对称，违反 ISP。

**方案**：补齐 Wrapper 的 owned/async 方法。

**文件与改动**：

1. **`crates/core/src/wrapper.rs`** — `IServiceResolver for ServiceProviderWrapper` 补齐
   ```rust
   fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
       IServiceResolver::get_owned_any(self.child.as_ref(), key)
           .or_else(|| IServiceResolver::get_owned_any(self.root.as_ref(), key))
   }
   fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
       IServiceResolver::get_keyed_owned_any(self.child.as_ref(), key, variant)
           .or_else(|| IServiceResolver::get_keyed_owned_any(self.root.as_ref(), key, variant))
   }
   fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
       // 若迭代 2.4 已加此方法
       self.child.get_by_type_id(tid).or_else(|| self.root.get_by_type_id(tid))
   }
   ```

2. **`crates/core/src/wrapper.rs`** — 补齐 inherent 方法
   ```rust
   pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
       let type_name = std::any::type_name::<T>();
       self.try_get_owned::<T>().ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })
   }
   pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
       // 委托 child → root，extract + try_unwrap
   }
   pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> { /* ... */ }
   pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> { /* ... */ }
   
   pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
       // 委托 child.get_async() → root.get_async()
   }
   pub async fn get_keyed_async<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Result<Arc<T>, RdiError> { /* ... */ }
   ```

3. **`crates/core/src/wrapper.rs`** — 补齐 `get_all` 已有，检查 `get_keyed`/`get_optional` 一致性

**验证**：新增测试 `wrapper_owned_resolution`、`wrapper_async_resolution` 验证对称性。

---

## Assumptions & Decisions

### 设计决策
1. **`build()` 返回 `Arc<ServiceProvider>`** — 统一 API，`self_weak` 在 `new()` 中初始化，与 `new_async()` 一致
2. **删除 `into_arc()`** — 不再需要，`build()` 直接返回 `Arc`
3. **删除 MEDI 命名别名** — `get_service`/`get_required_service`/`get_services`/`create_scope`，统一 Rust 习惯命名
4. **`dispose_scoped` 改 `try_lock`** — 避免与并发 `register_disposable` 死锁，Drop 作为安全网
5. **`IServiceResolver` 加 `get_by_type_id`** — 消除 trait 默认与 inherent 方法不一致，ServiceProvider 覆盖为 O(1)
6. **新增 `ScopeFactory` trait** — 支持注入"创建 scope 的能力"
7. **删除 `IServiceLocator`/`INamedRegistrar`/`ServiceLocatorBridge`** — 与 `IProvider` 重叠，统一用 `IProvider`
8. **`RdiError::ServiceNotFound` 加 context 字段** — 增强错误可诊断性
9. **tracing 作为 optional dependency** — 默认不启用，生产环境按需开启
10. **Wrapper 补齐 owned/async 方法** — API 对称，遵循 ISP

### 技术约束
- `TypeId::of::<T>()` 支持 `?Sized`（含 `dyn Trait`），可用于 trait 对象直查
- `try_lock` 失败时 disposable 延迟到下次 dispose 或被丢弃（可接受，Drop 是安全网）
- `get_by_type_id` 默认实现无法做 TypeId→type_name 反查（无 std API），fallback 到 `get_any`
- 保留 `Arc<Arc<T>>` 双重包装（迭代 2.2 决定不做，支持 trait 对象是核心能力）

### 依赖变更
- `tracing = { version = "0.1", optional = true }`（可选，迭代 3.4）

---

## Verification Steps

### 迭代 2 完成标准
- [ ] `build()` 返回 `Arc<ServiceProvider>`，`into_arc()` 删除
- [ ] `get_service`/`get_required_service`/`get_services`/`create_scope` 删除
- [ ] `dispose_scoped` 用 `try_lock`，`scope_drop_with_concurrent_resolution` 无死锁
- [ ] `IServiceResolver::get_by_type_id` 加入，ServiceProvider 覆盖为 O(1)
- [ ] 全量测试通过（184+ 测试）
- [ ] `cargo clippy -- -D warnings` 无警告

### 迭代 3 完成标准
- [ ] `ScopeFactory` trait 可注入 singleton
- [ ] `IServiceLocator`/`INamedRegistrar`/`ServiceLocatorBridge`/`impl_service_locator!` 删除
- [ ] `RdiError::ServiceNotFound` 带 context 字段
- [ ] `ServiceProviderWrapper` 补齐 owned/async 方法，API 对称
- [ ] `cargo test --features tracing` 通过
- [ ] 全量测试通过

### 最终生产就绪标准
- 全量测试通过（含新增并发/异步/边界测试）
- `cargo clippy -- -D warnings` 无警告
- `cargo test --features tracing` 通过
- README 文档同步更新（删除 MEDI 别名引用，更新 build 返回类型）
- 无冗余 API、无兼容代码、架构简单清晰

---

## 执行顺序

**迭代 2**（按依赖顺序）：
1. 2.1 统一 build 返回类型（影响所有测试，先做）
2. 2.2 删除 MEDI 别名（影响 API 表面积）
3. 2.3 修复 Scope Drop 竞态（独立改动）
4. 2.4 TypeId 直查（代码清洁度，最后做）

**迭代 3**（按依赖顺序）：
1. 3.2 清理冗余 Trait（先做，减少后续改动范围）
2. 3.5 API 对称化（Wrapper 补齐方法，依赖 3.2 后的 trait 结构）
3. 3.1 ScopeFactory trait（新增 trait）
4. 3.3 RdiError context（影响所有错误构造点）
5. 3.4 tracing 集成（最后做，可选）

每个任务完成后运行全量测试，确保不引入回归。
