# 与 MEDI 的差异清单

> 适用版本：**rust-dix 0.8.1**（patch 版，公开 API 与 0.8.0 相同）。本文逐条列出 rust-dix
> 与 Microsoft.Extensions.DependencyInjection（下称 MEDI，.NET 的依赖注入抽象）在**可观察行为**上的
> 相同点与差异点，重点标注那些"看起来一样、实际不一样"的语义，供 .NET 迁移、评审与
> 排障时对照。
>
> 说明：下文各节的"rust-dix 0.8.x"指该类语义在 0.8.0 建立、0.8.1 沿用（async singleton 顺序除外，见 §6）。

## 阅读方式

| 列 | 含义 |
|----|------|
| **MEDI 行为** | .NET `ServiceCollection` / `ServiceProvider`（含 ASP.NET Core 托管环境）的默认行为 |
| **rust-dix 0.8.x** | 本框架的默认行为；非默认行为会显式写"需开启 …" |
| **是否对齐** | ✅ 语义等价（API 名称可能不同）／⚠️ 部分对齐（需注意参数/边界）／❌ 不同（Rust 特化） |
| **迁移建议** | 从 .NET 迁来或从 rust-dix 0.7.x 升级时的动作 |

---

## 1. 解析语义（单服务 / 多实现 / keyed）

### 1.1 单服务解析：最后注册优先

| 项 | 内容 |
|----|------|
| **MEDI 行为** | `GetRequiredService<T>()` / `GetService<T>()` 返回**最后注册**的那个实现（`IEnumerable<T>` 与 keyed 是例外，见 1.2）。 |
| **rust-dix 0.8.x** | ✅ 同 MEDI。`get` / `get_optional` / `get_owned` / `get_keyed*` / `get_async` 等**所有单实例路径**都遵循"最后注册优先"；同一 key 的 keyed 重复注册同样是后注册生效。 |
| **是否对齐** | ✅ 对齐（0.7.x 曾按"先注册优先"，0.8.0 起修正）。 |
| **迁移建议** | 0.7.x 中依赖"先注册优先"做覆盖的代码，改用 `try_add*`（未注册才添加）或调整注册顺序。 |

### 1.2 多实现：`get_all` 不含 keyed

| 项 | 内容 |
|----|------|
| **MEDI 行为** | `GetServices<T>()` 只返回**非 keyed** 注册（按注册顺序）；keyed 实现要用 `GetKeyedServices<T>(key)`（按 key 过滤），或 `GetKeyedServices<T>(KeyedService.AnyKey)` 枚举全部 keyed。 |
| **rust-dix 0.8.x** | ✅ 同 MEDI。`get_all::<T>()` 与 `#[inject] Vec<Arc<T>>` 只返回**非 keyed** 注册，按注册顺序。keyed 实现用 `get_all_keyed::<T>()`（返回该类型**全部 keyed** 注册，按注册顺序）。 |
| **是否对齐** | ✅ 语义对齐：`get_all` ↔ `GetServices`，`get_all_keyed` ↔ `GetKeyedServices(AnyKey)`。⚠️ API 形态不同：没有"按 key 枚举"的批量 API，按 key 定位用 `get_keyed(key)`（单实例，最后注册生效）。 |
| **迁移建议** | 0.7.x 中把 keyed 实现也塞进 `get_all` 收集的代码（如观察者广播），改为：非 keyed 注册 → `get_all`；keyed 注册 → `get_all_keyed`；两者都要就分别取并合并。 |

### 1.3 `try_add*` 语义

| 项 | 内容 |
|----|------|
| **MEDI 行为** | `TryAddSingleton` / `TryAddScoped` / `TryAddTransient`：**仅当该服务尚未注册**时添加，已注册则跳过（不覆盖）。 |
| **rust-dix 0.8.x** | ✅ 同 MEDI。`try_add*` 系列仍是"未注册才添加"：**仅当该类型没有非 keyed 注册**时才添加（keyed 注册不阻止 `try_add`），不参与"最后注册覆盖"。 |
| **是否对齐** | ✅ 对齐。 |
| **迁移建议** | 需要"默认实现 + 可被外部覆盖"的组合根写法：先 `try_add` 默认实现，外部再 `add`/`singleton` 覆盖（后注册生效）。 |

---

## 2. Scope 校验（ValidateScopes）

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 裸 `BuildServiceProvider()` 默认 **不做** scope 校验（`ServiceProviderOptions.ValidateScopes = false`），root provider 解析 Scoped 会被缓存到 root；ASP.NET Core 开发环境默认 `ValidateScopes = true`，从 root 解析 Scoped 抛 `InvalidOperationException`。 |
| **rust-dix 0.8.x** | 默认**严格**（等价 `ServiceProviderOptions::default()`，`validate_scopes: true`）：root provider 解析 Scoped 服务（含经 Transient 链间接解析）返回 `Err(RdiError::ScopedFromRootProvider { service })`；`get_optional` / `try_*` 把该服务视为"在 root 不可解析"（`None`），`get_all` / `get_all_keyed` 省略该服务（空/不含）。`provider.scope()` 创建的 Scope 内解析 Scoped 不受影响。**例外**：`get_owned::<T>()` / `try_get_owned`（含 keyed owned）允许从 root 解析 Scoped——owned 实例交给调用方、不进入 scope 缓存、不被容器跟踪；但该服务构造函数内部解析的 Scoped 依赖仍按 root 规则校验（会得到 `ScopedFromRootProvider`）。 |
| **是否对齐** | ⚠️ 默认值相反：rust-dix 对齐的是 **ASP.NET Core 开发环境**的 `ValidateScopes = true`，而裸 MEDI `BuildServiceProvider()` 默认关闭。 |
| **补齐方式** | 需要 0.7.x 的 root-scope 语义（root 直接解析 Scoped 并缓存为根内单例）：`ServiceCollection::new()…build_with(ServiceProviderOptions::lenient())`，即 `validate_scopes: false`。 |
| **迁移建议** | 0.7.x 中从 root 解析 Scoped 的代码：① 改用 `provider.scope()` 并在 scope 内解析（推荐）；② 或显式 `build_with(ServiceProviderOptions::lenient())` 保持旧行为（仅限确实需要 root 单例化的服务）。 |

```rust
use rust_dix::*;

// 默认严格：root 解析 Scoped → Err(ScopedFromRootProvider)
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();
let err = provider.get::<Session>();          // Err(ScopedFromRootProvider { service: "…Session" })
let none = provider.get_optional::<Session>(); // None（不可返回错误的 API 静默视为不可解析）

// 正确姿势：在 scope 内解析
let scope = provider.scope();
let session: Arc<Session> = scope.get().unwrap();

// 例外：owned 解析允许从 root 直接取 Scoped（实例归调用方、不进缓存/不被跟踪）；
// 但该服务构造函数解析到的 Scoped 依赖仍受 root 校验。
let owned_session: Session = provider.get_owned().unwrap();

// 兼容 0.7.x：显式关闭校验（root 解析 Scoped 并缓存为根内单例）
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build_with(ServiceProviderOptions::lenient())
    .unwrap();
let s1: Arc<Session> = provider.get().unwrap(); // OK
let s2: Arc<Session> = provider.get().unwrap(); // 与 s1 同一实例（root 内缓存）
```

> 三个实现细节：
> 1. `get_all` / `get_all_keyed` 返回 `Vec`（签名不可失败），root scope 校验导致的
>    不可解析会被**省略**；但其它解析错误（如工厂 panic）仍以类型化 `RdiError`
>    payload panic 传播（对应 MEDI 中枚举失败抛异常）。
> 2. `is_registered::<T>()` 只报告注册状态：**dispose 之后仍然返回 `true`**，它不表示
>    "此刻可解析"。
> 3. **`Option` 注入不抑制 scope 校验**：`#[inject] Option<Arc<Scoped>>` 在 strict root
>    provider 下仍会以 typed `Err(RdiError::ScopedFromRootProvider)` **构造失败**，而不是
>    变成 `None`；只有容器方法 `get_optional::<Scoped>()` 才返回 `None`。可选 Scoped
>    依赖必须在 `provider.scope()` 内构造/解析。

---

## 3. 资源释放（Dispose）

| 项 | 内容 |
|----|------|
| **MEDI 行为** | `IServiceScope` 即 `IDisposable`；`scope.Dispose()` 按创建逆序释放 `IDisposable` 的 Scoped/Transient 实例。root `ServiceProvider` 实现 `IDisposable`，`Dispose()` 释放 singleton 与 root scoped，之后解析抛 `ObjectDisposedException`。 |
| **rust-dix 0.8.x** | 对齐但显式化：① `Scope` 自动跟踪该 scope 创建的 Scoped 实例，`Scope::dispose()`/`Drop` 按**创建逆序**释放实例，随后按**注册逆序**执行 `register_disposable` 清理闭包；dispose 后解析返回 `Err(ScopeDisposed)`（不可返回错误的 API 为 `None`/空）。② `ServiceProvider::dispose()` 按创建逆序释放全部 singleton 与 root scoped 实例、逆序执行清理、清空 named 注册，并标记 provider 已释放，之后解析返回 `Err(ProviderDisposed)`。两者均幂等。 |
| **是否对齐** | ⚠️ 部分对齐：`Scope` 的 Drop 自动 dispose 等价 MEDI 的 using/dispose 习惯；但 rust-dix 默认**不**通过 Drop 释放 root provider 的 singleton（需显式 `provider.dispose()`），而 MEDI 中 `ServiceProvider.Dispose()` 也是显式调用——这一点实际一致。差异在于：Rust 侧 `Arc<ServiceProvider>` 常被多处持有，"最后一个 Arc 释放"不保证调用 dispose，因此**显式 dispose** 是契约要求。 |
| **迁移建议** | 应用关闭路径显式 `provider.dispose()`；请求级 scope 用显式 `scope.dispose()` 或让其 Drop。`get_owned` 取得的实例归调用方所有，**不进入**容器跟踪/释放流程（Rust 特有，见 §9）。 |

```rust
let scope = provider.scope();
let a: Arc<ScopedA> = scope.get().unwrap();
let b: Arc<ScopedB> = scope.get().unwrap();
scope.dispose();               // 释放顺序：B 先、A 后（创建逆序），随后逆序执行清理闭包
let _ = scope.get::<ScopedA>(); // Err(ScopeDisposed)

provider.dispose();            // 释放 singleton + root scoped，清空 named 注册
let _ = provider.get::<AppConfig>(); // Err(ProviderDisposed)
```

> 边界细节：
> - `provider.dispose()` 会把**整棵 provider 树**标记为不可用：此前创建的 `Scope`
>   之后解析返回 `Err(ProviderDisposed)`（而 scope 自己 `dispose()` 后返回
>   `Err(ScopeDisposed)`）。
> - 实例的释放方式是"丢弃容器的强引用"：若业务代码仍持有某实例的 `Arc`，
>   该服务的 `Drop` 会在最后一个 `Arc` 释放时运行。
> - `get_owned` 取得的实例归调用方，**不进入** dispose 跟踪；
>   `is_registered` 在 dispose 后仍报告注册状态（`true`）。
> - **Transient 不在容器跟踪范围内**：MEDI 会在 scope 结束时 Dispose scope 内创建的
>   `IDisposable` **Transient** 实例；rust-dix 只跟踪/按创建逆序释放 **Scoped** 实例，
>   Transient 归调用方（随最后一个 `Arc` 释放）。需要确定性清理时用
>   `register_disposable`，或在 scope 内自行管理 Transient 生命周期。
> - **dispose 之后注册的清理闭包不会执行**：`register_disposable` 仍接受闭包，但队列
>   只在 dispose 时 drain 一次（dispose 幂等且之后 provider 已标记释放），因此请在
>   dispose 之前注册。

> **确定性构建顺序**：build 期**同步 singleton 按注册顺序创建**（0.8.0 起）、
> **async singleton 按依赖优先拓扑序递归初始化**（0.8.1 起），两者均确定、不依赖
> `HashMap` 迭代顺序；释放按**创建逆序**进行，即"**后建的先释放、先建的后释放**"，
> 构建与释放顺序均可复现。

---

## 4. `IServiceScopeFactory` 与 ScopeFactory 的弱引用契约

| 项 | 内容 |
|----|------|
| **MEDI 行为** | `IServiceScopeFactory` 由容器提供，强引用容器；singleton 注入它不会造成泄漏（容器与 factory 同生命周期，GC 回收环）。 |
| **rust-dix 0.8.x** | 内置 `dyn ScopeFactory` 无需注册即可解析（显式注册优先；本身是缓存单例），**持 `Weak<ServiceProvider>`**：singleton 保存它不会形成 `容器 → singleton → 容器` 的 `Arc` 环；代价是**容器必须先于其创建的服务存活**——provider `dispose()` 后 `create_scope()` 仍能返回 scope，但 scope 解析返回 `Err(ProviderDisposed)`；provider 的 `Arc` 全部释放后弱引用无法升级，`create_scope()` 会 panic。 |
| **是否对齐** | ❌ 不同（Rust 无 GC，用弱引用打破 `Arc` 环）。 |
| **迁移建议** | 后台 worker 等 Singleton 优先注入 `Arc<dyn ScopeFactory>`；不要在 Singleton 里保存 `#[inject(provider)] Arc<ServiceProvider>`（强引用环）。容器关闭前先停止 worker。 |

---

## 5. 构造期解析（在构造函数里解析服务）

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 构造函数内允许任意 `GetRequiredService`（可能绕过循环/captive 校验，结果依赖注册顺序）。 |
| **rust-dix 0.8.x** | **拒绝**：在 build 期构造函数内通过注入的 provider 做动态解析，返回类型化 `RdiError::FactoryPanic`（不会被悄悄构建）。请保存句柄（provider / `ScopeFactory`），在方法中延迟解析。 |
| **是否对齐** | ❌ 不同（更严格，换来可校验、可复现的构建）。 |
| **迁移建议** | 构造函数里只保存依赖句柄；把"运行时解析"移到方法中。 |

---

## 6. 异步 DI（Rust 扩展）

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 无异步 DI：`IServiceProvider.GetService` 全同步；异步初始化需自己在组合根完成。 |
| **rust-dix 0.8.x** | 提供扩展：`async_singleton` / `async_scoped` / `async_transient`（及 keyed 变体）+ `build_async()` / `build_async_with(options)` + `get_async` / `get_keyed_async`。含任何 `async_*` 注册时 `build()` 返回 `Err(AsyncServiceRequiresBuildAsync)`。 |
| **是否对齐** | ❌ Rust 扩展（MEDI 无对应能力）。 |
| **迁移建议** | .NET 中在 `Program.cs` 里 `await` 完成的一次性初始化，可映射为 `async_singleton`。 |

> **async 解析链的循环检测**：`async_scoped` / `async_transient` 的自引用/交叉递归用
> "**单次 poll 链路**"标记检测并返回 `RdiError::CircularDependency`（避免在自有的 per-key
> async 缓存锁上死锁）；跨 `await` 的合法并发解析不会被误报。同步 Scoped/Transient 仍用
> 线程局部解析栈检测。

> **async singleton 注册顺序无关（依赖驱动递归初始化，0.8.1+）**：`build_async` 初始化
> async singleton 时是**依赖驱动**的——工厂内 `await` 任意 async singleton（无论注册先后）
> 都会先递归初始化被依赖项。因此创建顺序是确定性的**依赖优先拓扑序**，而不是注册顺序；
> 递归初始化是单飞（single-flight）并按单次 poll 链路检测环：自引用/交叉依赖返回
> `RdiError::CircularDependency`，**不会 hang**。`build_async` 返回时所有 async singleton
> 仍已初始化（保留 eager / fail-fast 语义），且与同步 singleton"注册顺序无关"的语义一致，
> 也与 MEDI"注册与解析分离"的模型对齐。
>
> 0.8.0 曾要求 async singleton 的依赖先注册（否则返回带 `registration order` 提示的
> `FactoryPanic`）；该限制已在 **0.8.1 修复**，无 API 变更、无迁移动作。
>
> 逆序注册同样可以构建（依赖优先拓扑序）：
>
> ```rust
> let provider = ServiceCollection::new()
>     // 先注册依赖方（Repo）—— 它 await 的 DbPool 在下面才注册
>     .async_singleton(|sp| Box::pin(async move {
>         let db: Arc<DbPool> = sp.get_async().await.expect("DbPool");
>         Arc::new(Repo { db })
>     }))
>     // 后注册被依赖方
>     .async_singleton(|_| Box::pin(async { Arc::new(DbPool::connect().await) }))
>     .build_async()
>     .await
>     .unwrap(); // 0.8.1 起可成功：初始化按依赖驱动
> ```

> **build 期只能通过 `get_async` 解析 async singleton**：async singleton 初始化完成前，
> 同步 `get` 返回 `Err(RdiError::AsyncServiceRequiresBuildAsync)`、`get_optional` 为
> `None`，但 `is_registered` 仍为 `true`。async 工厂内请用 `sp.get_async::<T>()`（会递归
> 初始化依赖）；`build_async` 返回后已缓存的 async singleton 可直接 `get`。

> **同步 singleton 不能依赖 async singleton**：同步 singleton 的校验/构建（Phase 2/3）先于
> async singleton 初始化（Phase 4），因此其依赖图中出现 async singleton 时，`build_async`
> 返回 typed `RdiError::AsyncServiceRequiresBuildAsync { service }`（不 hang）。需要时把依赖方
> 也改为 async singleton，或在方法内用 `Arc<dyn ScopeFactory>` / `get_async` 延迟解析。

> **环的错误形态**：嵌套 `get_async` 得到 `RdiError::CircularDependency(_)`；若用户 async 工厂
> 对该错误 `.unwrap()` / `.expect()`，顶层 `build_async` 会得到 `RdiError::FactoryPanic { payload }`
> （payload 含 `"CircularDependency(...)"`）。判定：`get_async` 侧
> `matches!(err, RdiError::CircularDependency(_))`；`build_async` 顶层
> `matches!(err, RdiError::FactoryPanic { ref payload, .. } if payload.contains("CircularDependency"))`。

---

## 7. Keyed 服务的 key 类型与 `AnyKey`

| 项 | 内容 |
|----|------|
| **MEDI 行为** | keyed 服务的 key 是 `object?`：任意类型（string / enum / 自定义对象）；另有 `KeyedService.AnyKey` 用于"匹配任意 key"的注册。 |
| **rust-dix 0.8.x** | key 只支持 **`&str` / `String`**（注册与解析都是字符串）；**没有注册侧的 `AnyKey`**（无法注册"匹配任意 key"的实现）。枚举侧 `get_all_keyed` 对应 MEDI `GetKeyedServices(AnyKey)`。 |
| **是否对齐** | ⚠️ 部分对齐（key 类型收窄、无 AnyKey）。 |
| **迁移建议** | 用语义明确的字符串常量作为 key；需要"兜底任意 key"时注册一个非 keyed 默认实现并在代码里回退（`try_get_keyed(k).unwrap_or_else(|| get())`）。 |

---

## 8. `get_all` 与 MEDI `IEnumerable<T>`

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 构造函数可注入 `IEnumerable<T>`；解析得到**非 keyed** 注册的集合（按注册顺序），空集合合法。集合在解析时即被物化为数组。 |
| **rust-dix 0.8.x** | `#[inject] Vec<Arc<T>>` / `get_all::<T>()` 语义对齐：返回**非 keyed** 注册、按注册顺序、空 `Vec` 合法。差异只在表示形态：返回 `Vec<Arc<T>>`（共享所有权的 `Arc` 元素），而非 .NET 的对象引用数组；keyed 实现不在其中。 |
| **是否对齐** | ✅ 语义对齐；⚠️ 返回类型不同（Rust 无引用 GC，用 `Arc` 表达共享所有权）。 |
| **迁移建议** | 需要 keyed 实现时用 `get_all_keyed::<T>()`；`Vec` 的元素是 `Arc<T>`，如需长期持有请 clone `Arc`（浅拷贝），不要期望 `Vec` 本身是惰性视图。 |

---

## 9. `get_owned`（Rust 扩展）

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 无"拥有型解析"概念；Scoped/Transient 也是共享引用（对象引用），可变访问由对象自身负责。 |
| **rust-dix 0.8.x** | Rust 扩展：`get_owned::<T>()` 返回 owned `T`（非 `Arc<T>`），用于 `&mut self` 服务；Singleton 不可 owned（`Err`/`None`）；Scoped 的 owned 解析**绕过缓存**每次新建。**owned 解析是严格 scope 校验的例外**：从 root 对 Scoped 调 `get_owned`/`try_get_owned` 允许（实例归调用方、不进缓存/不跟踪），但其构造函数内部解析的 Scoped 依赖仍受 §2 校验。 |
| **是否对齐** | ❌ Rust 扩展。 |
| **迁移建议** | 需要 `&mut self` 的服务注册为 Transient/Scoped；`get_owned` 实例归调用方所有，不参与容器 dispose。需要共享时用 `get`（Scoped 需在 scope 内）。 |

---

## 10. 默认生命周期与注册 API 风格

| 项 | 内容 |
|----|------|
| **MEDI 行为** | 没有"默认生命周期"：`AddSingleton` / `AddScoped` / `AddTransient` 必须显式选择。 |
| **rust-dix 0.8.x** | `#[inject]` / `#[rust_dix::inject]` 省略生命周期参数时**默认 Singleton**；`#[derive(Inject)]` 只生成构造函数、不注册；keyed 实现必须手动注册（或 `#[module]`）。 |
| **是否对齐** | ⚠️ 不同（rust-dix 有默认值）。 |
| **迁移建议** | 面向接口/请求级服务显式写 `#[inject(scoped)]` / `#[inject(transient)]`，不要依赖默认 Singleton；需要多实现路由的服务用 keyed 手动注册。 |

---

## 11. API 对照补充表

| MEDI (.NET) | rust-dix 0.8.x | 备注 |
|-------------|----------------|------|
| `IServiceCollection` | `ServiceCollection` | 注册端 |
| `IServiceProvider` | `ServiceProvider` | 解析端 |
| `IServiceScope` | `Scope` / `ServiceScope` | 子作用域 |
| `IServiceScopeFactory` | `dyn ScopeFactory` | 内置弱引用实现，无需注册 |
| `CreateScope()` | `provider.scope()` | 等价（返回 `Scope`） |
| `GetRequiredService<T>()` | `get::<T>()` | `Result` 而非抛异常；**最后注册** |
| `GetService<T>()` | `get_optional::<T>()` | 可选解析 |
| `GetServices<T>()` | `get_all::<T>()` | 仅**非 keyed**、按注册顺序 |
| `GetKeyedService<T>(key)` | `get_keyed::<T>(key)` | key 为字符串 |
| `GetKeyedServices<T>(key)` | `get_keyed::<T>(key)`（单实例） | 无"按 key 枚举"的批量 API，需自行多次调用 |
| `GetKeyedServices<T>(KeyedService.AnyKey)` | `get_all_keyed::<T>()` | 枚举该类型全部 keyed 注册（按注册顺序） |
| `AddSingleton<T>()` / `AddScoped<T>()` / `AddTransient<T>()` | `.singleton(f)` / `.scoped(f)` / `.transient(f)` | 工厂闭包式 |
| `AddKeyedSingleton<T>(key)` 等 | `.keyed_singleton::<T>(key, f)` 等 | 三种生命周期齐全 |
| `TryAddSingleton` / `TryAddScoped` / `TryAddTransient` | `try_add_singleton` / `try_add_scoped` / `try_add_transient`（`try_add` 为 singleton） | 未注册才添加 |
| `ServiceProviderOptions.ValidateScopes` | `ServiceProviderOptions::validate_scopes` | 默认 `true`（见 §2） |
| `BuildServiceProvider(options)` | `build_with(options)` / `build_async_with(options)` | `build()` == `build_with(Default)` |
| `IServiceProviderIsService` | `is_registered::<T>()` / `is_registered_keyed::<T>(key)` | 仅查询，不创建实例 |
| `ServiceProvider.Dispose()` | `ServiceProvider::dispose()` | 幂等；之后解析 `Err(ProviderDisposed)` |
| `IServiceScope.Dispose()` | `Scope::dispose()`（Drop 自动） | 逆序释放；之后解析 `Err(ScopeDisposed)` |
| `ObjectDisposedException` | `RdiError::ProviderDisposed` / `RdiError::ScopeDisposed` | 类型化错误 |
| `InvalidOperationException`（未注册） | `RdiError::ServiceNotFound` / `KeyedServiceNotFound` | 类型化错误 |

---

## 12. 0.7.x → 0.8.x 迁移速查

| 旧行为（0.7.x） | 新行为（0.8.x） | 动作 |
|-----------------|-----------------|------|
| 单服务解析"先注册优先" | 最后注册优先（MEDI 语义） | 依赖首注册的代码改用 `try_add*` 或调整顺序 |
| `get_all` 含 keyed | `get_all` 仅非 keyed | keyed 实现改用 `get_all_keyed` |
| root 直接解析 Scoped 并缓存 | 默认 `Err(ScopedFromRootProvider)` | 改用 `provider.scope()`；确需旧行为用 `ServiceProviderOptions::lenient()` |
| `dispose` 仅覆盖显式注册的清理闭包 | Scope 自动逆序释放 Scoped 实例 + 逆序清理；新增 provider dispose | 应用关闭时显式 `provider.dispose()` |
| 0.7.x singleton 初始化顺序不确定（HashMap） | 同步 singleton 按注册顺序；async singleton 依赖优先拓扑序（0.8.1+），两者均确定 | 依赖构建顺序的初始化逻辑可写文档化断言 |
| `RdiError` 可被穷举匹配 | `#[non_exhaustive]` | 匹配处补 `_ =>` 分支 |
| async singleton 要求依赖先注册（0.8.0 限制） | 0.8.1 起**注册顺序无关**、依赖驱动递归初始化；有环返回 `CircularDependency` | 无需迁移；原先合法的失败配置现在可构建 |
| 从 root `get_owned` Scoped | **行为不变**：仍允许（owned 不缓存、不跟踪）；失败的是其 Scoped 依赖 | 无需迁移；依赖仍受严格校验，需要依赖时在 scope 内取 |

---

## 下一步

- [第一章目录](INDEX.md)（核心概念 / 适用场景小节规划中）
- [快速上手 · 第二章目录](../02-quickstart/INDEX.md)
- [用法指南与决策矩阵](../08-best-practices/usage-guide.md)
