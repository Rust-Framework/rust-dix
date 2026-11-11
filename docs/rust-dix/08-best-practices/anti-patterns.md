# 反模式警示

本文档列举 rust-dix 开发中的常见反模式及其修正方案。

> 本文示例采用简短路径风格，假定文件顶部已 `use rust_dix::*;`。导入约定详见 [用法指南](usage-guide.md#1-导入约定路径简化)。

## 反模式 1：依赖具体类型

### ❌ 错误做法

```rust
// Handler 依赖具体实现类型
pub struct CreateUserHandler {
    user_svc: Arc<UserService>,  // 暴露具体类型
    logger: Arc<FileLogger>,      // 暴露具体类型
}
```

**问题**：
- 无法在测试中替换 mock
- 替换实现需修改 Handler 代码
- 违反依赖倒置原则

### ✅ 正确做法

```rust
// Handler 依赖接口：struct 用 #[derive(Inject)] 仅生成构造函数，impl 用 #[inject] 注册为 trait
#[derive(Inject)]
pub struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,  // 接口
    logger: Arc<dyn ILogger>,         // 接口
}

#[inject]
impl IRequestHandler<CreateUserRequest, User> for CreateUserHandler { /* ... */ }
```

---

## 反模式 2：注册为具体类型

### ❌ 错误做法

```rust
ServiceCollection::new()
    .singleton(|_| Arc::new(UserService))  // 注册为 UserService
    .singleton(|_| Arc::new(FileLogger))    // 注册为 FileLogger
    .build()
    .unwrap();

// 无法按 trait 解析
let logger: Arc<dyn ILogger> = provider.get()?;  // Err(ServiceNotFound)
```

### ✅ 正确做法

```rust
ServiceCollection::new()
    .singleton::<dyn IUserService>(|_| Arc::new(UserService))
    .singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
    .build()
    .unwrap();

let logger: Arc<dyn ILogger> = provider.get();  // OK
```

---

## 反模式 3：trait 放在实现模块

### ❌ 错误做法

```rust
// handlers/user.rs
pub trait IUserService: Send + Sync {  // trait 定义在实现模块
    fn create(&self, name: &str) -> Result<User, String>;
}

pub struct UserService;
impl IUserService for UserService { ... }
```

**问题**：
- 接口暴露实现细节
- contracts 层无法引用接口
- 分层边界模糊

### ✅ 正确做法

```rust
// contracts/user.rs
pub trait IUserService: Send + Sync {
    fn create(&self, name: &str) -> Result<User, String>;
}

// handlers/user.rs
use contracts::user::IUserService;

pub struct UserService;
impl IUserService for UserService { ... }
```

---

## 反模式 4：main.rs 逐个注册

### ❌ 错误做法

```rust
// main.rs
fn main() {
    let provider = ServiceCollection::new()
        .singleton::<dyn IUserService>(|_| Arc::new(UserService))
        .singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
        .singleton::<dyn IDbContext>(|_| Arc::new(DbContext))
        // ... 逐个注册所有服务
        .build()
        .unwrap();
}
```

**问题**：
- 新增服务需修改 main.rs
- 组合根臃肿难维护
- 编译期无法自动发现

### ✅ 正确做法

```rust
// handlers/user.rs
// struct 用 #[derive(Inject)] 仅生成构造函数（不注册），impl 用 #[inject] 注册为 dyn IUserService
#[derive(Inject)]
pub struct UserService { ... }

#[inject]
impl IUserService for UserService { /* ... */ }

// main.rs
fn main() {
    let provider = Arc::new(
        ServiceCollection::from_injected()
            .singleton(|_| Arc::new(AppPaths::resolve()))  // 只注册基础设施
            .build()
            .unwrap()
    );
}
```

---

## 反模式 5：传递 Provider 到处使用

### ❌ 错误做法

```rust
// 业务代码直接使用 Provider
pub struct OrderService {
    provider: Arc<ServiceProvider>,  // Provider 作为字段
}

impl OrderService {
    fn process(&self, order: Order) {
        let user_svc = self.provider.get::<dyn IUserService>();  // 服务定位器模式
        let pay_svc = self.provider.get::<dyn IPayGateway>();
        // ...
    }
}
```

**问题**：
- 依赖不透明（隐藏了真正依赖什么）
- 运行时才发现依赖缺失
- 违反显式依赖原则

### ✅ 正确做法

```rust
// 显式声明依赖
#[inject(singleton)]
pub struct OrderService {
    user_svc: Arc<dyn IUserService>,  // 显式依赖
    pay_svc: Arc<dyn IPayGateway>,    // 显式依赖
}

impl OrderService {
    fn process(&self, order: Order) {
        self.user_svc.check_user(order.user_id);
        self.pay_svc.pay(order.amount);
    }
}
```

---

## 反模式 6：Singleton 依赖 Scoped

### ❌ 错误做法

```rust
// Session 注册为 Scoped
#[inject(scoped)]
pub struct Session { ... }

// GlobalService 注册为 Singleton
#[inject(singleton)]
pub struct GlobalService {
    session: Arc<Session>,  // Singleton 依赖 Scoped
}
```

**问题**：
- 0.4.0 起 build 阶段直接拒绝，返回 `RdiError::SingletonDependsOnScoped`
- Singleton 会永久持有 Scoped 实例（captive dependency），破坏 scope 隔离语义
- 间接依赖链（`Singleton → Transient → Scoped`）同样被检测拒绝

### ✅ 正确做法

```rust
// Scoped 服务只依赖 Singleton 或 Scoped
#[inject(scoped)]
pub struct RequestHandler {
    config: Arc<AppConfig>,       // Singleton → OK
    session: Arc<Session>,        // Scoped → OK
}

// Singleton 只依赖 Singleton
#[inject(singleton)]
pub struct GlobalService {
    config: Arc<AppConfig>,       // Singleton → OK
}
```

---

## 反模式 7：每次重建容器

### ❌ 错误做法

```rust
// 每次请求重建容器
fn handle_request(req: Request) -> Response {
    let provider = ServiceCollection::new()
        .singleton(|_| Arc::new(UserService))
        .build()
        .unwrap();  // 每次重建
    
    let handler = provider.get::<dyn IRequestHandler>();
    handler.handle(req)
}
```

**问题**：
- 性能开销巨大（构建、初始化）
- Singleton 语义丢失（每次重建）
- 浪费资源

### ✅ 正确做法

```rust
// 构建一次，全局持有
fn main() {
    let provider = Arc::new(ServiceCollection::from_injected().build().unwrap());
    
    // 请求处理复用容器
    for req in requests {
        let scope = provider.scope();
        let handler = scope.get::<dyn IRequestHandler>();
        handler.handle(req);
    }
}
```

---

## 反模式 8：从根 Provider 解析 Scoped（或仍按 0.7.x root-scope 语义设计）

### ⚠️ 语义澄清（0.9.x）

0.8.0 默认开启严格 scope 校验（`ServiceProviderOptions::validate_scopes = true`）。
从根 Provider 解析 Scoped 服务返回 `Err(RdiError::ScopedFromRootProvider)`：

```rust
// Session 注册为 Scoped
ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

// 从根 Provider 解析 — 默认严格校验下直接失败
let session = provider.get::<Session>();       // Err(ScopedFromRootProvider)
let session2 = provider.get::<Session>();      // 同样 Err
```

**反模式**：
- 仍按 0.7.x 的"根 Provider 即 root scope、根级 Scoped 缓存复用"设计代码；
- 期望根 Provider 每次 `get::<Scoped>()` 返回不同实例（误以为是 Transient 语义）；
- 在应用级代码里用 `lenient()` 掩盖"本该用 scope"的结构问题。

**问题**：请求级数据被当成根级单例、作用域边界消失；严格模式下则直接解析失败。

### ✅ 正确做法

需要请求级隔离时，必须从子 `Scope` 解析：

```rust
// 从子 Scope 解析 Scoped 服务
let scope = provider.scope();
let session1: Arc<Session> = scope.get().unwrap();
let session2: Arc<Session> = scope.get().unwrap();
// session1 与 session2 是同一实例（子 Scope 内缓存）

let scope2 = provider.scope();
let session3: Arc<Session> = scope2.get().unwrap();
// session3 与 session1 不同（跨 Scope 独立）
```

> 若确实需要 0.7.x 的 root-scope 行为（root 解析 Scoped 并缓存为根内单例），必须显式
> `build_with(ServiceProviderOptions::lenient())`，并在文档中说明为何需要根级共享。
> 详见 [与 MEDI 的差异清单](../01-introduction/medi-differences.md#2-scope-校验validatescopes)。

---

## 反模式 9：Handler 内直接调用

### ❌ 错误做法

```rust
// Handler 直接调用其他 Handler
impl IRequestHandler<CreateOrderRequest, Order> for CreateOrderHandler {
    fn handle(&self, req: CreateOrderRequest) -> Result<Order, String> {
        // 直接调用其他 Handler
        let user_handler = CreateUserHandler { ... };
        user_handler.handle(CreateUserRequest { ... });  // 绕过 DI
    }
}
```

**问题**：
- 绕过 DI 容器
- 无法注入依赖
- 无法替换 mock

### ✅ 正确做法

```rust
// Handler 通过 Mediator 或 DI 调用
impl IRequestHandler<CreateOrderRequest, Order> for CreateOrderHandler {
    fn handle(&self, req: CreateOrderRequest) -> Result<Order, String> {
        // 通过注入的 Mediator 发送请求
        self.mediator.send(CreateUserRequest { ... })?;
        // 或注入依赖的服务
        self.user_svc.create(...)?;
    }
}
```

---

## 反模式 10：struct 与 impl 同时用 `#[inject]`（双重注册）

### ❌ 错误做法

```rust
// struct 和 impl 都用 #[inject] → 容器里既有 UserHandler（具体类型）又有 dyn IUserHandler
#[inject(transient)]
struct UserHandler {
    #[inject]
    user_svc: Arc<dyn IUserService>,
}

#[inject]
impl IUserHandler for UserHandler { /* ... */ }  // 消费者可 get::<UserHandler>() 绕过接口
```

**问题**：
- 同一 struct 被注册两次（具体类型 + `dyn Trait`），通常非预期
- 消费者能绕过 trait 接口直接 `get::<具体类型>()`，破坏面向接口原则
- 破坏可 mock 性——测试中替换 `dyn IUserHandler` 后，`get::<UserHandler>()` 仍返回真实实现

### ✅ 正确做法

`#[inject]` 放置位置二选一：面向接口时 struct 上用 `#[derive(Inject)]`（仅生成构造函数、不注册），impl 上用 `#[inject]` 注册为 `dyn Trait`。

```rust
// Handler 推荐放 impl 上：struct 用 #[derive(Inject)] 仅生成构造函数，impl 用 #[inject] 注册为 trait
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

| 目标身份 | struct 上 | impl 上 | 结果 |
|---------|----------|---------|------|
| 仅具体类型（无 trait，如配置/值对象） | `#[inject]` | *不放* | ✅ |
| 仅 `dyn Trait`（面向接口，**Handler/Service/Repository 推荐**） | `#[derive(Inject)]` | `#[inject]` | ✅ |
| 两者都要（罕见，确需同时暴露） | `#[inject]` | `#[inject]` | ⚠️ 需注释说明意图 |
| 仅 impl、struct 无构造函数 | *不放* | `#[inject]` | ❌ 编译失败 |

**何时放 struct 上**：服务作为具体类型对外暴露（配置、值对象，无 trait 契约）。
**何时放 impl 上**：服务有 trait 契约，面向接口便于 mock（Handler / Service / Repository，**推荐**）。

### 为什么宏无法编译时检测双重注册？

双重注册只能靠门禁 A9/B6 在代码生成阶段拦截，过程宏本身**无法**编译时检测，原因有三：

1. **过程宏跨调用无状态**：`#[inject]` 标在 struct 上和标在 impl 上是两次独立的宏调用，彼此看不见对方，Rust 过程宏按设计即无共享状态。
2. **TypeId 不冲突**：struct 放置注册为 `TypeId::of::<T>()`（具体类型），impl 放置注册为 `TypeId::of::<dyn Trait>()`（trait 对象）—— 两者 TypeId 不同，在 `ServiceStore` 中落入不同桶，无运行时冲突可检测。
3. **双重注册是被支持的特性**：测试 `inject_combined_concrete_and_trait` 明确验证同一 struct 同时注册为具体类型 + `dyn Trait` 两者均可解析（"Concrete is also usable as the trait"）。加检测会破坏此合法用例（罕见但确需同时暴露）。

因此，双重注册的防范完全依赖**文档约束 + 门禁 A9/B6**，而非编译期检查。

> 详见 [用法指南 · 放置位置决策](usage-guide.md#放置位置struct-还是-impl二选一避免双重注册) 与 [常见陷阱 · 陷阱 10](common-pitfalls.md#陷阱-10struct-与-impl-同时用-inject双重注册)。

---

## 反模式 11：同步 singleton 依赖 async 服务

### ❌ 错误做法

```rust
// AsyncDb 用 async_singleton 注册
#[inject(singleton)]
struct Repository {
    #[inject]
    db: Arc<AsyncDb>,   // 同步构造函数无法 await
}
```

### 问题（0.9.0 精确诊断）

`build_async` 在同步 singleton 校验阶段返回
`RdiError::SyncSingletonDependsOnAsyncService { singleton, async_service }`（不会 hang）。
同步构造无法 `await`，该依赖方向在架构上不成立。

### ✅ 正确做法

```rust
// 方案 A：把依赖方也改为 async singleton
#[rust_dix::inject]   // async singleton 的注册方式按项目约定，如 async_singleton 闭包
async fn build_repository(db: Arc<AsyncDb>) -> Repository { Repository { db } }

// 方案 B：方法内延迟解析（注入 ScopeFactory / provider，在方法里 get_async）
#[derive(Inject)]
struct Handler {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,
}

impl Handler {
    async fn handle(&self) {
        let db: Arc<AsyncDb> = self.provider.get_async().await.unwrap();
        // ...
    }
}
```

---

## 反模式 12：AnyKey 误用 / 跨任务不传 AsyncFactoryContext

### ❌ 错误做法

```rust
// 1) 用 AnyKey 哨兵做单值解析
let svc = provider.get_keyed::<dyn IStorage>(ServiceKey::any_key()); // Err

// 2) 期望 get_all_keyed 枚举 AnyKey 兜底
let all = provider.get_all_keyed::<dyn IStorage>(); // 只含显式 key

// 3) spawn 任务解析时不传 ctx：跨任务环可能 hang
tokio::spawn(async move { sp.get_async::<Dep>().await });
```

### ✅ 正确做法

- 单值解析用具体 key（字符串或 `ServiceKey::new(...)`）；`AnyKey` 只是注册侧兜底标记。
- `get_all_keyed` 只列显式 key；需要遍历兜底请按业务 key 逐个 `get_keyed`。
- 需要在 spawned task 中解析时，工厂内 `let ctx = sp.async_context()`，clone 进任务并用
  `ctx.get_async::<T>()`；环返回 `CircularDependency`（或 `FactoryPanic`，用
  `RdiError::is_circular()` 判定）。

---

## 反模式速查表

| 反模式 | 问题 | 正确做法 |
|--------|------|---------|
| 依赖具体类型 | 无法替换 mock | `Arc<dyn Trait>` |
| 注册为具体类型 | 无法按 trait 解析 | `singleton::<dyn Trait>(...)` |
| trait 在实现模块 | 分层边界模糊 | contracts 层放 trait |
| main.rs 逐个注册 | 组合根臃肿 | `from_injected()` |
| 传递 Provider | 依赖不透明 | 显式依赖字段 |
| Singleton 依赖 Scoped | 生命周期混乱 | Singleton 只依赖 Singleton |
| 每次重建容器 | 性能开销 | 构建一次 Arc 持有 |
| 从根 Provider 解析 Scoped | 默认严格校验下解析失败；旧语义导致作用域丢失 | 从 `scope()` 解析；确需旧行为显式 `lenient()` |
| 误以为 `get_all` 含 keyed | 观察者/广播漏掉 keyed 实现 | 非 keyed 用 `get_all`，keyed 用 `get_all_keyed` |
| 同步 singleton 依赖 async 服务 | build 期 `SyncSingletonDependsOnAsyncService` | 依赖方改 async，或方法内 `ScopeFactory`/`get_async` 延迟解析 |
| 用 `any_key()` 做单值解析 | 返回 Err；哨兵只是注册侧标记 | 用具体 key（字符串 / `ServiceKey::new`） |
| 字符串键用 `ServiceKey::new("x")` | `&str` typed key ≠ 字符串注册的 `String` key | 用 `ServiceKey::from("x")` / `"x".into()` |
| 同步枚举未初始化 async（`get_all`/`Vec`） | typed panic 或 `Err(AsyncServiceRequiresGetAsync)`（async scoped 在 root 被 scope 校验省略） | async scoped/transient 用 `get_async` 逐项解析 |
| 以为 AnyKey scoped/transient owned 会被 root scope 拒绝 | owned 是例外，strict root 下允许 | 直接用 `get_keyed_owned`；不进缓存，传递依赖仍受校验 |
| 跨任务解析不传 `AsyncFactoryContext` | 跨任务环可能死锁 | 工厂内 `sp.async_context()` 并 clone 进任务 |
| Handler 直接调用 | 绕过 DI | 通过 Mediator 或注入服务 |
| struct 与 impl 同时用 `#[inject]` | 双重注册，可绕过接口 | struct 用 `#[derive(Inject)]`，impl 用 `#[inject]` |

---

下一节：性能优化建议（**规划中**，尚未发布；可先阅读 [用法指南](usage-guide.md) 的容器使用规则）。