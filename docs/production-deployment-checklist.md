# 生产环境部署检查清单

> 适用于 `rust-dix` v0.5.x。基于源码静态分析 + 运行时行为验证编写。

---

## 一、内存泄漏风险

### 1.1 Singleton 缓存永不清理

**机制**：`LazyCache.cells: HashMap<usize, Arc<OnceLock<AnyService>>>` 没有清理方法。Singleton 实例一旦创建，生命周期等于 `ServiceProvider` 生命周期。

**风险**：`ServiceProvider` 长期存活（如全局 `lazy_static` / `OnceLock`）时，所有 Singleton 永不释放。

**检查项**：
- [ ] 确认 `ServiceProvider` 的生命周期边界——是否在应用关闭时 `drop`？
- [ ] 确认没有 `static` 变量持有 `Arc<ServiceProvider>`（用 `Weak` 替代）
- [ ] 确认 Singleton 服务不持有大对象（如未限流的缓存）而不提供清理接口

**缓解**：应用关闭时显式 `drop(provider)`，让 Singleton 随之释放。

---

### 1.2 async_locks 累积

**机制**：`LazyCache.async_locks: HashMap<usize, Arc<FuturesMutex<Option<AnyService>>>>` 在首次 async scoped 解析时创建，永不清理。

**风险**：单个 `Scope` 内解析大量不同类型的 async scoped 服务 → 每个 key 额外占一个 `FuturesMutex`（约 40 字节）。

**检查项**：
- [ ] 确认 `Scope` 生命周期短（HTTP 请求级别，请求结束即 drop）
- [ ] 确认同一 `Scope` 内 async scoped 服务类型数量可控（通常 < 50）
- [ ] 确认没有长期存活的 `Scope`（如全局 `Scope` 池）

**缓解**：`Scope` drop 时 `async_locks` 随之释放（`LazyCache` 被 `ServiceProvider` 拥有，`Scope` drop 释放 `scope_provider`）。

---

### 1.3 disposables 未执行导致资源泄漏

**机制**：`dispose_scoped()` 用 `try_lock()`，若并发解析正在 `register_disposable`（持锁），则 `dispose` 跳过——未执行的 disposable 闭包泄漏其捕获资源。

**风险**：文件句柄、数据库连接、网络 socket 等非内存资源泄漏。

**检查项**：
- [ ] 确认 `Scope` 在并发解析完成后才 drop（不要在解析进行中 drop scope）
- [ ] 确认关键资源在 `dispose()` 之外有兜底释放（如 `Drop` impl on service struct）
- [ ] 确认 `scope.dispose()` 在 scope 生命周期的确定性终点被显式调用

**缓解**：在业务代码中，scope 使用模式应为：
```rust
let scope = provider.scope();
// ... 使用 scope 解析服务 ...
scope.dispose();  // 显式 dispose，不依赖 Drop
```

---

### 1.4 self_weak 循环引用（无风险）

**机制**：`self_weak: OnceLock<Weak<ServiceProvider>>` 用 `Weak` 而非 `Arc`，不阻止 `ServiceProvider` 被 drop。

**结论**：无泄漏风险，无需检查。

---

### 1.5 named registry 跨 DLL 累积

**机制**：`register_named()` 向 `named: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>` 插入条目，`remove_named()` 可移除。但 DLL 卸载时若未清理其注册项，会导致悬空引用。

**检查项**：
- [ ] 确认 DLL 卸载前调用 `remove_named()` 清理其注册的所有服务
- [ ] 确认跨 DLL 传递的是 `Arc<dyn IProvider>` 而非裸指针
- [ ] 确认 DLL 的 `#[module]` 生成的 `__rdi_build_provider_xxx()` 在 DLL 卸载时不再被调用

---

## 二、生命周期管理边界

### 2.1 Singleton

| 属性 | 行为 |
|------|------|
| 缓存位置 | `singleton_cache`（全局唯一，`OnceLock` 保护） |
| 初始化时机 | `build()` 时 eager init（工厂执行一次） |
| 生命周期 | = `ServiceProvider` 生命周期 |
| 线程安全 | `OnceLock::get_or_init` 保证工厂只执行一次 |
| 依赖约束 | 不能依赖 Scoped（captive dependency 检测） |

**检查项**：
- [ ] 确认所有 Singleton 工厂是幂等的（虽然只执行一次，但工厂内不应有副作用依赖）
- [ ] 确认 Singleton 不持有 `Scope` 或 `Scoped` 服务的 `Arc`（会延长 Scoped 生命周期）
- [ ] 确认 Singleton 工厂内 `#[inject(provider)]` 拿到的是 root provider（非 scope）

---

### 2.2 Scoped

| 属性 | 行为 |
|------|------|
| 缓存位置 | `root_scoped_cache`（per-`Scope` 独立 `LazyCache`） |
| 初始化时机 | 首次 `get()` 时懒初始化 |
| 生命周期 | = `Scope` 生命周期 |
| 隔离性 | 每个 `Scope` 独立缓存，互不影响 |
| 依赖约束 | 可依赖 Singleton / Scoped / Transient |

**检查项**：
- [ ] 确认每个 HTTP 请求 / 业务事务创建独立 `Scope`，请求结束 drop
- [ ] 确认 Scoped 服务不跨 `Scope` 传递（会破坏隔离性）
- [ ] 确认 child `Scope` 内的 Transient 依赖绑定到 child（非 root）— 已有测试验证
- [ ] 确认 `ScopeFactory` 注入到 Singleton 时，创建的 `Scope` 被正确释放

---

### 2.3 Transient

| 属性 | 行为 |
|------|------|
| 缓存位置 | 不缓存，每次 `get()` 新建 |
| 生命周期 | = 调用方持有的 `Arc<T>` 引用计数 |
| 依赖约束 | 可依赖 Singleton / Scoped / Transient |
| 循环依赖 | **运行时才暴露**（build 时不验证，会栈溢出） |

**检查项**：
- [ ] 确认 Transient 服务之间无循环依赖（A 依赖 B，B 依赖 A）
- [ ] 确认 Transient 服务的工厂是轻量的（每次解析都执行）
- [ ] 确认 Transient 服务持有 Scoped 依赖时，Scoped 绑定到正确的 scope

---

### 2.4 Owned

| 属性 | 行为 |
|------|------|
| 解析方式 | `try_unwrap(Arc<T>)` 获取所有权 |
| 适用生命周期 | Scoped / Transient（Singleton 返回 `None`） |
| 生命周期 | = 调用方拥有（值语义） |
| 线程安全 | `try_unwrap` 要求 `Arc` 引用计数为 1 |

**检查项**：
- [ ] 确认 Owned 服务不被其他地方持有 `Arc`（否则 `try_unwrap` 失败）
- [ ] 确认 `#[inject(owned)]` 字段类型是 bare `T` 或 `Option<T>`（非 `Arc<T>`）
- [ ] 确认 Singleton 服务未被 `get_owned()` 解析（会返回 `Err`）

---

### 2.5 Scope Drop 行为

**机制**：`Scope::drop()` 调用 `self.dispose()` → `dispose_scoped()`。`dispose_scoped` 用 `try_lock`，并发时跳过。

**检查项**：
- [ ] 确认 `Scope` drop 时无并发解析（否则 disposable 可能未执行）
- [ ] 确认关键资源不依赖 `Scope::drop` 执行 disposable（用显式 `dispose()` 或服务自身的 `Drop`）
- [ ] 确认 `Scope` 不在 `Arc` 中长期持有（应短生命周期）

---

## 三、异常处理机制

### 3.1 工厂 panic（sync 路径）

**机制**：sync 工厂 panic 自然传播到调用方。`OnceLock::get_or_init` 在工厂 panic 时不缓存，下次解析重试。

**行为**：
- `build()` 阶段：`ValidationResolver` 用 `catch_unwind` 捕获，转为 `RdiError`
- 运行时 `get()`：panic 传播到调用方，需调用方 `catch_unwind`
- 缓存：panic 后 `OnceLock` cell 保持未初始化，下次 `get()` 重新执行工厂

**检查项**：
- [ ] 确认 Singleton 工厂不会 panic（build 时会 panic 终止）
- [ ] 确认运行时 `get()` 调用方对 Transient/Scoped 工厂 panic 有兜底（`catch_unwind` 或上层错误处理）
- [ ] 确认 panic 后重试是安全的（工厂副作用可重复执行）

---

### 3.2 工厂 panic（async 路径）

**机制**：`run_async_factory_safely` 用 `AssertUnwindSafe(fut).catch_unwind().await` 捕获 panic，转为 `RdiError::FactoryPanic`。

**行为**：
- `get_async()` 返回 `Err(RdiError::FactoryPanic { service, payload })`
- 不 panic、不终止 task
- `FuturesMutex` guard 未填充，下次 `get_async()` 重试

**检查项**：
- [ ] 确认 `get_async()` 调用方处理 `Err(RdiError::FactoryPanic)`（不要 `.unwrap()`）
- [ ] 确认 async 工厂内 panic 后重试是安全的
- [ ] 确认 `RdiError::FactoryPanic` 的 `payload` 字段用于日志记录（非结构化字符串）

---

### 3.3 循环依赖检测

**机制**：`ValidationResolver` 在 `build()` 时检测循环依赖，输出有序路径（`A -> B -> C -> A`）。

**检测范围**：
- ✅ Singleton 的循环依赖（build 时执行工厂检测）
- ❌ Scoped / Transient 的循环依赖（运行时首次解析才暴露，栈溢出）

**检查项**：
- [ ] 确认所有 Singleton 的循环依赖在 build 时被检测（无运行时栈溢出）
- [ ] 确认 Scoped / Transient 服务无循环依赖（人工审查依赖图）
- [ ] 确认 `RdiError::CircularDependency` 的路径信息用于调试日志

---

### 3.4 captive dependency 检测

**机制**：`check_captive_dependency` 在 `build()` 时检测 Singleton 依赖 Scoped（含间接链 `Singleton → Transient → Scoped`）。

**检测范围**：
- ✅ 直接：Singleton 工厂内 `get::<Scoped>()`
- ✅ 间接：Singleton → Transient → Scoped（回溯查找任意 Singleton 祖先）
- ✅ Owned：Singleton 的 `#[inject(owned)]` 字段类型为 Scoped

**检查项**：
- [ ] 确认 build 时无 `RdiError::SingletonDependsOnScoped` 错误
- [ ] 确认 Transient 服务的 Scoped 依赖绑定到正确的 scope（非 root）

---

### 3.5 服务未找到

**机制**：
- `get::<T>()` 返回 `Result<Arc<T>, RdiError::ServiceNotFound>`
- `try_get::<T>()` 返回 `Option<Arc<T>>`
- `get_owned::<T>()` 返回 `Result<T, RdiError>`

**检查项**：
- [ ] 确认 `get()` 调用方处理 `Err`（不要无脑 `.unwrap()`）
- [ ] 确认可选依赖用 `try_get()` 而非 `get().ok()`
- [ ] 确认 `RdiError::ServiceNotFound` 的 `context` 字段用于定位解析链

---

### 3.6 key 冲突与多实现

**机制**：
- 非 keyed：同类型多次注册，后注册覆盖先注册
- keyed：`key` 必须唯一，`get_all::<T>()` 返回所有实现

**检查项**：
- [ ] 确认非 keyed 服务的注册顺序（后注册覆盖）
- [ ] 确认 keyed 服务的 key 唯一性（无拼写错误）
- [ ] 确认 `get_all::<T>()` 的消费方处理多实现（策略模式）

---

## 四、并发安全验证

### 4.1 OnceLock 缓存（sync Singleton）

**机制**：`OnceLock::get_or_init` 保证多线程下工厂只执行一次。

**检查项**：
- [ ] 确认 Singleton 工厂无共享可变状态（工厂只执行一次，但结果被多线程共享）
- [ ] 确认 Singleton 服务内部可变状态用 `Mutex` / `RwLock` / 原子操作保护

---

### 4.2 FuturesMutex 缓存（async Scoped）

**机制**：`get_or_init_with_async` 用 per-key `FuturesMutex` 保证 async factory 只执行一次。

**检查项**：
- [ ] 确认 async scoped 工厂无共享可变状态
- [ ] 确认 async scoped 服务内部可变状态用 `Mutex` / `RwLock` / 原子操作保护
- [ ] 确认同一 `Scope` 不会在多线程间共享（`Scope` 设计为单线程使用，多线程共享是反模式）

---

### 4.3 dispose/register 竞态

**机制**：`dispose_scoped` 用 `try_lock`，并发 `register_disposable` 时跳过。

**检查项**：
- [ ] 确认 `Scope` drop 时无并发解析（`try_lock` 失败会跳过 disposable）
- [ ] 确认关键资源有 `Drop` impl 兜底（不依赖 `register_disposable`）

---

## 五、性能验证

### 5.1 TypeId 直查

**机制**：`ServiceProvider::get_by_type_id` 用 `TypeId` O(1) 直查，`type_name` 字符串路径作为 fallback。

**检查项**：
- [ ] 确认热路径服务通过 `ServiceProvider` / `Scope` / `Wrapper` 解析（走 TypeId 直查）
- [ ] 确认非 `ServiceProvider` 实现者（如 `ValidationResolver`）仅用于 build 阶段（走 fallback）

---

### 5.2 Arc<Arc<T>> 双重包装

**机制**：trait 对象 `dyn Trait` 是 `?Sized`，`Any::downcast` 要求 `Sized`，故用 `Arc<Arc<T>>` 双重包装。

**检查项**：
- [ ] 确认解析热路径不频繁（每次解析两次 Arc 操作，约 2ns）
- [ ] 确认对性能极度敏感的场景用具体类型注入（非 `dyn Trait`），走 `extract` 的 `downcast::<Arc<T>>` 快速路径

---

### 5.3 基准测试参考

基于 `tests/bench_concurrent.rs`（4 worker threads，debug build）：

| 场景 | 耗时 | 说明 |
|------|------|------|
| 100 并发 async scoped（空工厂） | ~0.5ms | FuturesMutex 纯开销 |
| 100 并发 sync scoped（空工厂） | ~2ms | OnceLock + spawn_blocking 基线 |
| 100 并发 async scoped（10ms 工厂） | ~20ms | 串行需 1000ms，50x 加速 |
| 1000 并发 async scoped（空工厂） | ~3.4ms | 高并发无死锁 |

**结论**：`FuturesMutex` 开销可忽略（async 路径优于 sync 基线，因任务调度轻于线程切换）。

**检查项**：
- [ ] 确认 release build 性能（`cargo build --release` 比 debug 快 5-10x）
- [ ] 确认生产环境 worker threads 数量合理（通常 = CPU 核心数）

---

## 六、部署前最终检查

### 6.1 编译验证
- [ ] `cargo clippy --all-targets -- -D warnings` 零警告
- [ ] `cargo test --all` 全量通过
- [ ] `cargo doc --no-deps` 零警告

### 6.2 运行时验证
- [ ] 应用启动时无 `RdiError`（循环依赖、captive dependency、服务未找到）
- [ ] 应用关闭时 `drop(provider)` 释放所有 Singleton
- [ ] 长时间运行内存稳定（无 monotonic 增长，排查 async_locks 累积）

### 6.3 日志验证
- [ ] 工厂 panic 记录到日志（`RdiError::FactoryPanic` 的 `payload` 字段）
- [ ] 循环依赖路径记录到日志（`RdiError::CircularDependency` 的路径信息）
- [ ] captive dependency 记录到日志（`RdiError::SingletonDependsOnScoped` 的 singleton/scoped 名称）

### 6.4 跨 DLL 验证（如适用）
- [ ] DLL 卸载前 `remove_named()` 清理注册项
- [ ] 跨 DLL 传递 `Arc<dyn IProvider>` 而非裸指针
- [ ] DLL 的 `#[module]` 生成代码在卸载后不被调用
