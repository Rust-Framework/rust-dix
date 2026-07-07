# DI 框架生产就绪迭代计划

## Summary

基于对标 MEDI 的深度审查，框架综合评分 5.3/10，距离生产水平有 4 个 P0 阻塞 + 6 个 P1 重要问题。本计划分三个迭代推进到生产水平，遵循"拒绝冗余、拒绝妥协、简单清晰、逻辑严谨"原则。

**迭代节奏**：
1. **迭代 1（P0 阻塞修复）**：Async scoped 缓存、Disposable 链、OnceLock panic 恢复、全量验证、循环路径有序
2. **迭代 2（P1 API 统一与性能）**：build 返回类型统一、去 Arc<Arc<T>>、TypeId 直查、错误处理统一、Scope Drop 竞态
3. **迭代 3（P2 架构完善）**：IServiceScopeFactory、tracing 集成、RdiError context、API 对称化、冗余 trait 清理

---

## Current State Analysis

### 框架现状
- **184 个测试全部通过**，核心解析能力齐全
- 三生命周期（Singleton/Scoped/Transient）+ 键控服务 + Owned 解析 + 跨 DLL 桥接
- 异步支持已落地（build_async + async_* 注册 + get_async）
- 过程宏三件套（#[derive(Inject)] / #[inject] / #[module]）

### 阻塞生产的硬伤
| ID | 问题 | 位置 | 严重度 |
|----|------|------|--------|
| H4 | Async scoped 不缓存，每次解析都调 async_factory | provider.rs:324-332 | P0 |
| H5 | Disposable::dispose 从未被调用，dispose_scoped 只跑注册闭包 | scope.rs:75-77, provider.rs:222-228 | P0 |
| C1 | OnceLock panic 后缓存永久失效，run_factory_safely 二次 panic_any | provider.rs:561-583, cache.rs:90 | P0 |
| C5 | async 工厂 panic 未捕获，直接终止 task | provider.rs:331 | P0 |
| H3 | build 时验证仅覆盖 sync Singleton，Scoped/Transient 循环依赖运行时才暴露 | validation.rs:248-251 | P0 |
| E1 | 循环依赖路径用 HashSet 无序输出，无法定位真实循环 | validation.rs:103-114 | P0 |
| A2 | build() 返回 ServiceProvider，build_async() 返回 Arc<ServiceProvider> | collection.rs:193,217 | P1 |
| P2 | Arc<Arc<T>> 双重包装，每次解析两次 Arc 操作 | entry.rs:225-226, provider.rs:587-592 | P1 |
| P3 | IServiceResolver::get<T>() 走 type_name 字符串路径 O(n) 反查 TypeId | entry.rs:76-84 | P1 |
| E2 | get_required_service panic-based，与 get() Result-based 语义冲突 | provider.rs:473-475 | P1 |
| C2 | Scope Drop 与并发 get_any 竞态，mem::take 后注册的 disposable 丢失 | scope.rs:163-167, provider.rs:222-228 | P1 |

### 冗余设计（需清理）
- `IServiceLocator` trait 与 `IServiceResolver` 高度重叠（get_any/get_any_named vs get_any/get_keyed_any）
- `INamedRegistrar` trait 仅 2 方法，可合并到 `IServiceResolver` 或独立为方法
- `get_required_service` / `get_services` 等 MEDI 命名别名增加 API 表面积，Rust 习惯用 `get` / `get_all`

---

## Iteration 1: P0 阻塞修复

### 1.1 修复 Async Scoped 缓存（H4）

**问题**：`get_async` 对 scoped 服务直接调用 async_factory，不查 `root_scoped_cache`，违反"一 scope 一实例"语义。

**文件**：`crates/core/src/provider.rs`

**修改**：`get_async` / `get_keyed_async` 对 Scoped 走 `root_scoped_cache.get_or_init_with`，与 sync 路径一致。但 `LazyCache::get_or_init_with` 的 factory 是 `FnOnce() -> AnyService`（同步），async factory 无法直接塞入。

**方案**：`get_async` 对 Scoped 先查 `root_scoped_cache.get()`，命中返回；未命中则调 async_factory，await 后用 `prefill` 写入缓存。并发首次解析可能重复执行（无 OnceLock 保护），但 scoped 通常在单 scope 内单线程使用，可接受。若需严格去重，用 `Mutex<Option<AnyService>>` 配合 async factory。

**具体改动**：
```rust
// get_async 中 Scoped 分支
ServiceLifetime::Scoped => {
    // 先查缓存
    if let Some(cached) = self.root_scoped_cache.get(entry.cache_key) {
        return Self::extract(cached).ok_or(RdiError::ServiceNotFound(type_name));
    }
    // 未命中：调 async factory 并缓存
    let provider_arc = self.provider_arc().ok_or(...)?;
    let arc_any = async_factory(provider_arc).await;
    self.root_scoped_cache.prefill(entry.cache_key, arc_any.clone());
    Self::extract(arc_any).ok_or(RdiError::ServiceNotFound(type_name))
}
```

**验证**：新增测试 `async_scoped_caches_within_scope` — 同一 scope 内两次 `get_async` 返回 `Arc::ptr_eq` 为 true。

---

### 1.2 重新设计 Disposable 链（H5）

**问题**：`Disposable::dispose(&mut self)` 要求 `&mut self`，但缓存中是 `Arc<T>`，多线程下 `Arc::get_mut` 拿不到。`dispose_scoped` 只跑 `register_disposable` 注册的闭包，从不调用 trait 方法。

**方案**：删除 `Disposable` trait（`&mut self` 签名不可用）。改为纯闭包注册模式：服务在工厂内通过 `provider.register_disposable(Box::new(|| { ... }))` 注册清理逻辑。这更符合 Rust 所有权模型——清理闭包拥有所需资源引用。

**文件**：
- `crates/core/src/scope.rs` — 删除 `Disposable` trait
- `crates/core/src/provider.rs` — `register_disposable` / `dispose_scoped` 保留（已是闭包模式）
- `crates/core/src/lib.rs` — 删除 `Disposable` 的 re-export 和文档

**具体改动**：
- 删除 `scope.rs:26-28` 的 `pub trait Disposable`
- 删除 `scope.rs:13-25` 的 `Disposable` 文档注释
- `Scope::dispose` 文档更新：说明通过 `register_disposable` 注册清理闭包
- 保留 `provider.rs:215-217` 的 `register_disposable` 和 `provider.rs:222-228` 的 `dispose_scoped`

**验证**：新增测试 `scoped_disposable_executes_on_scope_drop` — 注册清理闭包，验证 Scope drop 时执行。

---

### 1.3 修复 OnceLock Panic 后缓存永久失效（C1）

**问题**：`run_factory_safely` 对非 RdiError panic 用 `panic_any(RdiError::FactoryPanic)` 再次 panic。`OnceLock::get_or_init` 若工厂 panic，cell 保持未初始化，下次调用重新执行工厂——但 `run_factory_safely` 又会 panic，导致线程终止 + 缓存永久 poisoned。

**方案**：`run_factory_safely` 不再用 `panic_any`。改为在 `catch_unwind` 捕获 panic 后，将 `RdiError::FactoryPanic` 存入线程局部或返回 `Result`。但 `get_or_init_with` 的 factory 签名是 `FnOnce() -> AnyService`（不返回 Result），无法直接传播错误。

**根本方案**：`LazyCache` 改为缓存 `Result<AnyService, RdiError>`，工厂 panic 时缓存错误，后续解析直接返回缓存错误（不重试）。这符合"快速失败"原则——工厂 panic 通常是非瞬时性错误。

**文件**：`crates/core/src/cache.rs`、`crates/core/src/provider.rs`

**具体改动**：
```rust
// cache.rs
pub(crate) type CachedResult = Result<AnyService, RdiError>;

pub struct LazyCache {
    cells: RwLock<HashMap<usize, Arc<OnceLock<CachedResult>>>>,
}

pub fn get_or_init_with<F>(&self, key: usize, factory: F) -> CachedResult
where F: FnOnce() -> CachedResult,
{
    // fast path / slow path 同当前，但 cell 存 CachedResult
}
```

```rust
// provider.rs get_any_by_entry
ServiceLifetime::Singleton => {
    let factory = entry.factory.clone();
    let resolver = self as &dyn IServiceResolver;
    let svc_name = entry.type_name;
    self.singleton_cache.get_or_init_with(entry.cache_key, move || {
        Self::run_factory_safely(svc_name, move || factory(resolver))
    })
    .and_then(|v| v)  // 拆 Result
    .or_else(|e| {
        // 工厂 panic 缓存了错误，返回 None 让上层报错
        // 但实际上应该传播错误
    })
}
```

**注意**：`get_any_by_entry` 返回 `Option<AnyService>`，需改为 `Result<AnyService, RdiError>` 以传播工厂 panic 错误。这会连锁影响 `IServiceResolver::get_any` 签名。**评估后决定：保持 `get_any` 返回 `Option`，工厂 panic 仍用 catch_unwind 传播（Rust 习惯），但 OnceLock 缓存成功结果避免重复执行。panic 后不缓存（cell 保持未初始化），下次解析重试。** 这是更简单的方案——去掉 `run_factory_safely` 中的 `panic_any(RdiError::FactoryPanic)`，让原始 panic 自然传播，OnceLock 不缓存失败结果。

**最终方案（最简）**：
- `run_factory_safely` 只做一件事：catch_unwind 捕获 panic，转换为 `RdiError::FactoryPanic` 并 `panic_any` 重新抛出（保留当前行为）
- `OnceLock::get_or_init` panic 后不缓存——这是 std 行为，下次调用重试
- **删除 `run_factory_safely` 中的 `if payload.is::<RdiError>()` 分支**——这个分支把 RdiError 原样重新抛出，但 RdiError panic 不应出现在工厂中（ValidationResolver 用 `last_error` 存储错误而非 panic）
- **关键修复**：`get_any_by_entry` 中 `get_or_init_with` 的工厂闭包 panic 会导致 `OnceLock` 不缓存，但 `run_factory_safely` 的 `panic_any(RdiError::FactoryPanic)` 会再次 panic，**线程终止**。改为：`run_factory_safely` 不 `panic_any`，而是直接 `panic!` 带消息（让调用方用 `catch_unwind` 捕获），或返回 `Result`。

**决定**：`run_factory_safely` 改为返回 `AnyService`（panic 时直接 panic，不包装），删除 `panic_any(RdiError::FactoryPanic)` 逻辑。`RdiError::FactoryPanic` 变体删除（无人生产它）。调用方若需捕获工厂 panic，自行 `catch_unwind`。**这是最简方案——Rust 的 panic 机制本身就是错误传播通道，无需二次包装。**

**文件**：
- `crates/core/src/provider.rs` — `run_factory_safely` 简化为直接执行工厂（去掉 catch_unwind + panic_any 包装）。`get_any_by_entry` 直接调工厂，panic 自然传播。
- `crates/core/src/error.rs` — 删除 `RdiError::FactoryPanic` 变体

**影响**：`provider.rs:561-583` 的 `run_factory_safely` 整个删除。`get_any_by_entry` 中 `Self::run_factory_safely(svc_name, move || factory(resolver))` 改为 `factory(resolver)`。`validate_dependencies` 中的 `catch_unwind` 保留（build 阶段需捕获用户工厂 panic 转为 `RdiError`）。

**验证**：现有测试 `get_panics_on_missing` 等仍通过。新增测试 `factory_panic_propagates` — 注册 panic 工厂，验证 `get()` panic。

---

### 1.4 修复 Async 工厂 Panic 未捕获（C5）

**问题**：`get_async` 的 `async_factory(provider_arc).await` 无 `catch_unwind`，panic 直接终止 task。

**方案**：用 `AssertUnwindSafe(async_factory(provider_arc)).catch_unwind().await` 捕获 panic，转为 `RdiError`。

**文件**：`crates/core/src/provider.rs`

**具体改动**：
```rust
// get_async 中
use std::future::Future;
use std::panic::AssertUnwindSafe;

let fut = async_factory(provider_arc);
let result = AssertUnwindSafe(fut).catch_unwind().await;
let arc_any = result.map_err(|payload| {
    let msg = payload.downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().map(|s| s.clone()))
        .unwrap_or_else(|| "<non-string panic payload>".to_string());
    RdiError::FactoryPanic { service: type_name.to_string(), payload: msg }
})?;
```

**注意**：需要 `futures` crate 或手动实现 `Future::catch_unwind`。`std::panic::catch_unwind` 不支持 Future。需引入 `futures` 依赖或用 `async-compat`。**评估：引入 `futures` crate 的 `FutureExt::catch_unwind`。**

**Cargo.toml**：`futures = "0.3"` 加入 dependencies。

**验证**：新增测试 `async_factory_panic_returns_error` — 注册 panic async 工厂，验证 `get_async()` 返回 `Err(RdiError::FactoryPanic)`。

---

### 1.5 全量验证（H3）

**问题**：`validate_dependencies` 仅对 `entry.lifetime == Singleton && entry.async_factory.is_none()` 执行验证。Scoped/Transient 的循环依赖仅在运行时首次解析才暴露。

**方案**：build 阶段对所有服务（含 Scoped/Transient）执行循环依赖检测。但不需要实际执行工厂——只需静态分析依赖图。

**静态分析方案**：工厂闭包无法静态分析依赖（它是黑盒）。当前方案通过执行 Singleton 工厂来检测循环，这是因为 Singleton 在 build 时必然初始化。Scoped/Transient 不在 build 时初始化，无法通过执行检测。

**替代方案**：要求用户在注册时声明依赖（如 `ServiceCollection::singleton::<A>(|r| { let b: Arc<B> = r.get()?; ... }).depends_on::<B>()`）。但这增加心智负担。

**决定**：保持当前行为（仅验证 Singleton），但在文档中明确说明 Scoped/Transient 的循环依赖在运行时首次解析时才暴露。这是 MEDI 的行为——MEDI 也不验证 Transient 的循环依赖（Transient 不缓存，循环解析会栈溢出）。

**修改**：`validation.rs` 文档注释更新，说明验证范围。`RdiError` 无需新增变体。

**验证**：现有循环依赖测试保持通过。

---

### 1.6 循环依赖路径有序输出（E1）

**问题**：`check_cycle` 用 `resolving: RwLock<HashSet<usize>>`，`HashSet` 无序，`chain.join(" -> ")` 输出的循环路径是乱的。

**方案**：`resolving` 改用 `Vec<usize>` 维护解析顺序。`check_cycle` 用 `Vec::contains` 检测循环（O(n)，但 n 是依赖深度，通常 <10）。循环路径直接用 `Vec` 的顺序输出。

**文件**：`crates/core/src/validation.rs`

**具体改动**：
```rust
// ValidationResolver 字段
resolving: RwLock<Vec<usize>>,  // 改 HashSet → Vec

// check_cycle
fn check_cycle(&self, entry: &ServiceEntry) -> bool {
    let resolving = self.resolving.read().unwrap();
    if resolving.contains(&entry.cache_key) {
        let type_name = self.get_type_name(entry.cache_key);
        let cycle_desc = format!(
            "{} -> {}",
            resolving.iter().map(|&ck| self.get_type_name(ck)).collect::<Vec<_>>().join(" -> "),
            type_name
        );
        *self.last_error.lock().unwrap() = Some(RdiError::CircularDependency(cycle_desc));
        return true;
    }
    false
}

// resolve_via_factory
self.resolving.write().unwrap().push(entry.cache_key);  // push
// ... factory ...
self.resolving.write().unwrap().pop();  // pop（不是 remove）
```

**验证**：`circular_dependency_error_message` 测试验证输出路径有序。

---

## Iteration 2: P1 API 统一与性能

### 2.1 统一 build 返回类型（A2）

**问题**：`build()` 返回 `ServiceProvider`，`build_async()` 返回 `Arc<ServiceProvider>`。`build()` 的结果无法直接用 `provider_arc()`（需手动 `into_arc()`）。

**方案**：`build()` 也返回 `Arc<ServiceProvider>`，`self_weak` 在 `new()` 中初始化。删除 `into_arc()` 方法（不再需要）。

**文件**：
- `crates/core/src/provider.rs` — `new()` 改为返回 `Result<Arc<Self>, RdiError>`，内部初始化 `self_weak`
- `crates/core/src/collection.rs` — `build()` 返回 `Result<Arc<ServiceProvider>, RdiError>`
- `crates/core/src/provider.rs` — 删除 `into_arc()` 方法

**具体改动**：
```rust
// provider.rs
pub(crate) fn new(store: ServiceStore) -> Result<Arc<Self>, RdiError> {
    // ... validation + singleton init ...
    let sp_arc = Arc::new(sp);
    sp_arc.self_weak.get_or_init(|| Arc::downgrade(&sp_arc));
    Ok(sp_arc)
}

// collection.rs
pub fn build(self) -> Result<Arc<ServiceProvider>, RdiError> {
    // ... build store ...
    ServiceProvider::new(store)
}
```

**影响**：所有测试中 `ServiceCollection::new().build().unwrap()` 返回 `Arc<ServiceProvider>`。`provider.scope()` 直接可用。`provider.get()` 需通过 `Arc::deref` 调用（已支持）。

**验证**：全量测试调整后通过。

---

### 2.2 去 Arc<Arc<T>> 双重包装（P2）

**问题**：工厂返回 `Arc<dyn Any + Send + Sync>`，内部是 `Arc<Arc<T>>`。每次解析 `downcast::<Arc<T>>()` + `Arc::clone(&*)` 两次 Arc 操作。

**方案**：工厂直接返回 `Arc<T>` 并 cast 为 `Arc<dyn Any + Send + Sync>`（内部就是 `Arc<T>`，不是 `Arc<Arc<T>>`）。`extract` 函数改为 `downcast::<T>()` 直接获取 `Arc<T>`。

**文件**：
- `crates/core/src/collection.rs` — `push` / `push_async` 中工厂闭包改为 `val as Arc<dyn Any + Send + Sync>`（不包装 `Arc::new(val)`）
- `crates/core/src/provider.rs` — `extract` 改为 `arc.downcast::<T>()`
- `crates/core/src/entry.rs` — `IServiceResolver::get<T>()` 等默认实现中的 downcast 改为 `downcast::<T>()`
- `crates/macros/src/inject_attr.rs` — 工厂生成代码改为 `v as Arc<dyn Any + Send + Sync>`（不包装 `Arc::new(v)`）
- `crates/core/src/validation.rs` — `cache.prefill` 的值同步调整

**具体改动**：
```rust
// collection.rs push
let sf: ServiceFactory = Arc::new(move |r| {
    let val: Arc<T> = (f)(r);
    val as Arc<dyn Any + Send + Sync>  // 直接 cast，不 Arc::new(val)
});

// provider.rs extract
pub(crate) fn extract<T: ?Sized + Send + Sync + 'static>(
    arc: Arc<dyn Any + Send + Sync>,
) -> Option<Arc<T>> {
    arc.downcast::<T>().ok()  // 直接 downcast 为 Arc<T>
}

// entry.rs IServiceResolver::get<T>()
fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
where Self: Sized,
{
    let type_name = std::any::type_name::<T>();
    self.get_any(type_name)
        .and_then(|a| a.downcast::<T>().ok())  // 直接 downcast
        .ok_or(RdiError::ServiceNotFound(type_name))
}
```

**注意**：`Arc<dyn Any + Send + Sync>::downcast::<T>()` 需要 `T: 'static`。当前所有服务类型都要求 `'static`，满足约束。但 `?Sized` 类型（如 `dyn Trait`）无法直接 `downcast`（`Any` 要求 `Sized`）。**这是关键技术约束——trait 对象 `dyn Trait` 无法用 `Any::downcast`。**

**方案调整**：保留 `Arc<Arc<T>>` 双重包装用于 trait 对象，但具体类型用单层 `Arc<T>`。**评估后决定：统一保留 `Arc<Arc<T>>` 双重包装，因为框架核心价值是支持 trait 对象注入，不能牺牲这能力换性能。** 性能优化改为其他方向（TypeId 直查）。

**决定**：本项（2.2）不做。改为在迭代 2 中只做 TypeId 直查优化。

---

### 2.3 TypeId 直查优化（P3）

**问题**：`IServiceResolver::get<T>()` 默认实现走 `type_name::<T>()` 字符串路径，`get_any(key: &str)` 内部 `type_names.iter().find(|(_, name)| **name == key)` O(n) 反查 TypeId。`ServiceProvider` 已有 `type_map: HashMap<&'static str, TypeId>`，但 trait 默认实现无法访问。

**方案**：`IServiceResolver` 新增 `get_by_type_id(&self, tid: TypeId) -> Option<AnyService>` 方法，默认实现走 `type_name` 字符串路径（兼容外部实现），`ServiceProvider` 覆盖为直接 `store.get(&tid)` O(1) 查找。`get<T>()` 默认实现改用 `get_by_type_id(TypeId::of::<T>())`。

**文件**：`crates/core/src/entry.rs`、`crates/core/src/provider.rs`、`crates/core/src/validation.rs`、`crates/core/src/wrapper.rs`、`crates/core/src/scope.rs`

**具体改动**：
```rust
// entry.rs IServiceResolver
pub trait IServiceResolver: Send + Sync {
    /// TypeId 直查（默认走字符串路径，ServiceProvider 覆盖为 O(1)）
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        let name = std::any::type_name_by_type_id(tid); // 不存在此 API
        // 退而求其次：默认实现仍走 type_name，但 ServiceProvider 覆盖
        None
    }
    
    fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
    where Self: Sized,
    {
        // T: ?Sized 时无法 TypeId::of::<T>()，需保留字符串路径
        // 但 T: Sized 时走 TypeId 直查
        ...
    }
}
```

**技术约束**：`TypeId::of::<T>()` 要求 `T: 'static` 但不要求 `Sized`——实际上 `TypeId::of::<dyn Trait>()` 是合法的。所以可以统一用 `TypeId::of::<T>()`。

**方案**：
```rust
// entry.rs
fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
where Self: Sized,
{
    let tid = TypeId::of::<T>();
    self.get_by_type_id(tid)
        .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
        .ok_or(RdiError::ServiceNotFound(std::any::type_name::<T>()))
}

// provider.rs
impl IServiceResolver for ServiceProvider {
    fn get_by_type_id(&self, tid: TypeId) -> Option<AnyService> {
        let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
        self.get_any_by_entry(entry)
    }
    // get_any 保留（字符串路径，供宏生成的代码使用）
}
```

**影响**：`get_any` 保留用于宏生成的代码（`#[inject]` 字段注入通过 `type_name::<T>()` 调 `get_any`）。但 trait 默认 `get<T>()` 改走 `get_by_type_id`，性能提升。

**验证**：全量测试通过。新增 bench 对比优化前后（可选）。

---

### 2.4 错误处理统一（E2）

**问题**：`get_required_service` panic-based，与 `get()` Result-based 语义冲突。

**方案**：删除 `get_required_service` / `get_service` / `get_services` 等 MEDI 命名别名。统一用 `get` / `get_optional` / `get_all`。Rust 习惯用 `Result`，panic 是反模式。

**文件**：`crates/core/src/provider.rs`、`crates/core/src/scope.rs`

**具体改动**：
- 删除 `provider.rs:467-481` 的 `get_service` / `get_required_service` / `get_services`
- 删除 `scope.rs:88-101` 的 `get_service` / `get_required_service` / `get_services`
- 全局搜索测试中的 `get_required_service` / `get_service` / `get_services`，替换为 `get` / `get_optional` / `get_all`

**验证**：全量测试调整后通过。

---

### 2.5 修复 Scope Drop 与并发 get_any 竞态（C2）

**问题**：`Scope::drop` 调用 `dispose()` → `dispose_scoped()` → `mem::take(&mut *disposables.lock())`。若另一线程正在 `get::<ScopedX>()` 解析（工厂内可能触发 `register_disposable`），`mem::take` 后注册的 disposable 丢失。

**方案**：`Scope` 不实现 `Drop` 自动 dispose（避免与并发解析竞态）。改为要求用户显式调用 `Scope::dispose()`。`Drop` 只作为安全网——用 `try_lock` 避免阻塞，若锁不上则跳过 dispose。

**文件**：`crates/core/src/scope.rs`、`crates/core/src/provider.rs`

**具体改动**：
```rust
// scope.rs
impl Drop for Scope {
    fn drop(&mut self) {
        // 安全网：尝试 dispose，若锁竞争则跳过（避免阻塞或死锁）
        if let Ok(disposables) = self.scope_provider.disposables.try_lock() {
            if !disposables.is_empty() {
                drop(disposables); // 释放锁后再 dispose
                self.scope_provider.dispose_scoped();
            }
        }
    }
}

// provider.rs — disposables 改用 Mutex（已是 Mutex），暴露 try_lock 能力
// dispose_scoped 仍用 lock()，但 Drop 路径用 try_lock
```

**注意**：`disposables` 当前是 `Mutex<Vec<...>>`，`Mutex::try_lock` 可用。但 `dispose_scoped` 内部用 `lock()`，Drop 路径需改为 `try_lock` 全链路。

**决定**：`dispose_scoped` 改为 `try_lock`，锁不上时返回 false（表示有并发解析正在进行，dispose 跳过）。

**验证**：新增测试 `scope_drop_with_concurrent_resolution` — 多线程并发解析 + drop scope，验证无死锁。

---

## Iteration 3: P2 架构完善

### 3.1 引入 IServiceScopeFactory（H2）

**问题**：`scope()` 直接挂在 `ServiceProvider` 上，无法注入"创建 scope 的能力"给 singleton 服务。

**方案**：新增 `ScopeFactory` trait，`ServiceProvider` 实现它。singleton 服务可注入 `Arc<dyn ScopeFactory>` 按需创建 scope。

**文件**：新增 `crates/core/src/scope_factory.rs` 或在 `scope.rs` 中定义

**具体改动**：
```rust
// scope.rs
pub trait ScopeFactory: Send + Sync {
    fn create_scope(&self) -> Scope;
}

impl ScopeFactory for ServiceProvider {
    fn create_scope(&self) -> Scope {
        // 需要 &Arc<Self>，但 trait 方法是 &self
        // 用 self_weak.upgrade() 获取 Arc
    }
}
```

**技术约束**：`Scope::new` 需要 `Arc<ServiceProvider>`，但 `ScopeFactory::create_scope(&self)` 只有 `&self`。需通过 `self_weak` 获取 `Arc`。这要求 `self_weak` 已初始化（迭代 2 已统一 build 返回 `Arc<ServiceProvider>`，`self_weak` 在 build 时初始化）。

**验证**：新增测试 `scope_factory_injected_into_singleton` — singleton 服务注入 `Arc<dyn ScopeFactory>`，按需创建 scope。

---

### 3.2 清理冗余 Trait（架构清理）

**问题**：`IServiceLocator` 与 `IServiceResolver` 高度重叠；`INamedRegistrar` 仅 2 方法。

**方案**：
- 删除 `IServiceLocator` trait — 其功能由 `IServiceResolver` + `IProvider` 覆盖
- 删除 `INamedRegistrar` trait — named registry 方法直接在 `IProvider` trait 中
- 删除 `ServiceLocatorBridge` — 用户直接用 `Arc<dyn IProvider>` 或 `Arc<ServiceProvider>`
- 删除 `bridge.rs` 中的 `impl_service_locator!` 宏

**文件**：`crates/core/src/service_locator.rs`（删除）、`crates/core/src/bridge.rs`（清理）

**影响**：`bridge.rs` 测试需调整。`IServiceLocator` 的外部使用者需迁移到 `IServiceResolver`。

**验证**：全量测试调整后通过。

---

### 3.3 RdiError 增强 context（E5）

**问题**：`ServiceNotFound(&'static str)` 只有类型名，无解析链上下文。

**方案**：`ServiceNotFound` 增加 `context: Option<String>` 字段，记录"解析 A 时发现 B 未注册"。

**文件**：`crates/core/src/error.rs`

**具体改动**：
```rust
#[error("service not registered: {ty}{}", context.as_ref().map(|c| format!(" (while resolving {c})")).unwrap_or_default())]
ServiceNotFound { ty: &'static str, context: Option<String> },
```

**影响**：所有 `RdiError::ServiceNotFound("xxx")` 改为 `RdiError::ServiceNotFound { ty: "xxx", context: None }`。

**验证**：全量测试调整后通过。

---

### 3.4 tracing 集成（E4）

**问题**：解析失败、工厂 panic、scope dispose 全部无结构化日志。

**方案**：在关键路径加 `tracing::debug!` / `tracing::warn!`。`tracing` 作为 optional dependency，默认不启用。

**文件**：`crates/core/Cargo.toml`、`crates/core/src/provider.rs`、`crates/core/src/validation.rs`

**具体改动**：
- `Cargo.toml`: `tracing = { version = "0.1", optional = true }`，`[features] tracing = ["dep:tracing"]`
- `provider.rs`: `get_any_by_entry` 加 `#[cfg(feature = "tracing")] tracing::debug!(...)`
- `validation.rs`: 循环依赖 / captive dependency 检测时 `#[cfg(feature = "tracing")] tracing::warn!(...)`

**验证**：`cargo test --features tracing` 通过。

---

### 3.5 API 对称化（A3）

**问题**：`ServiceProvider` / `Scope` / `ServiceProviderWrapper` 的 API 不完全对称（Wrapper 缺 `get_owned` / `get_async` 等）。

**方案**：审查三个类型的公开 API，补齐缺失方法。`ServiceProviderWrapper` 补 `get_owned` / `get_async` / `get_keyed_async` / `get_all`（已有）。

**文件**：`crates/core/src/wrapper.rs`

**验证**：新增测试验证 Wrapper API 与 Provider/Scope 对称。

---

## Assumptions & Decisions

### 设计决策
1. **删除 `Disposable` trait** — `&mut self` 签名在 `Arc<T>` 缓存下不可用，改为纯闭包注册模式
2. **删除 `get_required_service` 等 MEDI 命名别名** — Rust 习惯用 Result，panic 是反模式
3. **删除 `IServiceLocator` / `INamedRegistrar` trait** — 与 `IServiceResolver` / `IProvider` 重叠
4. **删除 `ServiceLocatorBridge`** — 直接用 `Arc<dyn IProvider>` 更简单
5. **保留 `Arc<Arc<T>>` 双重包装** — 支持 trait 对象注入是核心能力，不能牺牲
6. **build() 改为返回 `Arc<ServiceProvider>`** — 统一 API，`self_weak` 在 build 时初始化
7. **`run_factory_safely` 简化** — 删除 `panic_any(RdiError::FactoryPanic)` 二次包装，panic 自然传播
8. **`RdiError::FactoryPanic` 保留** — async 工厂 panic 仍需转为 `RdiError`（async 无法自然传播 panic）
9. **循环依赖验证仅覆盖 Singleton** — 与 MEDI 行为一致，Scoped/Transient 在运行时首次解析暴露

### 技术约束
- `Arc<dyn Any + Send + Sync>::downcast::<T>()` 要求 `T: Sized`，trait 对象无法 downcast，必须保留 `Arc<Arc<T>>`
- `OnceLock::get_or_init` panic 后不缓存，是 std 行为
- `std::panic::catch_unwind` 不支持 Future，async 工厂 panic 捕获需 `futures::FutureExt::catch_unwind`
- `TypeId::of::<T>()` 支持 `?Sized` 类型（如 `dyn Trait`），可用于 TypeId 直查

### 依赖变更
- 新增 `futures = "0.3"`（async catch_unwind）
- 新增 `tracing = { version = "0.1", optional = true }`（可选日志）

---

## Verification Steps

### 迭代 1 完成标准
- [ ] `async_scoped_caches_within_scope` 测试通过
- [ ] `scoped_disposable_executes_on_scope_drop` 测试通过
- [ ] `factory_panic_propagates` 测试通过
- [ ] `async_factory_panic_returns_error` 测试通过
- [ ] `circular_dependency_error_message` 验证路径有序
- [ ] 全量 184+ 测试通过

### 迭代 2 完成标准
- [ ] `build()` 返回 `Arc<ServiceProvider>`，`into_arc()` 删除
- [ ] `get_required_service` / `get_service` / `get_services` 删除
- [ ] `IServiceResolver::get<T>()` 走 `get_by_type_id` O(1) 路径
- [ ] `scope_drop_with_concurrent_resolution` 无死锁
- [ ] 全量测试通过

### 迭代 3 完成标准
- [ ] `ScopeFactory` trait 可注入 singleton
- [ ] `IServiceLocator` / `INamedRegistrar` / `ServiceLocatorBridge` 删除
- [ ] `RdiError::ServiceNotFound` 带 context
- [ ] `cargo test --features tracing` 通过
- [ ] `ServiceProvider` / `Scope` / `ServiceProviderWrapper` API 对称
- [ ] 全量测试通过

### 最终生产就绪标准
- 全量测试通过（含新增并发/异步/边界测试）
- `cargo clippy -- -D warnings` 无警告
- `cargo bench` 解析热路径性能不退化
- README 文档同步更新
