# 生产环境部署检查清单

> 适用于 `rust-dix` **v0.8.x**（0.8.0 起）。基于源码静态分析 + 运行时行为验证编写。
> 文中标注"0.8.0+"的条目是 0.8.0 新行为；需要兼容 0.7.x 的地方会注明。

## 0. 0.8.0 语义变更摘要（部署前必读）

| 变更 | 生产影响 |
|------|----------|
| 单服务解析改为**最后注册优先**（MEDI 语义） | 覆盖关系依赖"先注册优先"的代码要调整注册顺序或用 `try_add*` |
| `get_all` / `#[inject] Vec<Arc<T>>` **只含非 keyed** | 观察者/广播逻辑若依赖 keyed 实现，改用 `get_all_keyed` |
| 默认**严格 scope 校验** | root provider 解析 Scoped 返回 `Err(ScopedFromRootProvider)`；请求级服务必须在 `provider.scope()` 内解析；旧行为需显式 `ServiceProviderOptions::lenient()`。例外：`get_owned`/`try_get_owned` 可从 root 取 Scoped（owned 不缓存），其 Scoped 依赖仍受校验 |
| 新增 `Scope::dispose()` / `ServiceProvider::dispose()` | Scope 逆序释放 Scoped 实例；provider 逆序释放 singleton/root scoped 并清空 named 注册；dispose 后解析分别返回 `ScopeDisposed` / `ProviderDisposed` |
| eager singleton 创建顺序确定（按注册顺序） | 不再依赖 `HashMap` 迭代顺序，初始化/释放顺序可复现 |

---

## 一、内存泄漏风险

### 1.1 Singleton 缓存释放

**机制**：`LazyCache` 的 singleton cell 在 `ServiceProvider` 生命周期内保持。0.8.0 起
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

**机制**：`LazyCache.async_locks: HashMap<usize, Arc<FuturesMutex<Option<AnyService>>>>` 在首次 async scoped 解析时创建，永不清理。

**风险**：单个 `Scope` 内解析大量不同类型的 async scoped 服务 → 每个 key 额外占一个 `FuturesMutex`（约 40 字节）。

**检查项**：
- [ ] 确认 `Scope` 生命周期短（HTTP 请求级别，请求结束即 drop）
- [ ] 确认同一 `Scope` 内 async scoped 服务类型数量可控（通常 < 50）
- [ ] 确认没有长期存活的 `Scope`（如全局 `Scope` 池）

**缓解**：`Scope` drop 时 `async_locks` 随之释放（`LazyCache` 被 `ServiceProvider` 拥有，`Scope` drop 释放 `scope_provider`）。

---

### 1.3 disposables / scoped 实例未执行导致资源泄漏

**机制**：`Scope` 自动跟踪该 scope 创建的 Scoped 实例。0.8.0 起
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
| 初始化时机 | 同步 singleton：`build()` / `build_async()` 时 eager init（工厂执行一次，**按注册顺序创建**，0.8.0 起顺序确定）；async singleton：`build_async()` 按**注册顺序**初始化（依赖须先注册）。此时 provider `Arc` 已创建，工厂内可解析 `#[inject(provider)]` / 内置 `ScopeFactory`（0.7.1+） |
| async 注册顺序约束 | async singleton 在初始化期解析"**后注册、尚未初始化**"的 async singleton → 返回带 `registration order` 提示的 `RdiError::FactoryPanic`；请把被依赖方先注册 |
| 生命周期 | = `ServiceProvider` 生命周期，直到 `provider.dispose()` |
| 释放顺序 | `dispose()` 按**创建逆序**释放；`register_disposable` 清理闭包按**注册逆序**执行 |
| 线程安全 | `OnceLock::get_or_init` 保证工厂只执行一次 |
| 依赖约束 | 不能依赖 Scoped（captive dependency 检测） |

**检查项**：
- [ ] 确认所有 Singleton 工厂是幂等的（虽然只执行一次，但工厂内不应有副作用依赖）
- [ ] 确认 Singleton 不持有 `Scope` 或 `Scoped` 服务的 `Arc`（会延长 Scoped 生命周期）
- [ ] 确认 Singleton 工厂内 `#[inject(provider)]` 拿到的是 root provider（非 scope）
- [ ] 需要按任务建 scope 的 Singleton 优先注入 `Arc<dyn ScopeFactory>`（内置、弱引用，0.7.1+ 在 eager singleton 中可用）；仅确需容器自身时才用 `#[inject(provider)]`
- [ ] 确认 Singleton 构造函数内不通过注入的 provider 做动态解析（框架会在 build 期返回 `FactoryPanic` 拒绝，避免绕过 captive dependency 校验）
- [ ] 确认依赖构建顺序的初始化逻辑基于"按注册顺序创建"设计（0.8.0 起确定；旧版依赖 HashMap 顺序）
- [ ] 确认 async singleton 的依赖**先注册**（`build_async` 按注册顺序初始化；解析后注册且未初始化的 async singleton 会返回带 `registration order` 提示的 `FactoryPanic`）

---

### 2.2 Scoped

| 属性 | 行为 |
|------|------|
| 缓存位置 | `scoped_cache`（per-`Scope` 独立 `LazyCache`） |
| 初始化时机 | 首次 `get()` 时懒初始化 |
| 生命周期 | = `Scope` 生命周期；`dispose()`/Drop 按**创建逆序**释放 |
| 隔离性 | 每个 `Scope` 独立缓存，互不影响 |
| 依赖约束 | 可依赖 Singleton / Scoped / Transient |
| root 解析 | **0.8.0 起默认拒绝**：从 root provider 解析 Scoped（含经 Transient 链间接）返回 `Err(ScopedFromRootProvider)`；0.7.x 旧行为需 `build_with(ServiceProviderOptions::lenient())`。例外：`get_owned`/`try_get_owned` 可从 root 取 Scoped（归调用方、不缓存；其 Scoped 依赖仍受校验） |

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

### 3.1 sync 工厂 panic / 失败（0.8.0 语义）

**机制**：0.8.0 起同步解析路径不再把工厂 panic 直接抛给业务调用方，而是统一转成类型化
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
- ✅ **async 解析链**：`async_scoped` / `async_transient` 的**自引用/交叉递归**在**同一 poll 链路**内检测
  （`AsyncPollGuardFuture` 只在单次 poll 期间把"正在解析"标记暴露给嵌套解析），返回
  `RdiError::CircularDependency`，从而**避免在自有的 per-key async 缓存锁上死锁**。
- ✅ 通过注入 provider 发起的动态解析同样参与解析栈/poll 链检测
- ❌ 构造函数内动态解析被直接拒绝（`FactoryPanic`），因此无法借它绕过循环检测

> **不会误报**：跨 `await` 的合法并发解析（不同任务、甚至同一线程交替 poll）看不到彼此的
> 单次 poll 标记，因此正常并发不会被判成循环；async 工厂内也**不得**递归 `await` 自身正在
> 构建的服务（会得到 `CircularDependency`，而不是挂死）。

**检查项**：
- [ ] 确认所有 Singleton 的循环依赖在 build 时被检测
- [ ] 确认 Scoped / Transient 同步循环依赖在首次解析时返回 `Err(RdiError::CircularDependency)`（调用方处理，勿 `.unwrap()`）
- [ ] 确认 async 工厂不会递归 `await` 自身正在构建的服务（应得到 `CircularDependency` 而非死锁）
- [ ] 确认跨 `await` 的合法并发解析不被误报为循环依赖
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
  `ScopedFromRootProvider` / `ScopeDisposed` / `ProviderDisposed` 等 typed 错误
- `try_get::<T>()` / `try_get_keyed::<T>(key)` 返回 `Option<Arc<T>>`：未注册、
  strict root scope 校验不可解析、已 dispose 或解析失败时均为 `None`（**不 panic**）
- `get_owned::<T>()` 返回 `Result<T, RdiError>`（Singleton 为 `OwnedNotSupported`）

**检查项**：
- [ ] 确认 `get()` 调用方处理 `Err`（不要无脑 `.unwrap()`）
- [ ] 确认可选依赖用 `try_get()` 而非 `get().ok()`（`try_get` 对框架级失败不 panic）
- [ ] 确认 `RdiError::ServiceNotFound` 的 `context` 字段用于定位解析链
- [ ] 确认 dispose 后仍被使用的 provider/scope 会得到 typed 错误（`ProviderDisposed` / `ScopeDisposed`），而不是静默失败

---

### 3.6 key 冲突与多实现

**机制**：
- 非 keyed：同类型多次注册，**最后注册生效**（0.8.0 起，MEDI 语义；0.7.x 为"先注册优先"）
- keyed：同 key 重复注册同样最后注册生效；`#[module]` 内重复 key 仍是编译期错误（`rdi-E004`）
- `get_all::<T>()` **只返回非 keyed** 实现（按注册顺序）；keyed 实现用 `get_all_keyed::<T>()`

**检查项**：
- [ ] 确认非 keyed 服务的注册顺序（**最后注册优先**；需要"未注册才添加"用 `try_add*`）
- [ ] 确认 keyed 服务的 key 唯一性（无拼写错误；同 key 重复注册会静默覆盖）
- [ ] 确认 `get_all::<T>()` 的消费方不假设包含 keyed 实现（广播/观察者场景改用 `get_all_keyed` 或把目标注册为非 keyed）
- [ ] 0.7.x 升级：确认没有依赖"先注册优先"的覆盖逻辑

---

### 3.7 Scope 校验错误

**机制**：0.8.0 默认严格 scope 校验：root provider 解析 Scoped 服务（含经 Transient
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

**机制**：0.8.0 起 `Scope` 既跟踪 scoped 实例也维护 `disposables` 队列。`dispose()` **等待**
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
- [ ] `cargo clippy --all-targets -- -D warnings` 零警告
- [ ] `cargo test --all` 全量通过
- [ ] `cargo doc --no-deps` 零警告

### 6.2 运行时验证
- [ ] 应用启动时无 `RdiError`（循环依赖、captive dependency、服务未找到、`ScopedFromRootProvider`）
- [ ] 应用关闭时显式 `provider.dispose()`，按创建逆序释放所有 Singleton 与 root scoped 实例
- [ ] 长时间运行内存稳定（无 monotonic 增长，排查 async_locks 累积）
- [ ] 关闭后异步任务/worker 不再解析已 dispose 的 provider（期望 `ProviderDisposed`，不应 panic 或静默）

### 6.3 日志验证
- [ ] 工厂 panic 记录到日志（`RdiError::FactoryPanic` 的 `payload` 字段）
- [ ] 循环依赖路径记录到日志（`RdiError::CircularDependency` 的路径信息）
- [ ] captive dependency 记录到日志（`RdiError::SingletonDependsOnScoped` 的 singleton/scoped 名称）

### 6.4 跨 DLL 验证（如适用）
- [ ] DLL 卸载前 `remove_named()` 清理注册项
- [ ] 跨 DLL 传递 `Arc<dyn IProvider>` 而非裸指针
- [ ] DLL 的 `#[module]` 生成代码在卸载后不被调用
