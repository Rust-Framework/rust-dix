# 生产环境部署检查清单

> 适用于 `rust-dix` **v0.10.x**（当前 0.10.0，特性发布：宏 key 表达式 / typed resolver /
> 扩展契约）。基于源码静态分析 + 运行时行为验证编写。
> 0.9 引入、0.10 继续适用：类型化 keyed 键 / `AnyKey` 兜底 / `AsyncFactoryContext`（跨任务环检测）/
> 精确诊断，并把 async 环检测升级为解析链；MSRV **1.95**，两个 crate 均
> `#![forbid(unsafe_code)]`，CI 含 RustSec `cargo audit`，安全策略见 `SECURITY.md`。
> 需要兼容 0.7.x / 0.8.x 的地方会注明。

## 0. 0.10.x 语义变更摘要（部署前必读）

完整语义见 [语义权威页（0.10）](semantics.md)，错误见 [错误目录](error-catalog.md)；
宏 key/自定义 resolver/async AnyKey 边界见 [扩展契约](extension-points.md)；
本清单只列**会影响生产部署**的 5 条：

| 变更 | 生产影响 |
|------|----------|
| 最后注册优先 + 严格 scope 校验 | 覆盖关系检查注册顺序/`try_add*`；root 解析 Scoped 失败，请求级服务必须在 `scope()` 内解析（owned 例外） |
| `get_all` / `Vec` 仅非 keyed、含已初始化 async singleton | 广播逻辑用 `get_all`；keyed 用 `get_all_keyed`；未初始化 async 枚举会 panic/`Err` |
| 类型化键 / AnyKey（0.9 起，0.10 同样适用） | keyed API 取 `impl Into<ServiceKey>`；AnyKey 按 (注册/类型, key) 缓存、预填充/owned/嵌套有例外；单值 `any_key()` 非法 |
| async 诊断/环检测（0.9 起，0.10 同样适用） | 同步解析 async → `AsyncServiceRequiresGetAsync`；同步 singleton 不得依赖 async；跨任务传 `AsyncFactoryContext`；`is_circular()` 启发式 |
| 供应链/安全（0.9 起，0.10 同样适用） | MSRV 1.95、`forbid(unsafe_code)`、CI matrix + fmt/clippy/doc + RustSec `cargo audit`、`SECURITY.md` |

---

## 一、内存泄漏风险

### 1.1 Singleton 缓存释放

**机制**：`LazyCache` 的 singleton cell 在 `ServiceProvider` 生命周期内保持。0.8.0 起（0.10.x 同样适用）
`ServiceProvider::dispose()` 会按**创建逆序**释放全部 singleton 与 root scoped 实例、
逆序执行 `register_disposable` 清理闭包、清空 named 注册，并标记 provider 已释放
（此后解析返回 `Err(ProviderDisposed)`）。**仅 drop `Arc`/不调用 dispose 不会主动执行
这些清理**。

**风险**：`ServiceProvider` 长期存活（如全局 `lazy_static` / `OnceLock`）且从不
`dispose()` 时，所有 Singleton 永不释放。

**检查项**：
- [ ] 确认应用关闭路径显式调用 `provider.dispose()`
- [ ] 确认没有 `static` 变量长期持有 `Arc<ServiceProvider>`（用 `Weak` 替代）
- [ ] 确认 Singleton 服务不持有大对象（如未限流的缓存）而不提供清理接口
- [ ] 确认 dispose 后没有代码继续使用该 provider（否则拿到 `ProviderDisposed`）

**缓解**：应用关闭时显式 `provider.dispose()`；或在确定性生命周期内 `drop(provider)`。

---

### 1.2 async_locks 累积

**机制**：`LazyCache.async_locks: HashMap<usize, Arc<FuturesMutex<Option<AnyService>>>>` 在首次 async 解析时按 key 创建；**随所属 `LazyCache`（provider/scope）的生命周期存在，不单独清理**（provider 级缓存直到 provider dispose；scope 级缓存随 scope drop 释放）。

**风险**：同一容器/作用域内解析大量不同类型的 async 服务 → 每个 key 额外占一个 `FuturesMutex`（约 40 字节）。

**检查项**：
- [ ] 确认 `Scope` 生命周期短（HTTP 请求级别，请求结束即 drop）
- [ ] 确认同一 `Scope` 内 async scoped 服务类型数量可控（通常 < 50）
- [ ] 确认没有长期存活的 `Scope`（如全局 `Scope` 池）

**缓解**：scope 级 `async_locks` 随 `Scope` drop 释放（`scope_provider` 的 `LazyCache`）；provider 级随 `provider.dispose()` 释放。长期存活的 provider 上，按 key 数量与 key 复用情况评估增量。

---

### 1.3 disposables / scoped 实例未执行导致资源泄漏

**机制**：`Scope` 自动跟踪该 scope 创建的 Scoped 实例。0.8.0 起（0.10.x 同样适用）
`Scope::dispose()`/Drop 先按**创建逆序**释放这些实例，再按**注册逆序**执行
`register_disposable` 清理闭包。实现会先取出待释放队列、**释放锁再执行**用户代码，
避免持锁回调导致重入死锁。

**风险**：若清理闭包自身再次触发长时间阻塞的解析，可能拖慢 scope 结束；非内存资源仍建议在服务 `Drop` 中兜底释放。

**检查项**：
- [ ] 确认 `Scope` 在业务终点显式调用 `scope.dispose()`（`Drop` 会作为安全网）
- [ ] 确认关键资源在 `dispose()` 之外有兜底释放（如 `Drop` impl on service struct）
- [ ] 确认 `register_disposable` 的闭包本身不会再调用 `dispose()` 形成逻辑环
- [ ] 确认 dispose 后的 scope 不再被使用（解析返回 `Err(ScopeDisposed)`）

**缓解**：在业务代码中，scope 使用模式应为：
```rust
let scope = provider.scope();
// ... 使用 scope 解析服务 ...
scope.dispose();  // 显式 dispose：逆序释放 scoped 实例 + 逆序执行清理闭包
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
| 初始化时机 | 同步 singleton：`build()` / `build_async()` 时 eager init（工厂执行一次，**按注册顺序创建**，0.8.0 起（0.10.x 同样适用）顺序确定）；async singleton：`build_async()` 时**依赖驱动递归初始化**（0.8.1 起注册顺序无关，最终为依赖优先拓扑序）。此时 provider `Arc` 已创建，工厂内可解析 `#[inject(provider)]` / 内置 `ScopeFactory`（0.7.1+） |
| async 初始化约束 | async singleton 可 `await` 任意（含后注册的）async singleton，会先递归初始化依赖；**依赖图必须无环**——自引用/交叉依赖返回 `RdiError::CircularDependency`（不会 hang）；`build_async` 返回时所有 async singleton 均已初始化 |
| build 期 async 解析 | async singleton **初始化完成前只能用 `get_async` 解析**（工厂内 `sp.get_async::<T>()` 会递归初始化依赖）；此时同步 `get` 返回 `Err(RdiError::AsyncServiceRequiresGetAsync)`、`get_optional` 为 `None`，但 `is_registered` 仍为 `true`。`build_async` 返回后已缓存的 async singleton 可直接 `get` |
| 生命周期 | = `ServiceProvider` 生命周期，直到 `provider.dispose()` |
| 释放顺序 | `dispose()` 按**创建逆序**释放；`register_disposable` 清理闭包按**注册逆序**执行 |
| 线程安全 | `OnceLock::get_or_init` 保证工厂只执行一次 |
| 依赖约束 | 不能依赖 Scoped（captive dependency 检测）；**同步 singleton 的依赖图里不能出现 async singleton**——`build_async` 返回精确诊断 `Err(RdiError::SyncSingletonDependsOnAsyncService { singleton, async_service })`（不 hang），因为同步 singleton 的校验/构建（Phase 2/3）先于 async 初始化（Phase 4） |
| AnyKey 兜底 | `keyed_any_*` 注册**不做 eager 初始化/单独校验**（需要请求 key；build 期校验中同步 singleton 工厂可经 `get_keyed_any` 按请求 key 解析，结果预填充到 **(注册, key) 缓存**、运行时同注册同 key 复用同一实例、多消费者只构造一次）；singleton/scoped 兜底按 (服务注册/类型, 请求 key) 缓存（不同注册同 key 独立）；`get_all_keyed` 不含兜底；仅同步注册；scoped/transient 的 `get_keyed_owned` 在 strict root 允许（owned 例外） |

**检查项**：
- [ ] 确认所有 Singleton 工厂是幂等的（虽然只执行一次，但工厂内不应有副作用依赖）
- [ ] 确认 Singleton 不持有 `Scope` 或 `Scoped` 服务的 `Arc`（会延长 Scoped 生命周期）
- [ ] 确认 Singleton 工厂内 `#[inject(provider)]` 拿到的是 root provider（非 scope）
- [ ] 需要按任务建 scope 的 Singleton 优先注入 `Arc<dyn ScopeFactory>`（内置、弱引用，0.7.1+ 在 eager singleton 中可用）；仅确需容器自身时才用 `#[inject(provider)]`
- [ ] 确认 Singleton 构造函数内不通过注入的 provider 做动态解析（框架会在 build 期返回 `FactoryPanic` 拒绝，避免绕过 captive dependency 校验）
- [ ] 确认依赖构建顺序的初始化逻辑基于"**同步 singleton 按注册顺序 / async singleton 依赖优先拓扑序**"设计（两者均确定，不依赖 `HashMap`）
- [ ] 确认 async singleton 依赖图**无环**（自引用/交叉依赖会返回 `RdiError::CircularDependency`，不会死锁/挂起；注册顺序无关）
- [ ] 确认同步 singleton 的依赖图**不含 async singleton**（含则 `build_async` 返回 `SyncSingletonDependsOnAsyncService`）；需要时把依赖方也改为 async singleton，或在方法内用 `Arc<dyn ScopeFactory>` / `get_async` 延迟解析
- [ ] 确认 build 期解析 async singleton 只用 `get_async`（初始化完成前同步 `get` 会返回 `AsyncServiceRequiresGetAsync`、`get_optional` 为 `None`；`is_registered` 仍为 `true`）
- [ ] 确认 async 工厂在 spawned task 中解析依赖时传 `AsyncFactoryContext`（否则跨任务环可能死锁；环错误用 `is_circular()` 判定，启发式）
- [ ] 确认启动期没有同步枚举未初始化的 async 服务（`get_all` 对未初始化 async singleton/transient typed panic；async scoped 在 root 被 scope 校验省略；`Vec` 注入对任何未初始化 async 返回 `Err`；async scoped/transient 用 `get_async` 逐项解析）

---

### 2.2 Scoped

| 属性 | 行为 |
|------|------|
| 缓存位置 | `scoped_cache`（per-`Scope` 独立 `LazyCache`） |
| 初始化时机 | 首次 `get()` 时懒初始化 |
| 生命周期 | = `Scope` 生命周期；`dispose()`/Drop 按**创建逆序**释放 |
| 隔离性 | 每个 `Scope` 独立缓存，互不影响 |
| 依赖约束 | 可依赖 Singleton / Scoped / Transient |
| root 解析 | **0.8.0 起（0.10.x 同样适用）默认拒绝**：从 root provider 解析 Scoped（含经 Transient 链间接）返回 `Err(ScopedFromRootProvider)`；0.7.x 旧行为需 `build_with(ServiceProviderOptions::lenient())`。例外：`get_owned`/`try_get_owned` 可从 root 取 Scoped（归调用方、不缓存；其 Scoped 依赖仍受校验） |

**检查项**：
- [ ] 确认每个 HTTP 请求 / 业务事务创建独立 `Scope`，请求结束显式 `dispose()` 或 drop
- [ ] 确认没有代码从 root provider 解析 Scoped（严格校验下会得到 `ScopedFromRootProvider`）
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
| 循环依赖 | **运行时检测**（线程局部解析栈 → `RdiError::CircularDependency`） |

**检查项**：
- [ ] 确认 Transient 服务之间无循环依赖（A 依赖 B，B 依赖 A）
- [ ] 确认 Transient 服务的工厂是轻量的（每次解析都执行）
- [ ] 确认 Transient 服务持有 Scoped 依赖时，Scoped 绑定到正确的 scope

---

### 2.4 Owned

| 属性 | 行为 |
|------|------|
| 解析方式 | `try_unwrap(Arc<T>)` 获取所有权 |
| 适用生命周期 | Scoped / Transient（Singleton 返回 `Err(OwnedNotSupported)` / `None`） |
| 生命周期 | = 调用方拥有（值语义）；**不进入容器跟踪/释放** |
| 线程安全 | `try_unwrap` 要求 `Arc` 引用计数为 1 |
| root 解析 | **owned 解析是严格 scope 校验的例外**：root 上对 Scoped 调 `get_owned`/`try_get_owned` 允许（实例归调用方、不进缓存/不被跟踪）；但其构造函数内的 Scoped 依赖仍受校验（会返回 `ScopedFromRootProvider`） |

**检查项**：
- [ ] 确认 Owned 服务不被其他地方持有 `Arc`（否则 `try_unwrap` 失败）
- [ ] 确认 `#[inject(owned)]` 字段类型是 bare `T` 或 `Option<T>`（非 `Arc<T>`）
- [ ] 确认 Singleton 服务未被 `get_owned()` 解析（会返回 `Err`）
- [ ] 确认 Scoped 的 `get_owned` 用法：root 上允许（owned 不缓存），但若其构造函数依赖其他 Scoped 服务，需在 scope 内解析
- [ ] 确认 Owned 实例的清理由调用方负责（不指望 scope/provider dispose 释放它）

---

### 2.5 Scope Drop 与 dispose

**机制**：`Scope` 自动跟踪该 scope 创建的 Scoped 实例。`Scope::dispose()`（`Drop` 会调用）
先按**创建逆序**释放这些实例，再按**注册逆序**执行 `register_disposable` 清理闭包；
dispose 后解析返回 `Err(ScopeDisposed)`（不可返回错误的 API 为 `None`/空）。dispose 幂等。

**检查项**：
- [ ] 确认 `Scope` 在业务终点显式调用 `dispose()`（`Drop` 仍会兜底，但显式更清晰）
- [ ] 确认关键资源不依赖 `Scope::drop` 执行 disposable（用显式 `dispose()` 或服务自身的 `Drop`）
- [ ] 确认 `Scope` 不在 `Arc` 中长期持有（应短生命周期）
- [ ] 确认 dispose 后的 scope 不再被传递使用（否则解析返回 `ScopeDisposed`）

---

### 2.6 ServiceProvider dispose

**机制**：`ServiceProvider::dispose()` 按**创建逆序**释放全部 singleton 与 root scoped
实例、按**注册逆序**执行清理闭包、清空 named 注册，并标记 provider 已释放；之后解析
返回 `Err(ProviderDisposed)`。幂等。

**检查项**：
- [ ] 确认应用关闭路径显式调用 `provider.dispose()`（仅靠 `Arc` 引用归零不会释放缓存）
- [ ] 确认 dispose 前已停止后台 worker（它们通过 `ScopeFactory` 仍会尝试建 scope）
- [ ] 确认 dispose 后没有模块继续解析服务（`ProviderDisposed`）
- [ ] 确认跨 DLL 场景在 dispose 前已 `remove_named()` 或随 dispose 清空
- [ ] 确认 `register_disposable` 的清理闭包在 `dispose()` **之前**注册（dispose 之后注册的闭包不会执行）

---

## 三、异常处理机制

### 3.1 sync 工厂 panic / 失败（0.10.x 语义）

**机制**：0.8.0 起（0.10.x 同样适用）同步解析路径不再把工厂 panic 直接抛给业务调用方，而是统一转成类型化
`RdiError`。`OnceLock` 类缓存在工厂 panic 时不会写入（cell 保持未初始化），下次解析会重试。

**行为**：
- **build 阶段**：`ValidationResolver` 用 `catch_unwind` 捕获，转为 `RdiError`；
  `build()` / `build_async()` 返回 `Err`（不是"panic 终止"）。
- **required-resolution API**（`get` / `get_owned` / `get_async` / `get_keyed*` 等）：
  内部捕获工厂 panic 与框架级失败（scope 校验、已 dispose、循环、构造期解析拒绝），返回
  `Err(RdiError::FactoryPanic { service, payload })` 或对应 typed 错误；调用方按 `Result`
  处理即可，**不需要**自己 `catch_unwind`。
- **trait 层 Option 方法**（`IServiceResolver::get_any` / `get_keyed_any` / `get_by_type_id` /
  `get_keyed_by_type_id`）：签名不可失败，违规或失败时以 typed `RdiError` payload **panic**，
  供 required API 捕获后转为 `Err`；不要把它们当作"探测"接口。
- **可选/探测 API**（`get_optional` / `try_get` / `try_get_keyed` / `try_get_owned` 等）：
  返回 `None`，不 panic（未注册、strict root scope 校验、已 dispose 或解析失败均视为不可解析）。
- **枚举 API**（`get_all` / `get_all_keyed`）：签名不可失败；已 dispose 返回空、root scope
  校验导致的不可解析被省略，但**其它解析错误（如工厂 panic）仍以 typed `RdiError` payload
  panic 传播**（对应 MEDI 中枚举失败抛异常）。

**检查项**：
- [ ] 确认 Singleton 工厂不会 panic（build 会返回 `Err`，不是 panic 终止）
- [ ] 确认 required API 调用方处理 `Err(RdiError::FactoryPanic { .. })` 等 typed 错误，而非依赖 `catch_unwind`
- [ ] 确认可选依赖用 `get_optional` / `try_get`（不 panic），不要用 `get().ok()` 吞掉失败语义
- [ ] 确认 `get_all` / `get_all_keyed` 的调用方了解"签名不可失败、非 scope 类错误会 typed panic"的契约
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
- ✅ Singleton 依赖图（build 时执行工厂检测；含经 Transient 中间节点的间接环）
- ✅ Scoped / Transient 的同步循环依赖（运行时线程局部解析栈 → `RdiError::CircularDependency`，不再静默栈溢出）
- ✅ **async 解析链**：`get_async` / `get_keyed_async` 的 future 在构造时捕获**解析链**
  （0.9.0 由"单次 poll"升级为链式），`async_scoped` / `async_transient` 的自引用/交叉递归、
  **async singleton 的依赖环** 均返回 `RdiError::CircularDependency`，从而**避免在自有的
  per-key async 缓存锁上死锁**。
- ✅ **跨任务**：在 async 工厂内捕获 `sp.async_context()` 并 clone 进 spawned task，经 ctx
  解析会继承链——跨任务环同样返回 `CircularDependency`；完全不传 ctx 的完全脱离任务仍可能死锁。
- ✅ 通过注入 provider 发起的动态解析同样参与解析栈/链检测
- ❌ 构造函数内动态解析被直接拒绝（`FactoryPanic`），因此无法借它绕过循环检测

> **不会误报**：链式检测只沿解析链传播，跨 `await` 的合法并发解析（不同任务、甚至同一线程
> 交替 poll）不属于同一条链，因此正常并发不会被判成循环；async 工厂内也**不得**递归
> `await` 自身正在构建的服务（会得到 `CircularDependency`，而不是挂死）。

> **错误形态（0.9.0）**：嵌套解析得到 `RdiError::CircularDependency(_)`；若用户 async 工厂
> 对该错误 `.unwrap()` / `.expect()`，panic 会被捕获并转为**顶层** `RdiError::FactoryPanic { payload }`，
> 其中 `payload` 含 `"CircularDependency(...)"`。推荐用 `RdiError::is_circular()` 判定；注意它是
> **启发式**（payload 含该文本即命中，自定义 payload 可能误判），精确形态时优先匹配 typed 变体：
> - `get_async` 侧可用 `matches!(err, RdiError::CircularDependency(_))`；
> - `build_async` 顶层可用
>   `matches!(err, RdiError::FactoryPanic { ref payload, .. } if payload.contains("CircularDependency"))`
>   或 `err.is_circular()`（启发式）。

**检查项**：
- [ ] 确认所有 Singleton 的循环依赖在 build 时被检测
- [ ] 确认 Scoped / Transient 同步循环依赖在首次解析时返回 `Err(RdiError::CircularDependency)`（调用方处理，勿 `.unwrap()`）
- [ ] 确认 async singleton 依赖图无环（`build_async` 依赖驱动初始化，有环返回 `CircularDependency`、不 hang；注册顺序无关）
- [ ] 确认 async 工厂不会递归 `await` 自身正在构建的服务（应得到 `CircularDependency` 而非死锁）
- [ ] 确认跨 `await` 的合法并发解析不被误报为循环依赖
- [ ] 确认 cross-task 场景传 `AsyncFactoryContext`（否则可能死锁）
- [ ] 确认环错误形态：`get_async` 侧为 `CircularDependency`；工厂 `.unwrap()` 后 `build_async` 顶层为 `FactoryPanic`（payload 含 `CircularDependency`），用 `RdiError::is_circular()`（**启发式**）或 typed 匹配判定
- [ ] 确认 `get_all` / `#[inject] Vec<Arc<T>>` 的消费方不依赖"同步枚举 async"（已初始化 async singleton 会包含；未初始化 async singleton/transient typed panic 或 Err；async scoped 在 root 被 scope 校验省略）
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

### 3.5 服务未找到 / 不可解析

**机制**：
- `get::<T>()` / `get_keyed::<T>(key)` 返回 `Result<Arc<T>, RdiError>`：
  未注册为 `ServiceNotFound` / `KeyedServiceNotFound`；另可能是
  `ScopedFromRootProvider` / `ScopeDisposed` / `ProviderDisposed` 等 typed 错误；
  **同步解析 async 服务**为 `AsyncServiceRequiresGetAsync`（0.9.0 新增精确错误）
- `try_get::<T>()` / `try_get_keyed::<T>(key)` 返回 `Option<Arc<T>>`：未注册、
  strict root scope 校验不可解析、已 dispose 或解析失败时均为 `None`（**不 panic**）
- `get_owned::<T>()` 返回 `Result<T, RdiError>`（Singleton 为 `OwnedNotSupported`）
- `get_keyed::<T>(key)` 的 key 为 `impl Into<ServiceKey>`（字符串或类型化键）；
  单值不能用 `ServiceKey::any_key()`

**检查项**：
- [ ] 确认 `get()` 调用方处理 `Err`（不要无脑 `.unwrap()`）
- [ ] 确认可选依赖用 `try_get()` 而非 `get().ok()`（`try_get` 对框架级失败不 panic）
- [ ] 确认 `RdiError::ServiceNotFound` 的 `context` 字段用于定位解析链
- [ ] 确认 dispose 后仍被使用的 provider/scope 会得到 typed 错误（`ProviderDisposed` / `ScopeDisposed`），而不是静默失败

---

### 3.6 key 冲突、类型化键与 AnyKey

**机制**（完整语义见 [语义权威页（0.10） §1-§2](semantics.md)）：
- 非 keyed：同类型多次注册最后生效；keyed 同 key 重复注册最后生效，`#[module]` 重复 key 编译期错误。
- **类型化键/AnyKey**：keyed API 取 `impl Into<ServiceKey>`；字符串键用 `ServiceKey::from("x")`；AnyKey 精确优先、按 (注册/类型, key) 缓存、validation 预填充、owned 例外、嵌套不同 key 合法、单值 `any_key()` 非法。
- **枚举与 async**：`get_all`/`get_all_keyed` 含已初始化 async singleton；未初始化 async → `AsyncServiceRequiresGetAsync`（`get_all` typed panic / `Vec` Err；async scoped 在 root 被 scope 校验省略）。

**检查项**：
- [ ] 确认非 keyed 服务的注册顺序（**最后注册优先**；需要"未注册才添加"用 `try_add*`）
- [ ] 确认 keyed 服务的 key 唯一性（无拼写错误；同 key 重复注册会静默覆盖）
- [ ] 确认类型化 key 的相等语义（不同类型/值不相等，`1u32` 与 `"1"` 不等；字符串键不用 `ServiceKey::new`）
- [ ] 确认 AnyKey 兜底的精确-key 优先与"按 (服务注册/类型, 请求 key) 缓存"符合预期；validation 预填充实例运行时复用
- [ ] 确认 AnyKey 的 scoped/transient owned 解析按 owned 例外处理（不进缓存）
- [ ] 确认 AnyKey 工厂内嵌套解析不同请求 key 不会被误判为环
- [ ] 确认 `get_all_keyed` 的消费方不假设包含 AnyKey 兜底/非 keyed 注册
- [ ] 确认同步枚举的消费方不会遇到未初始化 async 服务（否则用 `get_async` 逐项解析）
- [ ] 确认没有用 `ServiceKey::any_key()` 做单值解析
- [ ] 确认未使用 `async_any_*`（0.10 未提供）；动态 key 异步构建按 [扩展契约](extension-points.md#4-async-anykey-边界010-仍未提供) 的 workaround 处理
- [ ] 0.7.x/0.8.x 升级：确认没有依赖"先注册优先"的覆盖逻辑

---

### 3.7 Scope 校验错误

**机制**：0.8.0 起默认严格 scope 校验（0.10.x 同样适用）：root provider 解析 Scoped 服务（含经 Transient
链间接）返回 `Err(RdiError::ScopedFromRootProvider { service })`；
`get_optional` / `get_all` / `get_all_keyed` / `try_*` 视为不可解析（`None`/空）。
**例外**：`get_owned`/`try_get_owned` 允许从 root 解析 Scoped（实例归调用方、不缓存），
但其构造函数内的 Scoped 依赖仍受校验。`provider.scope()` 内解析不受影响。

**检查项**：
- [ ] 确认启动自检没有在 root 上解析 Scoped（会得到 `ScopedFromRootProvider`）
- [ ] 确认请求/任务级代码都从 `scope` 解析 Scoped
- [ ] 如确需 0.7.x 的 root-scope 行为，确认是**显式** `build_with(ServiceProviderOptions::lenient())` 并注释原因

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

**机制**：0.8.0 起（0.10.x 同样适用） `Scope` 既跟踪 scoped 实例也维护 `disposables` 队列。`dispose()` **等待**
`disposables` 锁（不再 `try_lock` 静默跳过），取出队列后释放锁再执行用户清理闭包，避免
持锁调用用户代码导致重入死锁。`Scope::dispose()` 与 `ServiceProvider::dispose()` 均幂等，
重复调用不会重复释放。

**检查项**：
- [ ] 确认 `register_disposable` 闭包不会再次调用 `dispose()` 形成逻辑环
- [ ] 确认关键资源有 `Drop` impl 兜底（不依赖 `register_disposable`）
- [ ] 确认 dispose 与并发请求/worker 的边界：先停止新请求/worker，再 dispose provider
- [ ] 确认 dispose 后没有线程继续使用该 provider/scope（否则拿到 `ProviderDisposed` / `ScopeDisposed`）

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
- [ ] 工具链 **Rust ≥ 1.95**（`Cargo.toml` 的 `rust-version`；CI matrix 覆盖 1.95 与 stable）
- [ ] `cargo fmt --all -- --check` 零差异
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 零警告
- [ ] `cargo test --workspace` 全量通过
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` 零警告
- [ ] `cargo audit`（RustSec）无未处理漏洞

### 6.2 运行时验证
- [ ] 应用启动时无 `RdiError`（循环依赖、captive dependency、服务未找到、`ScopedFromRootProvider`）
- [ ] 应用关闭时显式 `provider.dispose()`，按创建逆序释放所有 Singleton 与 root scoped 实例
- [ ] 长时间运行内存稳定（无 monotonic 增长，排查 provider/scope 的 `async_locks` 按 key 累积）
- [ ] 关闭后异步任务/worker 不再解析已 dispose 的 provider（期望 `ProviderDisposed`，不应 panic 或静默）

### 6.3 日志验证
- [ ] 工厂 panic 记录到日志（`RdiError::FactoryPanic` 的 `payload` 字段）
- [ ] 循环依赖路径记录到日志（`RdiError::CircularDependency` 的路径信息；包裹形态用 `RdiError::is_circular()` 判定，可结合 typed 变体避免启发式误判）
- [ ] captive dependency 记录到日志（`RdiError::SingletonDependsOnScoped` 的 singleton/scoped 名称）
- [ ] 同步→async 依赖、同步解析/枚举 async 服务记录日志（`SyncSingletonDependsOnAsyncService` / `AsyncServiceRequiresGetAsync`）

### 6.4 跨 DLL 验证（如适用）
- [ ] DLL 卸载前 `remove_named()` 清理注册项
- [ ] 跨 DLL 传递 `Arc<dyn IProvider>` 而非裸指针
- [ ] DLL 的 `#[module]` 生成代码在卸载后不被调用

---

## 七、供应链与安全（0.9.0）

**机制**：
- **MSRV 1.95**：`Cargo.toml` 的 `rust-version = "1.95"`，CI matrix（ubuntu/windows × 1.95/stable）
  实测；发布工具链不得低于该版本。
- **CI（`.github/workflows/ci.yml`）**：测试矩阵跑 `cargo test --workspace`；stable 额外跑
  `cargo fmt --check`、`cargo clippy -D warnings`、`RUSTDOCFLAGS=-D warnings cargo doc`；
  另有 RustSec `cargo audit` job（`rustsec/audit-check`）。发布由 `publish.yml` 在 `v*` tag 触发。
- **`#![forbid(unsafe_code)]`**：两个 crate 均禁止 `unsafe`；同步使用 std 的
  `Mutex` / `RwLock` / `OnceLock`。
- **`SECURITY.md`**：漏洞通过 GitHub 私密 advisories 上报（目标 72h 内响应、30 天内修复），
  声明支持版本（0.10.x；0.8.x 仅安全修复）与安全设计/边界（无网络/文件访问、panic containment、
  dispose 契约、环安全、供应链审计）。

**检查项**：
- [ ] 确认构建/发布环境 Rust ≥ 1.95
- [ ] 确认 CI 的 fmt / clippy / doc / `cargo audit` 全部通过
- [ ] 确认部署锁文件（`Cargo.lock`）经过 `cargo audit`，无未处理漏洞
- [ ] 确认漏洞上报与响应流程按 `SECURITY.md` 执行
- [ ] 确认没有引入 `unsafe`（两 crate `#![forbid(unsafe_code)]`，新增依赖需复核其 unsafe 使用）
