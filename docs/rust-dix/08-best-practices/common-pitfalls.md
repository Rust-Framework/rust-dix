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
ServiceCollection::new()
    .singleton::<dyn IServiceA>(|r| {
        let b: Arc<dyn IServiceB> = r.get();  // A 依赖 B
        Arc::new(ServiceA { b })
    })
    .singleton::<dyn IServiceB>(|r| {
        let a: Arc<dyn IServiceA> = r.get();  // B 依赖 A → 循环
        Arc::new(ServiceB { a })
    })
    .build()
    .unwrap();
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

> 注意：构造函数内通过 provider 发起的解析属于动态解析，不进入构建期静态囚徒依赖
> 链；不要依赖构造函数里 `provider.get()` 来获得 captive dependency 报错。

## 陷阱 5：根 Scope 与子 Scope 语义混淆

### 现象

期望从根 Provider 每次解析 Scoped 都得到新实例，但实际拿到的是同一实例；或期望根级 Scoped 跨请求隔离，但实例被永久持有。

### 原因

0.4.0 起，`ServiceProvider` 本身即 root scope。从根 Provider 解析 Scoped 服务时，实例缓存在 `root_scoped_cache`，**后续根级解析返回同一实例**（等同应用级单例），不会每次新建：

```rust
ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

let s1: Arc<Session> = provider.get();  // 根 Provider → 缓存在 root_scoped_cache
let s2: Arc<Session> = provider.get();  // s1 == s2（同一根 scope 实例）
```

### 排查

需要请求级隔离时，必须创建子 `Scope`：

```rust
let scope = provider.scope();
let s1: Arc<Session> = scope.get().unwrap();
let s2: Arc<Session> = scope.get().unwrap();
// s1 == s2（子 Scope 内缓存）

let scope2 = provider.scope();
let s3: Arc<Session> = scope2.get().unwrap();
// s3 != s1（跨子 Scope 独立）
```

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
2. **Scoped owned 绕过缓存**：`get_owned` 对 Scoped 绕过 `scoped_cache` /
   `root_scoped_cache`，每次调用创建新实例。这是有意为之——owned `&mut self`
   与 scope 共享语义不兼容。
3. **宏 owned 字段未注册或为 Singleton**：`#[derive(Inject)]` 的 `#[inject(owned)]` 裸 `T` 字段
   走 owned 路径，若该类型未注册或注册为 Singleton，构造失败返回 `Err`（含字段 `context`）。

```rust
// ❌ DbContext 注册为 Singleton，无法 owned
.singleton(|_| Arc::new(DbContext::new()))
// ...
let ctx: DbContext = provider.get_owned()?;  // Err

// ❌ 期望 Scoped owned 缓存复用（实际每次新建）
.scoped(|_| Arc::new(DbContext::new()))
let a = provider.get_owned::<DbContext>();
let b = provider.get_owned::<DbContext>();
// a、b 是不同实例（绕过缓存）
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

## 排查速查表

| 现象 | 可能原因 | 排查方向 |
|------|---------|---------|
| `service not registered` | TypeId 不匹配 | 检查注册是否 `::<dyn Trait>` |
| `keyed service not registered` | key 错误或未注册 | 检查 key 拼写 |
| `SingletonDependsOnScoped` | Singleton 依赖 Scoped | build 阶段已拒绝，改 Singleton 只依赖 Singleton |
| 根级 Scoped 不随请求变化 | root_scoped_cache 持久缓存 | 改用 `provider.scope()` 子 Scope 解析 |
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