# DI 框架生产就绪迭代执行计划（剩余任务）

## Summary

迭代 1（P0 阻塞修复）已全部完成，184 个测试通过。迭代 2 中任务 2.1（build 返回类型统一）已完成，任务 2.2（删除 MEDI 命名别名）源码改动完成但 `crates/core/tests/scope.rs` 仍引用已删除方法（`create_scope` / `get_service` / `get_required_service` / `get_services`），导致当前编译失败。

本计划聚焦剩余 7 个任务，按优先级推进至生产水平。**核心原则**：拒绝冗余代码、拒绝妥协设计、拒绝兼容代码；架构简单清晰、逻辑严谨、功能全面；严格遵循 SRP/OCP/LSP/ISP/DIP。

---

## Current State Analysis

### 已完成
| 任务 | 状态 | 证据 |
|------|------|------|
| 迭代 1 全部（1.1-1.6） | ✅ | 184 测试通过 |
| 2.1 统一 build 返回类型 | ✅ | `provider.rs::new()` 返回 `Result<Arc<Self>, RdiError>`，`self_weak` 在 new 中初始化，`into_arc()` 已删除，`collection.rs::build()` 返回 `Arc<ServiceProvider>`，`module.rs` 宏生成代码已去掉 `Arc::new()` 包装 |
| 2.2 源码删除 MEDI 别名 | ✅ | `provider.rs` 已无 `create_scope`/`get_service`/`get_required_service`/`get_services`；`scope.rs` 已无 `get_service`/`get_required_service`/`get_services` |

### 待完成（按优先级）
| 优先级 | 任务 | 阻塞情况 |
|--------|------|----------|
| **P0** | 2.2 完成：修复 `tests/scope.rs` + 清理陈旧文档注释 | **当前编译失败** |
| **P0** | 2.3 修复 Scope Drop 并发竞态 | 并发安全 |
| **P1** | 3.2 清理冗余 Trait（IServiceLocator/INamedRegistrar/ServiceLocatorBridge/impl_service_locator!） | 架构清晰 |
| **P1** | 3.5 API 对称化（ServiceProviderWrapper 补齐 owned/async） | ISP 合规 |
| **P1** | 2.4 TypeId 直查优化（trait 默认实现走 TypeId 路径） | 代码清洁度 |
| **P2** | 3.1 引入 ScopeFactory trait | 新能力 |
| **P2** | 3.3 RdiError 增强 context | 可诊断性 |

### 关键代码现状
| 文件 | 行号 | 现状 |
|------|------|------|
| `crates/core/tests/scope.rs` | L12, L24, L27, L36 | 引用已删除方法，**编译失败** |
| `crates/core/src/entry.rs` | L67 | 文档注释提及已删除的 `into_arc` |
| `crates/core/src/provider.rs` | L27, L75, L196 | 文档注释提及 `create_scope()` / `into_arc()` |
| `crates/core/src/lib.rs` | L176, L192 | 文档注释提及 `create_scope()` |
| `crates/core/src/provider.rs` | L227-233 | `dispose_scoped` 用阻塞式 `lock()` |
| `crates/core/src/entry.rs` | L76-213 | `IServiceResolver::get<T>()` 等默认实现走字符串路径 |
| `crates/core/src/service_locator.rs` | 整文件 | `IServiceLocator` + `INamedRegistrar` 与 `IProvider` 重叠 |
| `crates/core/src/bridge.rs` | L71-139 | `ServiceLocatorBridge` + `impl_service_locator!` 宏 |
| `crates/core/src/wrapper.rs` | L70-88 | 缺 `get_owned_any`/`get_keyed_owned_any` trait 实现 |
| `crates/core/src/wrapper.rs` | — | 缺 `get_owned`/`try_get_owned`/`get_keyed_owned`/`try_get_keyed_owned`/`get_async`/`get_keyed_async` inherent 方法 |
| `crates/core/src/error.rs` | L4-5 | `ServiceNotFound(&'static str)` 无 context |

---

## P0 任务

### 2.2 完成：修复测试 + 清理陈旧文档注释

**问题**：源码已删除 MEDI 别名，但 `crates/core/tests/scope.rs` 仍调用 `create_scope()` / `get_service()` / `get_required_service()` / `get_services()`，导致编译失败。同时多处文档注释引用已删除的 `into_arc()` / `create_scope()`。

**文件与改动**：

1. **`crates/core/tests/scope.rs`** — 替换方法调用 + 重命名测试函数
   - L7 `fn create_scope_medinaming()` → `fn scope_resolves_scoped()`
   - L12 `p.create_scope()` → `p.scope()`
   - L18 `fn scope_get_service_and_get_required()` → `fn scope_get_optional_and_get_unwrap()`
   - L24 `scope.get_service()` → `scope.get_optional()`
   - L27 `scope.get_required_service()` → `scope.get().unwrap()`
   - L33 `fn scope_get_services_returns_empty()` → `fn scope_get_all_returns_empty()`
   - L36 `scope.get_services()` → `scope.get_all()`

2. **`crates/core/src/entry.rs`** L67 — 文档注释清理
   - `or if the provider was not created via [`ServiceProvider::into_arc`].` → `or if the resolver is not backed by a `ServiceProvider` (e.g. `ValidationResolver` during build-time validation).`
   - 删除对已删除 `into_arc` 的引用

3. **`crates/core/src/provider.rs`** — 三处陈旧文档注释
   - L27: `子 Scope（通过 `create_scope()` 创建）` → `子 Scope（通过 [`ServiceProvider::scope`] 创建）`
   - L75: `out of the box — no `into_arc()` step needed.` → 保留（这是说明性文字，提到 `into_arc()` 是历史对比，可接受）→ **改为**：删除 `— no `into_arc()` step needed` 这半句
   - L196: `与 [`Self::into_arc`] 一样初始化 self_weak` → `与 [`Self::new`] 一样初始化 self_weak`

4. **`crates/core/src/lib.rs`** — 两处陈旧文档注释
   - L176: `通过 `create_scope()` 创建的子 Scope` → `通过 [`ServiceProvider::scope`] 创建的子 Scope`
   - L192: `created via [`ServiceProvider::create_scope`]` → `created via [`ServiceProvider::scope`]`

**验证**：`cargo test` 全量通过；`cargo clippy -- -D warnings` 无警告。

---

### 2.3 修复 Scope Drop 并发竞态

**问题**：`Scope::drop` → `dispose()` → `dispose_scoped()` → `mem::take(&mut *disposables.lock())`。若另一线程正在 `get::<ScopedX>()` 解析（工厂内 `register_disposable` 持有锁），`drop` 的 `lock()` 会阻塞；若 `drop` 与 `register` 交错，`mem::take` 后注册的 disposable 丢失。

**方案**：`dispose_scoped()` 改用 `try_lock`，锁不上时跳过（表示有并发解析正在进行）。`Drop` 作为安全网，不保证执行所有 disposable。

**文件与改动**：

1. **`crates/core/src/provider.rs`** L227-233 — `dispose_scoped` 改用 `try_lock`
   ```rust
   pub(crate) fn dispose_scoped(&self) {
       // try_lock 避免与并发 register_disposable 死锁。
       // 锁不上时跳过——并发解析仍在进行，disposable 留待下次 dispose 或被丢弃。
       let to_run: Vec<Box<dyn FnOnce() + Send>> = match self.disposables.try_lock() {
           Ok(mut guard) => std::mem::take(&mut *guard),
           Err(_) => return, // 并发解析持锁，跳过本次 dispose
       };
       // 释放锁后再执行闭包（避免持锁调用用户代码，防止重入死锁）
       for cleanup in to_run {
           cleanup();
       }
   }
   ```

2. **`crates/core/src/scope.rs`** L44-65 — `Scope::dispose` 文档更新
   - 在现有文档基础上补充：`Drop 时自动调用作为安全网。若 Drop 时有并发解析正在进行（锁竞争），部分 disposable 可能延迟到下次 dispose 或被丢弃。需要确定性清理时，应在 scope 结束前显式调用此方法。`

3. **新增测试** `crates/core/tests/scope.rs` — `scope_drop_with_concurrent_resolution`
   - 多线程并发解析 Scoped 服务（工厂内 `register_disposable` + `thread::sleep`）+ 主线程 drop scope
   - 验证无死锁（测试在合理时间内完成）
   - 验证已注册的 disposable 被执行（用 `AtomicUsize` 计数）

**验证**：新增测试通过；无死锁。

---

## P1 任务

### 3.2 清理冗余 Trait

**问题**：`IServiceLocator`（4 方法）与 `IServiceResolver` 的 `get_any` 重叠；`INamedRegistrar`（2 方法）与 `IProvider` 的 `rdi_register_named`/`rdi_remove_named` 重叠；`ServiceLocatorBridge` 只是 `IProvider` 的薄包装，无附加价值；`impl_service_locator!` 宏为三个类型生成重复的 `IServiceLocator` 实现。

**方案**：删除 `IServiceLocator`、`INamedRegistrar`、`ServiceLocatorBridge`、`impl_service_locator!` 宏。统一用 `IProvider`（已覆盖解析 + named 注册）。

**文件与改动**：

1. **删除 `crates/core/src/service_locator.rs`** — 整个文件删除

2. **`crates/core/src/bridge.rs`** — 删除 `ServiceLocatorBridge` 和 `impl_service_locator!` 宏，保留 `IProvider` trait 和 3 个 impl
   ```rust
   // 保留：IProvider trait + 3 个 impl（ServiceProvider / ServiceProviderWrapper / Scope）
   // 删除：ServiceLocatorBridge 结构体 + new() + provider() 方法
   // 删除：impl IServiceLocator for ServiceLocatorBridge
   // 删除：impl INamedRegistrar for ServiceLocatorBridge
   // 删除：impl_service_locator! 宏 + 3 个调用
   ```
   - 删除 `use crate::service_locator::{INamedRegistrar, IServiceLocator};` 导入
   - 保留 `use std::any::Any;` `use std::sync::Arc;` `use crate::entry::IServiceResolver;` `use crate::provider::ServiceProvider;` `use crate::scope::Scope;` `use crate::wrapper::ServiceProviderWrapper;`

3. **`crates/core/src/lib.rs`** — 更新 re-exports
   - 删除 `pub mod service_locator;`
   - 删除 `pub use service_locator::IServiceLocator;`
   - 删除 `pub use service_locator::INamedRegistrar;`
   - 删除 `pub use bridge::ServiceLocatorBridge;`
   - 保留 `pub use bridge::IProvider;`
   - 更新 L142 模块说明表：删除 `IServiceLocator / ServiceLocatorBridge` 行
   - 更新 L208-215 注释：将"Plugin / cross-DLL support"区块改为只导出 `IProvider`

4. **`crates/core/src/scope.rs`** L124, L129 — 文档注释更新
   - `/// Register a named service (for `impl_service_locator!` macro).` → `/// Register a named service (for [`IProvider`] trait).`
   - `/// Remove a named service (for `impl_service_locator!` macro).` → `/// Remove a named service (for [`IProvider`] trait).`

5. **`crates/core/src/wrapper.rs`** L91, L96 — 同上文档注释更新

6. **`crates/core/src/provider.rs`** L627, L635 — 同上文档注释更新

7. **`crates/core/tests/bridge.rs`** — 重写测试文件
   - 保留 `rdiprovider_root_resolves`、`rdiprovider_wrapped_resolves`、`rdiprovider_get_named_any`、`rdiprovider_register_and_remove_named`（这 4 个测试 `IProvider` trait 行为，与清理无关）
   - 删除 `servicelocatorbridge_roundtrip`、`servicelocatorbridge_provider_accessor`（测试已删除的 `ServiceLocatorBridge`）
   - 删除 `serviceprovider_as_servicelocator_get_any`、`serviceprovider_as_servicelocator_named`、`serviceprovider_as_servicelocator_register_remove`（测试已删除的 `IServiceLocator` trait）
   - 删除 `named_registrar_via_bridge`（测试已删除的 `INamedRegistrar` trait）

**验证**：全量测试通过；`cargo clippy -- -D warnings` 无警告。

---

### 3.5 API 对称化（ServiceProviderWrapper）

**问题**：`ServiceProviderWrapper` 缺 `get_owned_any`/`get_keyed_owned_any` trait 实现，缺 `get_owned`/`try_get_owned`/`get_keyed_owned`/`try_get_keyed_owned`/`get_async`/`get_keyed_async` inherent 方法，与 `ServiceProvider`/`Scope` API 不对称，违反 ISP。

**方案**：补齐 Wrapper 的 owned/async 方法，委托 child → root（与现有 `get_any` 一致）。

**文件与改动**：

1. **`crates/core/src/wrapper.rs`** — `IServiceResolver for ServiceProviderWrapper` 补齐 trait 实现
   ```rust
   fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
       IServiceResolver::get_owned_any(self.child.as_ref(), key)
           .or_else(|| IServiceResolver::get_owned_any(self.root.as_ref(), key))
   }
   fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
       IServiceResolver::get_keyed_owned_any(self.child.as_ref(), key, variant)
           .or_else(|| IServiceResolver::get_keyed_owned_any(self.root.as_ref(), key, variant))
   }
   ```

2. **`crates/core/src/wrapper.rs`** — 补齐 inherent 方法
   ```rust
   pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
       let type_name = std::any::type_name::<T>();
       self.try_get_owned::<T>().ok_or(RdiError::ServiceNotFound(type_name))
   }
   pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
       let arc_any = IServiceResolver::get_owned_any(self.child.as_ref(), std::any::type_name::<T>())
           .or_else(|| IServiceResolver::get_owned_any(self.root.as_ref(), std::any::type_name::<T>()))?;
       let arc_t = crate::provider::ServiceProvider::extract::<T>(arc_any)?;
       Arc::try_unwrap(arc_t).ok()
   }
   pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> {
       let type_name = std::any::type_name::<T>();
       self.try_get_keyed_owned::<T>(key).ok_or(RdiError::KeyedServiceNotFound { key: key.to_string(), ty: type_name })
   }
   pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
       let arc_any = IServiceResolver::get_keyed_owned_any(self.child.as_ref(), std::any::type_name::<T>(), key)
           .or_else(|| IServiceResolver::get_keyed_owned_any(self.root.as_ref(), std::any::type_name::<T>(), key))?;
       let arc_t = crate::provider::ServiceProvider::extract::<T>(arc_any)?;
       Arc::try_unwrap(arc_t).ok()
   }
   pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
       self.child.get_async::<T>().await.or_else(|_| {
           // 异步错误回退到 root——但 or_else 闭包不能 await，需用 match
       })
   }
   ```
   - **async 委托的技术约束**：`or_else` 闭包不能 `await`，需用 `match` 或 `?` + 立即 `await`
   ```rust
   pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
       match self.child.get_async::<T>().await {
           Ok(svc) => Ok(svc),
           Err(_) => self.root.get_async::<T>().await,
       }
   }
   pub async fn get_keyed_async<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Result<Arc<T>, RdiError> {
       match self.child.get_keyed_async::<T>(key).await {
           Ok(svc) => Ok(svc),
           Err(_) => self.root.get_keyed_async::<T>(key).await,
       }
   }
   ```

3. **新增测试** `crates/core/tests/wrapper.rs` — `wrapper_owned_resolution` + `wrapper_async_resolution`
   - owned: child 注册 Transient 服务，wrapper.get_owned() 成功；Singleton 服务 get_owned() 返回 None
   - async: child 注册 async_transient，wrapper.get_async() 成功；root 回退成功

**验证**：新增测试通过；Wrapper API 与 Provider/Scope 对称。

---

### 2.4 TypeId 直查优化

**问题**：`IServiceResolver::get<T>()` 默认实现走 `type_name::<T>()` 字符串路径，`get_any(key)` 内部 `type_map.get(key)` 反查 TypeId。虽然 ServiceProvider 的 inherent `get<T>()` 已是 TypeId 直查（L301-305 走 `try_get` → `TypeId::of::<T>()`），但 trait 默认实现与 inherent 方法不一致，外部实现者（如 ValidationResolver）走低效路径。

**方案**：`IServiceResolver` 新增 `get_by_type_id` 和 `get_keyed_by_type_id` 默认方法（返回 `None`，外部实现者按需覆盖），ServiceProvider/Scope/ServiceProviderWrapper 覆盖为 O(1) 直查。trait 默认 `get<T>()` 等方法改用 `get_by_type_id` 优先，fallback 到 `get_any` 字符串路径。

**文件与改动**：

1. **`crates/core/src/entry.rs`** — `IServiceResolver` trait 新增 2 个方法 + 重写默认实现
   ```rust
   pub trait IServiceResolver: Send + Sync {
       // 现有 get_any / get_keyed_any / get_all_any / provider_arc / get_owned_any / get_keyed_owned_any 保留

       /// TypeId 直查（默认 None，ServiceProvider 等覆盖为 O(1) 直查）。
       /// 外部实现者若需优化可覆盖；不覆盖则 fallback 到 get_any 字符串路径。
       fn get_by_type_id(&self, _tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
           None
       }
       fn get_keyed_by_type_id(&self, _tid: TypeId, _key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
           None
       }

       fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
       where Self: Sized,
       {
           let type_name = std::any::type_name::<T>();
           let tid = TypeId::of::<T>();
           self.get_by_type_id(tid)
               .or_else(|| self.get_any(type_name))
               .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
               .ok_or(RdiError::ServiceNotFound(type_name))
       }
       // try_get / get_keyed / try_get_keyed / get_all 同理改用 get_by_type_id / get_keyed_by_type_id 优先
       // get_owned / try_get_owned / get_keyed_owned / try_get_keyed_owned 保留字符串路径（owned 不走热路径）
   }
   ```

2. **`crates/core/src/provider.rs`** — `IServiceResolver for ServiceProvider` 覆盖 2 个方法
   ```rust
   fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
       let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
       self.get_any_by_entry(entry)
   }
   fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
       let entry = self.store.get(&tid)?.iter().find(|e| e.key.as_deref() == Some(key))?;
       self.get_any_by_entry(entry)
   }
   ```

3. **`crates/core/src/scope.rs`** — `IServiceResolver for Scope` 覆盖 2 个方法（委托 scope_provider）

4. **`crates/core/src/wrapper.rs`** — `IServiceResolver for ServiceProviderWrapper` 覆盖 2 个方法（child → root）

**技术约束**：`TypeId::of::<T>()` 支持 `?Sized`（含 `dyn Trait`），可用。`get_by_type_id` 默认返回 `None`，外部实现者（ValidationResolver）不覆盖，fallback 到 `get_any` 字符串路径——行为不变。

**验证**：全量测试通过。性能无退化（热路径 inherent 方法未变）。

---

## P2 任务

### 3.1 引入 ScopeFactory trait

**问题**：`scope()` 直接挂在 `ServiceProvider` 上（`self: &Arc<Self>` 签名），无法注入"创建 scope 的能力"给 singleton 服务（如后台 worker 按需创建 request scope）。

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
           // 通过 self_weak 获取 Arc<Self>（new() 已初始化 self_weak）
           self.self_weak.get()
               .and_then(|w| w.upgrade())
               .map(|arc| Scope::new(arc))
               .expect("ScopeFactory::create_scope called before self_weak initialization")
       }
   }
   ```

3. **`crates/core/src/lib.rs`** — re-export `ScopeFactory`
   - `pub use scope::ScopeFactory;`

4. **新增测试** `crates/core/tests/scope.rs` — `scope_factory_injected_into_singleton`
   - 注册 `Arc<dyn ScopeFactory>` 为 singleton（用 `provider.scope()` 创建初始 scope，或直接用 `provider_arc()`）
   - singleton 服务注入 `Arc<dyn ScopeFactory>`，按需创建 scope，验证 scope 独立性（不同 scope 的 Scoped 服务实例不同）

**验证**：新增测试通过。

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
       // 其他变体不变
   }
   ```

2. **全局搜索替换**：`RdiError::ServiceNotFound("xxx")` → `RdiError::ServiceNotFound { ty: "xxx", context: None }`
   - `crates/core/src/entry.rs` — 约 4 处（trait 默认实现）
   - `crates/core/src/provider.rs` — 约 5 处（inherent 方法）
   - 测试中的 `match RdiError::ServiceNotFound(ty)` 模式匹配更新

3. **`crates/core/src/provider.rs`** — `get_any_by_entry` 等方法在解析失败时构造带 context 的错误（可选增强）
   - 暂不实现深层 context 传播（需重构返回类型为 `Result`，影响大），仅保留 `context: None` 占位

**验证**：全量测试通过；错误消息格式更新。

---

## Assumptions & Decisions

### 设计决策
1. **`dispose_scoped` 改 `try_lock`** — 避免与并发 `register_disposable` 死锁，Drop 作为安全网
2. **`IServiceResolver` 加 `get_by_type_id`/`get_keyed_by_type_id`** — 消除 trait 默认与 inherent 方法不一致，ServiceProvider 覆盖为 O(1)，外部实现者 fallback 到字符串路径
3. **删除 `IServiceLocator`/`INamedRegistrar`/`ServiceLocatorBridge`/`impl_service_locator!`** — 与 `IProvider` 重叠，统一用 `IProvider`
4. **Wrapper 补齐 owned/async 方法** — API 对称，遵循 ISP
5. **新增 `ScopeFactory` trait** — 支持注入"创建 scope 的能力"
6. **`RdiError::ServiceNotFound` 加 context 字段** — 增强错误可诊断性，暂不实现深层 context 传播
7. **不实现 tracing 集成** — 优先级最低，可选，本计划不包含

### 技术约束
- `TypeId::of::<T>()` 支持 `?Sized`（含 `dyn Trait`），可用于 trait 对象直查
- `try_lock` 失败时 disposable 延迟到下次 dispose 或被丢弃（可接受，Drop 是安全网）
- `get_by_type_id` 默认实现返回 `None`，外部实现者 fallback 到 `get_any` 字符串路径
- async 委托用 `match` 而非 `or_else`（闭包不能 `await`）
- `ScopeFactory::create_scope(&self)` 通过 `self_weak.upgrade()` 获取 `Arc<Self>`（`new()` 已初始化）

### 依赖变更
- 无新增依赖

---

## Verification Steps

### 完成标准
- [ ] `crates/core/tests/scope.rs` 编译通过，MEDI 别名调用替换为 Rust 习惯命名
- [ ] 陈旧文档注释清理完毕（无 `into_arc` / `create_scope` 引用，除了历史对比说明）
- [ ] `dispose_scoped` 用 `try_lock`，`scope_drop_with_concurrent_resolution` 无死锁
- [ ] `IServiceLocator`/`INamedRegistrar`/`ServiceLocatorBridge`/`impl_service_locator!` 删除
- [ ] `ServiceProviderWrapper` 补齐 owned/async 方法，API 对称
- [ ] `IServiceResolver::get_by_type_id`/`get_keyed_by_type_id` 加入，ServiceProvider 覆盖为 O(1)
- [ ] `ScopeFactory` trait 可注入 singleton
- [ ] `RdiError::ServiceNotFound` 带 context 字段
- [ ] 全量测试通过（含新增并发/异步/边界测试）
- [ ] `cargo clippy -- -D warnings` 无警告

---

## 执行顺序

1. **2.2 完成**（解除编译阻塞）→ 运行 `cargo test` 验证
2. **2.3 修复 Scope Drop 竞态** → 运行 `cargo test` 验证
3. **3.2 清理冗余 Trait** → 运行 `cargo test` 验证
4. **3.5 API 对称化 Wrapper** → 运行 `cargo test` 验证
5. **2.4 TypeId 直查优化** → 运行 `cargo test` 验证
6. **3.1 引入 ScopeFactory trait** → 运行 `cargo test` 验证
7. **3.3 RdiError context 增强** → 运行 `cargo test` 验证
8. **最终验证**：`cargo clippy -- -D warnings` + 全量测试

每个任务完成后运行全量测试，确保不引入回归。
