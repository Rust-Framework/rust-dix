# 反模式警示

本文档列举 rust-dicore 开发中的常见反模式及其修正方案。

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
// Handler 依赖接口
#[rust_dicore::inject(singleton)]
pub struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,  // 接口
    logger: Arc<dyn ILogger>,         // 接口
}

#[rust_dicore::inject]
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
let logger: Arc<dyn ILogger> = provider.get();  // panic
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
#[rust_dicore::inject(singleton)]
pub struct UserService { ... }

#[rust_dicore::inject]
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
#[rust_dicore::inject(singleton)]
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
#[rust_dicore::inject(scoped)]
pub struct Session { ... }

// GlobalService 注册为 Singleton
#[rust_dicore::inject(singleton)]
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
#[rust_dicore::inject(scoped)]
pub struct RequestHandler {
    config: Arc<AppConfig>,       // Singleton → OK
    session: Arc<Session>,        // Scoped → OK
}

// Singleton 只依赖 Singleton
#[rust_dicore::inject(singleton)]
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
        let scope = provider.create_scope();
        let handler = scope.get::<dyn IRequestHandler>();
        handler.handle(req);
    }
}
```

---

## 反模式 8：根 Provider 解析 Scoped 期望每请求独立

### ⚠️ 语义澄清

0.4.0 起，`ServiceProvider` 本身即 root scope。从根 Provider 解析 Scoped 服务时，实例缓存在 `root_scoped_cache` 中，**后续所有根级解析返回同一实例**（等同于应用级单例）。

```rust
// Session 注册为 Scoped
ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

// 从根 Provider 解析 — 缓存在 root_scoped_cache
let session: Arc<Session> = provider.get();
let session2: Arc<Session> = provider.get();
// session 与 session2 是同一实例（根 scope 缓存）
```

**反模式**：期望根 Provider 每次 `get::<Scoped>()` 返回不同实例（误以为是 Transient 语义）。
**问题**：根 scope 缓存永远不会释放，请求级数据被永久持有，无法实现"每请求一个 Session"。

### ✅ 正确做法

需要请求级隔离时，必须从子 `Scope` 解析：

```rust
// 从子 Scope 解析 Scoped 服务
let scope = provider.create_scope();
let session1: Arc<Session> = scope.get();
let session2: Arc<Session> = scope.get();
// session1 与 session2 是同一实例（子 Scope 内缓存）

let scope2 = provider.create_scope();
let session3: Arc<Session> = scope2.get();
// session3 与 session1 不同（跨 Scope 独立）
```

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
| 根 Provider 解析 Scoped | Scope 语义丢失 | 从 Scope 解析 |
| Handler 直接调用 | 绕过 DI | 通过 Mediator 或注入服务 |

---

下一节：[性能优化建议](performance-tips.md)