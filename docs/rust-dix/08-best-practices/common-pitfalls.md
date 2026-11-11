# 常见陷阱与排查

本文档列举 rust-dix 开发中的高频错误及其排查方法。

> 本文示例采用简短路径风格，假定文件顶部已 `use rust_dix::*;`。导入约定详见 [用法指南](usage-guide.md#1-导入约定路径简化)。

## 陷阱 1：TypeId 不匹配

### 现象

```
Err(RdiError::ServiceNotFound { ty: "dyn ILogger", context: None })
```

或 inject 构造时：

```
Err(RdiError::ServiceNotFound { ty: "...Logger", context: Some("MyHandler::log") })
```

### 原因

注册时的类型与解析时的类型不一致：

```rust
// 注册为具体类型
.singleton(|_| Arc::new(FileLogger))

// 解析为 trait
let logger: Arc<dyn ILogger> = provider.get()?;  // Err(ServiceNotFound) — TypeId 不匹配
```

### 排查

检查注册代码是否使用泛型参数 `::<dyn Trait>`：

```rust
// ✅ 修正
.singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
```

## 陷阱 2：keyed 服务未找到

### 现象

```
Err(RdiError::KeyedServiceNotFound { key: "wechat", ty: "dyn IPayGateway", context: … })
```

### 原因

- key 名称拼写错误
- keyed 服务未注册
- 注册为其他 key

### 排查

```rust
// 检查 key 是否正确
.keyed_singleton::<dyn IPayGateway>("wechat", ...)  // 注册 key = "wechat"
provider.get_keyed::<dyn IPayGateway>("wechat")?;  // 解析 key = "wechat"

// 安全解析
if let Some(gateway) = provider.try_get_keyed::<dyn IPayGateway>("wechat") {
    // ...
}
```

## 陷阱 3：生命周期依赖异常

### 现象

`build()` 返回 `RdiError::SingletonDependsOnScoped` 错误，容器构建失败。

### 原因

Singleton 服务依赖了 Scoped 服务（captive dependency 反模式）。
0.4.0 起，build 阶段会检测此反模式并拒绝构建：

```rust
#[inject(singleton)]
struct GlobalService {
    session: Arc<Session>,  // Session 注册为 Scoped → build 报错
}

// build() 返回 Err(RdiError::SingletonDependsOnScoped { .. })
```

Singleton 会永久持有 Scoped 实例，导致后续 scope 无法获得新的 Scoped 实例。
间接依赖链（`Singleton → Transient → Scoped`）同样会被检测拒绝。

### 排查

检查 Singleton 的字段依赖，确保只依赖 Singleton：

```rust
// ✅ Singleton 只依赖 Singleton
#[inject(singleton)]
struct GlobalService {
    config: Arc<AppConfig>,  // Singleton
}
```

## 陷阱 4：循环依赖

### 现象

程序运行时栈溢出或死循环。

### 原因

服务 A 依赖 B，B 依赖 A：

```rust
#[derive(Inject)]
struct ServiceA {
    #[inject]
    b: Arc<ServiceB>,
}

#[derive(Inject)]
struct ServiceB {
    #[inject]
    a: Arc<ServiceA>,
}

ServiceCollection::new()
    .singleton(|r| __rdi_construct_ServiceA(r))
    .singleton(|r| __rdi_construct_ServiceB(r))
    .build()?; // Err(RdiError::CircularDependency)
```

### 排查

0.3+ 在 `build()` 阶段就会检测循环依赖并返回 `RdiError::CircularDependency`
（运行期 Scoped/Transient 解析也有线程局部解析栈兜底；通过注入 provider 发起的
动态解析同样参与该栈检测）。仍建议从设计上消除循环；确需延迟解析时：

1. **重构依赖方向**：消除循环
2. **延迟解析**：用 `#[inject(provider)]` 按需解析（eager singleton 与懒解析服务都支持）

```rust
#[inject(singleton)]
struct ServiceA {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,  // 按需解析
}

impl ServiceA {
    fn use_b(&self) {
        let b: Arc<dyn IServiceB> = self.provider.get().unwrap();  // 延迟解析
    }
}
```

> 注意：在 build 期构造函数内通过注入的 provider 发起的解析会被拒绝（返回类型化
> `FactoryPanic`），目的是避免绕过循环/囚徒依赖校验；把解析移到方法中即可。

## 陷阱 5：根 Provider 解析 Scoped（默认严格 scope 校验）

### 现象

0.8.0 起（0.10.x 同样适用）从根 Provider 解析 Scoped 服务直接报错，而不是像旧版本那样在根内缓存复用：

```
Err(RdiError::ScopedFromRootProvider { service: "…Session" })
```

或期望"根级 Scoped 跨请求隔离"，但服务根本解析不出来。

### 原因

0.8.0 起默认开启严格 scope 校验（0.10.x 同样适用）（`ServiceProviderOptions::default()`，
`validate_scopes: true`，对齐 ASP.NET Core 开发环境的 `ValidateScopes`）。root
provider 解析 Scoped 服务（含经 Transient 链间接解析）返回
`Err(ScopedFromRootProvider)`；不可返回错误的 API（`get_optional` / `get_all` /
`get_all_keyed` / `try_*`）把该服务视为不可解析（`None` / 空）。

> **例外**：`get_owned` / `try_get_owned`（含 keyed owned）允许从 root 解析 Scoped ——
> owned 实例归调用方、不进缓存/不被跟踪。但若该服务的构造函数内部还解析 Scoped 依赖，
> 这些依赖在 root 上仍会报 `ScopedFromRootProvider`（详见陷阱 9）。

> **注意 `Option` 注入 ≠ `get_optional`**：`#[inject] Option<Arc<Scoped>>` 在 strict root
> 下**不会**变成 `None`，而是构造失败（typed `Err(ScopedFromRootProvider)`）；
> 只有容器方法 `provider.get_optional::<Scoped>()` 才返回 `None`。可选 Scoped 依赖
> 需在 `provider.scope()` 内构造/解析。

0.7.x 的"根 Provider 即 root scope、Scoped 缓存在 `root_scoped_cache` 复用"行为，
现在只能通过 `ServiceProviderOptions::lenient()` 显式开启：

```rust
// 0.7.x 旧行为（显式开启）：root 解析 Scoped 并缓存为根内单例
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build_with(ServiceProviderOptions::lenient())
    .unwrap();

let s1: Arc<Session> = provider.get().unwrap(); // OK
let s2: Arc<Session> = provider.get().unwrap(); // s1 == s2（根 scope 内缓存）
```

### 排查

需要请求级隔离时，在子 `Scope` 内解析：

```rust
let scope = provider.scope();
let s1: Arc<Session> = scope.get().unwrap();
let s2: Arc<Session> = scope.get().unwrap();
// s1 == s2（子 Scope 内缓存）

let scope2 = provider.scope();
let s3: Arc<Session> = scope2.get().unwrap();
// s3 != s1（跨子 Scope 独立）
```

> 推荐始终用 `provider.scope()` 处理请求级服务；只有在确实需要"根级 Scoped 单例化"
> 时才用 `lenient()`。详见 [与 MEDI 的差异清单](../01-introduction/medi-differences.md#2-scope-校验validatescopes)。

## 陷阱 6：过程宏编译失败

### 现象

```
error: named struct or unit struct required
error: only structs are supported
```

### 原因

- 使用元组结构体（不支持）
- 使用 enum（不支持）

### 排查

```rust
// ❌ 元组结构体
#[inject(singleton)]
struct Service(i32, String);  // 不支持

// ✅ 命名结构体
#[inject(singleton)]
struct Service {
    value: i32,
    name: String,
}

// ✅ 单元结构体
#[inject(singleton)]
struct Marker;
```

## 陷阱 7：重复 keyed key

### 现象

```
error: rdi-E004: duplicate key `wechat`
```

### 原因

`#[module]` 中重复定义同名 keyed 服务：

```rust
#[module]
mod services {
    register!(keyed "wechat": singleton: dyn IPayGateway => WechatPay);
    register!(keyed "wechat": singleton: dyn IPayGateway => Alipay);  // 重复
}
```

### 排查

> **0.10 起**：`rdi-E004` 仅对**字符串字面量** key 报编译错误；动态表达式 key 的重复按
> 运行时"最后注册优先"（last-wins），不会编译失败。上例的
> `register!(keyed "wechat": singleton: dyn IPayGateway => WechatPay)` 形式自 0.10 起会真正
> 注册 `WechatPay`（此前 `=> Impl` 会被静默丢弃）。

确保每个 key 在 module 内唯一：

```rust
#[module]
mod services {
    register!(keyed "wechat": singleton: dyn IPayGateway => WechatPay);
    register!(keyed "alipay": singleton: dyn IPayGateway => Alipay);  // 不同 key
}
```

## 陷阱 8：字段类型未注册

### 现象

```
Err(RdiError::ServiceNotFound { ty: "…IUserService", context: Some("Handler::user_svc") })
```

### 原因

过程宏生成的构造函数解析**标记了 `#[inject]` 的**字段时，字段类型未注册：

```rust
#[inject(singleton)]
struct Handler {
    #[inject]
    user_svc: Arc<dyn IUserService>,  // IUserService 未注册
}
```

> 注：未标记字段走 `Default::default()`，不会触发此错误。
> 若字段本应注入却忘了加 `#[inject]`，会静默使用 `Default`——务必显式标记依赖字段。

### 排查

确保所有依赖字段都已注册，且显式标 `#[inject]`：

```rust
// ✅ 先注册依赖
#[inject(singleton)]
struct UserService;

#[inject]
impl IUserService for UserService { /* ... */ }

#[inject(singleton)]
struct Handler {
    #[inject]
    user_svc: Arc<dyn IUserService>,  // OK
}
```

## 陷阱 9：Owned 服务解析误用

### 现象

```
Err(RdiError::…) — get_owned: not registered or Singleton: DbContext
```

或：期望 Scoped 的 `get_owned` 复用缓存，但每次都拿到新实例。

### 原因

`get_owned::<T>()` 返回 owned `T`（非 `Arc<T>`），语义与 `get` 不同：

1. **Singleton 不可 owned**：Singleton 全局共享，refcount 恒 >1，无法 `try_unwrap`。
   `get_owned` 对 Singleton 返回 `Err`（`try_get_owned` 返回 `None`）。
2. **Scoped owned 绕过缓存**：`get_owned` 对 Scoped 绕过 `scoped_cache`，每次调用创建新实例。
   这是有意为之——owned `&mut self` 与 scope 共享语义不兼容。**owned 解析是严格 scope
   校验的例外**：root provider 上对 Scoped 调 `get_owned`/`try_get_owned` 是允许的
   （实例归调用方、不进缓存/不被跟踪）；但若该服务的构造函数内部还解析 Scoped 依赖，
   这些依赖在 root 上仍会返回 `Err(ScopedFromRootProvider)`，此时需在 `scope` 内取。
3. **宏 owned 字段未注册或为 Singleton**：`#[derive(Inject)]` 的 `#[inject(owned)]` 裸 `T` 字段
   走 owned 路径，若该类型未注册或注册为 Singleton，构造失败返回 `Err`（含字段 `context`）。

```rust
// ❌ DbContext 注册为 Singleton，无法 owned
.singleton(|_| Arc::new(DbContext::new()))
// ...
let ctx: DbContext = provider.get_owned()?;  // Err

// ❌ 期望 Scoped owned 缓存复用（实际每次新建）
.scoped(|_| Arc::new(DbContext::new()))
let a = provider.get_owned::<DbContext>();   // root 上允许（owned 不缓存）
let b = provider.get_owned::<DbContext>();
// a、b 是不同实例（owned 绕过缓存）

// 若 DbContext 还依赖其他 Scoped 服务，则需要 scope：
let scope = provider.scope();
let c = scope.get_owned::<DbContext>();
```

### 排查

- 需要 `&mut self` 的服务（如 DbContext）应注册为 **Transient 或 Scoped**，不要 Singleton。
- 若需共享 + 可变，用 `Arc<RwLock<T>>`（interior mutability）而非 `get_owned`。
- `#[derive(Inject)]` `#[inject(owned)]` 裸 `T` 字段：确保对应类型已注册为 Transient/Scoped。

```rust
// ✅ DbContext 注册为 Transient，可 owned
.transient(|_| Arc::new(DbContext::new()))
let mut ctx: DbContext = provider.get_owned();
ctx.save_changes();  // ✓ &mut self
```

## 陷阱 10：struct 与 impl 同时用 `#[inject]`（双重注册）

### 现象

容器里同一 struct 被注册了两次（既作为具体类型，又作为 `dyn Trait`），消费者能绕过 trait 接口直接 `get::<具体类型>()`，破坏面向接口原则与可 mock 性：

```rust
// 既能拿到 trait
let h: Arc<dyn IUserHandler> = provider.get();
// 又能绕过 trait 直接拿到具体类型 —— 通常非预期
let h2: Arc<UserHandler> = provider.get();
```

### 原因

`#[inject]` 放在 struct 上 = 注册为**具体类型**（向 inventory 提交一次）；放在 `impl Trait for T` 上 = 注册为 **`dyn Trait`**（再提交一次）。两者同时用会让同一 struct 被注册两次。

关键混淆点：`#[derive(Inject)]` 与 `#[inject]` 都能在 struct 侧生成构造函数，但**只有 `#[inject]` 会提交注册**。面向接口时若误把 struct 上的 `#[derive(Inject)]` 写成 `#[inject]`，就会触发双重注册。

```rust
// ❌ 双重注册：struct 和 impl 都用 #[inject]
#[inject(transient)]
struct UserHandler {
    #[inject]
    user_svc: Arc<dyn IUserService>,
}

#[inject]
impl IUserHandler for UserHandler { /* ... */ }
// 结果：容器里既有 UserHandler（具体类型）又有 dyn IUserHandler
```

### 排查

**二选一原则**：`#[inject]` 一般只放一处。

| 目标身份 | struct 上 | impl 上 | 结果 |
|---------|----------|---------|------|
| 仅具体类型（无 trait 契约，如配置/值对象） | `#[inject]` | *不放* | ✅ |
| 仅 `dyn Trait`（面向接口，**Handler/Service/Repository 推荐**） | `#[derive(Inject)]` | `#[inject]` | ✅ |
| 两者都要（罕见，确需同时暴露） | `#[inject]` | `#[inject]` | ⚠️ 需注释说明意图 |
| 仅 impl、struct 无构造函数 | *不放* | `#[inject]` | ❌ 编译失败（无 `__rdi_construct`） |

```rust
// ✅ Handler 面向接口：struct 用 #[derive(Inject)] 仅生成构造函数（不注册），impl 用 #[inject] 注册为 trait
#[derive(Inject)]
struct UserHandler {
    #[inject]
    user_svc: Arc<dyn IUserService>,
}

#[inject]  // 仅注册为 dyn IUserHandler
impl IUserHandler for UserHandler {
    fn handle(&self, req: Req) -> Resp { /* ... */ }
}
```

**何时放 struct 上**：服务作为具体类型对外暴露——无 trait，或消费者直接用具体类型（配置、值对象）。
**何时放 impl 上**：服务有 trait 契约，面向接口便于 mock 替换——Handler / Service / Repository 均属此类。**Handler 推荐放 impl 上**，struct 上用 `#[derive(Inject)]` 而非 `#[inject]`。

> 详见 [用法指南 · 放置位置决策](usage-guide.md#放置位置struct-还是-impl二选一避免双重注册)。

> **为何宏无法编译时检测？** 过程宏跨调用无状态、struct 与 impl 的 TypeId 不冲突、且双重注册是被支持的特性（见 `inject_combined_concrete_and_trait` 测试）。防范完全依赖门禁 A9/B6。详见 [反模式 10 · 为什么宏无法编译时检测双重注册](anti-patterns.md#为什么宏无法编译时检测双重注册)。

## 陷阱 11：singleton 持有 `Arc<ServiceProvider>` 形成引用环

### 现象

外部 `drop(provider)` 后容器的 singleton 缓存与单例仍互相持有，内存不释放；
反复 `build()` 的测试/长跑进程内存持续增长。

### 原因

provider 的 singleton 缓存持有 singleton，而 singleton 的
`#[inject(provider)]` 字段又持有 `Arc<ServiceProvider>`，形成
`容器 → singleton → 容器` 的 `Arc` 环。0.7.1 起内置 `ScopeFactory` 使用
`Weak` 引用，专门用于打破这个环。

### 排查

- singleton 需要按任务创建 scope 时，优先 `#[inject] scope_factory: Arc<dyn ScopeFactory>`
  （内置实现持 `Weak`，不形成环）。
- 确实需要容器自身时可用 `#[inject(provider)]`，但应只在应用级、与进程同生命周期的
  容器上使用；不要在短生命周期容器上这样做。
- 不要让 singleton 保存 `Scope` 或 scoped 实例（作用域缓存会持住它们）。

```rust
// ✅ 推荐：弱引用 scope 工厂，无 Arc 环
#[derive(Inject)]
struct OutboxWorker {
    #[inject]
    scope_factory: Arc<dyn ScopeFactory>,
}
```

## 陷阱 12：AnyKey 兜底的常见误解

### 现象

- `get_all_keyed()` 列不出通过 `keyed_any_*` 注册的兜底实现；
- 用 `ServiceKey::any_key()` 解析单个服务得到 `Err`；
- 以为 AnyKey 兜底在多个请求 key 之间共享同一个实例。

### 原因

AnyKey 兜底（对齐 MEDI `KeyedService.AnyKey`）要点（完整语义：
[语义权威页（0.10）](../09-reference/semantics.md#2-anykey-兜底) 与
[扩展契约](../09-reference/extension-points.md)：

1. `get_all_keyed` 只列显式 key，不含兜底；`any_key()` 不能做单值解析
   （`is_registered_keyed(any_key()) == false`）。
2. singleton/scoped 实例**按 (注册/类型, 请求 key) 缓存**（同注册同 key 复用，不同 key 或不同
   AnyKey 注册独立；scoped 每 scope 每份）；validation 期消费的 singleton 预填充缓存，运行时
   同注册同 key 复用同一实例（单次构造）。
3. owned 例外：scoped/transient 的 `get_keyed_owned`/`try_get_keyed_owned` 在 strict root
   允许（不进缓存，传递依赖仍校验）；工厂内嵌套**不同**请求 key 合法。
4. 仅同步注册、不 eager 初始化；`keyed_any_scoped` 被同步 singleton 捕获仍 captive；工厂
   `&ServiceKey` 仅 `Debug`/`Display`/`Eq`/`Hash`，无 typed downcast 访问器。

### 排查

```rust
// ✅ 正确：精确 key + AnyKey 兜底
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IStorage>("admin", |_| Arc::new(AdminStore))
    .keyed_any_singleton::<dyn IStorage>(|_, key| Arc::new(Store::for_key(key)))
    .build()
    .unwrap();

let admin: Arc<dyn IStorage> = provider.get_keyed("admin").unwrap(); // 精确
let other: Arc<dyn IStorage> = provider.get_keyed("tenant-1").unwrap(); // 兜底
let all = provider.get_all_keyed::<dyn IStorage>(); // 只含 "admin"
```

## 陷阱 13：跨任务 async 环死锁（未传 `AsyncFactoryContext`）

### 现象

async 工厂里 `tokio::spawn` 一个任务去解析其它服务时，`build_async` 可能 hang（而不是返回
`CircularDependency`）。

### 原因

async 环检测是**解析链**式的：链在 future 构造时捕获。完全脱离的任务没有继承链，跨任务
自引用/交叉依赖可能重新在 per-key async 缓存锁上死锁。0.9.0 新增
`AsyncFactoryContext` 用于把链显式传进任务。

### 排查

```rust
.async_singleton(|sp| Box::pin(async move {
    let ctx = sp.async_context().expect("built provider");
    let dep = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async::<Dep>().await }
    }).await.expect("task").expect("Dep");
    Arc::new(Service { dep })
}))
```

有环时 ctx 侧返回 `CircularDependency`；若工厂 `.unwrap()` 了该错误，顶层 `build_async`
得到 `FactoryPanic`，可用 `RdiError::is_circular()` 判定；注意该方法是**启发式**（payload
含 `CircularDependency` 文本即命中，自定义 payload 可能误判），精确形态优先匹配 typed 变体。

## 陷阱 14：同步 singleton 依赖 async 服务 / 同步解析或枚举 async 服务

### 现象

- `build_async` 返回 `RdiError::SyncSingletonDependsOnAsyncService { singleton, async_service }`；
- 已构建容器上同步 `get::<异步服务>()` 返回 `RdiError::AsyncServiceRequiresGetAsync`；
- `get_all` / `#[inject] Vec<Arc<T>>` 枚举时 panic（typed payload）或返回 `Err`。

### 原因

同步构造无法 `await`：同步 singleton 的校验/构建先于 async singleton 初始化。同步解析
未初始化的 async 服务也不可用（`get_optional` 返回 `None`，`is_registered` 仍为 `true`）。
枚举同理会包含**已初始化**的 async singleton；对**未初始化**的 async singleton/transient 同步枚举会得到 `AsyncServiceRequiresGetAsync`：`get_all` 签名不可失败，以 typed `RdiError` payload panic；async scoped 在 root 枚举会被严格 scope 校验省略（在 `Scope` 内枚举才 panic）；`get_all_any_result` / `Vec<Arc<T>>` 注入对任何未初始化 async 返回 `Err`。

### 排查

- 同步 singleton 不要依赖 async 服务：把依赖方也改为 async singleton，或在方法内用
  `Arc<dyn ScopeFactory>` / `get_async` 延迟解析。
- 解析 async 服务用 `get_async` / `get_keyed_async`；async scoped/transient 用 `get_async`
  逐项解析；`AnyKey` 兜底为同步注册，异步解析会回退同步路径。
- 含 `async_*` 注册的集合必须 `build_async()`（`build()` 返回
  `AsyncServiceRequiresBuildAsync`）。

## 陷阱 15：`ServiceKey::new("x")` 与字符串键不相等

### 现象

`keyed_singleton("x", …)` 注册后，`provider.get_keyed(ServiceKey::new("x"))` 返回
`KeyedServiceNotFound`（或 `is_registered_keyed` 为 `false`）。

### 原因

key 相等包含值的类型：`ServiceKey::new("x")` 是 `&str` typed key，而字符串注册 API 把 key
存为 `String`，两者不相等。这是 0.9.0 typed key 的 footgun。

### 排查

```rust
// ✅ 字符串键：用 from/into（或直接传 "x"）
provider.get_keyed::<dyn ITrait>("x")?;
provider.get_keyed::<dyn ITrait>(ServiceKey::from("x"))?;
provider.get_keyed::<dyn ITrait>("x".into())?;

// ✅ 类型化键：两边都用 ServiceKey::new(同一类型值)
.keyed_singleton::<dyn ITrait>(ServiceKey::new(EnumKey::X), ...)
provider.get_keyed::<dyn ITrait>(ServiceKey::new(EnumKey::X))?;
```

## 排查速查表

| 现象 | 可能原因 | 排查方向 |
|------|---------|---------|
| `service not registered` | TypeId 不匹配 | 检查注册是否 `::<dyn Trait>` |
| `keyed service not registered` | key 错误或未注册 | 检查 key 拼写 |
| `SingletonDependsOnScoped` | Singleton 依赖 Scoped | build 阶段已拒绝，改 Singleton 只依赖 Singleton |
| `ScopedFromRootProvider` | 默认严格 scope 校验：从 root 解析 Scoped | 改用 `provider.scope()` 子 Scope 解析；确需旧行为用 `lenient()` 构建 |
| `get_all` 拿不到某实现 | 该实现是 keyed 注册（`get_all` 只含非 keyed） | 改用 `get_all_keyed`，或把观察者/广播目标注册为非 keyed |
| `get_all`/`Vec` 枚举 panic 或 `Err(AsyncServiceRequiresGetAsync)` | 同步枚举未初始化的 async singleton/transient（async scoped 在 root 被 scope 校验省略、Scope 内才 panic） | 用 `get_async` 逐项解析；已初始化 async singleton 会被包含 |
| `get_all_keyed` 拿不到 AnyKey 兜底 | AnyKey 兜底不计入枚举（只列显式 key） | 用 `get_keyed(key)` 逐个解析；枚举语义对齐 MEDI |
| `get_keyed(ServiceKey::new("x"))` 找不到已注册 `"x"` | `&str` typed key ≠ 字符串注册的 `String` key | 用 `ServiceKey::from("x")` / `"x".into()`；`ServiceKey::new` 只用于 typed 键 |
| 以为 AnyKey scoped/transient owned 会被 root scope 拒绝 | owned 是例外，strict root 下允许 | 直接用 `get_keyed_owned`；不进缓存，传递依赖仍受校验 |
| AnyKey 工厂按 key 分支时想 downcast | 有意不提供 typed downcast 访问器 | 用 `Debug`/`Display`/`Eq`/`Hash` 比较，或闭包捕获类型信息 |
| `get_keyed(any_key())` 返回 Err | AnyKey 哨兵不能做单值解析 | 用具体 key（字符串/`ServiceKey::new`）解析 |
| async 工厂 spawn 任务后 build hang | 未传 `AsyncFactoryContext`，解析链断开 | 工厂内 `sp.async_context()`，clone 进任务用 `ctx.get_async`；`is_circular()` 为启发式 |
| `SyncSingletonDependsOnAsyncService` | 同步 singleton 依赖 async 服务 | 依赖方改 async singleton，或方法内 `ScopeFactory`/`get_async` 延迟解析 |
| `AsyncServiceRequiresGetAsync` | 同步 `get` 解析 async 服务（`get_optional` 返回 `None`，不报错） | 改用 `get_async`/`get_keyed_async` |
| `AsyncServiceRequiresBuildAsync` | 含 `async_*` 注册却调 `build()` | 改用 `build_async()` |
| 编译失败：工具链低于 MSRV | 0.10.x MSRV 为 Rust **1.95** | 升级工具链；CI 覆盖 1.95/stable 并跑 `cargo audit`/clippy 零警告 |
| 后注册的实现没生效 | 0.8.0 起为最后注册优先（0.10.x 同样适用），先注册的被后注册覆盖 | 检查注册顺序；需要"未注册才添加"用 `try_add*` |
| dispose 后解析报 `ScopeDisposed` / `ProviderDisposed` | 容器/作用域已释放 | 不要把已 dispose 的 provider/scope 继续传递使用 |
| 栈溢出 | 循环依赖 | 重构或延迟解析 |
| 编译失败 | 元组结构体 | 改命名结构体 |
| `duplicate key` | key 重复 | 确保 key 唯一 |
| `svc not registered` | 字段类型未注册 | 注册所有依赖 |
| `service not registered: dyn ScopeFactory`（eager singleton） | 0.7.0 无法在构建期注入容器服务 | 升级 0.7.1+；singleton 改用 `#[inject] Arc<dyn ScopeFactory>` |
| `get_owned: ... Singleton` | 对 Singleton 调 `get_owned` | 改用 Transient/Scoped，或用 `try_get_owned` 取 `None` |
| drop provider 后内存不释放 | singleton 持有 `Arc<ServiceProvider>` 引用环 | singleton 内改用弱引用的 `Arc<dyn ScopeFactory>` |
| `get_owned` 每次不同实例 | Scoped owned 绕过缓存（设计如此） | 需共享用 `get()`，需 `&mut self` 用 `get_owned()` |
| 同一服务能被 `get::<具体类型>()` 绕过 trait | struct 与 impl 同时用 `#[inject]`（双重注册） | struct 改 `#[derive(Inject)]`，仅 impl 上用 `#[inject]` |

---

下一节：[反模式警示](anti-patterns.md)