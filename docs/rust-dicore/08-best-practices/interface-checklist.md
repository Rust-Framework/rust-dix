# 面向接口开发清单

本文档提供面向接口开发的规则速查表，供代码审查和开发参考。

## 一、分层规则

### contracts 层（接口层）

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 定义 trait（如 `IUserService`） | 放具体实现 |
| 定义 DTO（请求/响应结构体） | 放依赖 `Arc<dyn Trait>` 的字段 |
| trait 方法签名简单清晰 | trait 方法依赖具体类型 |
| 仅依赖标准库或框架 | 依赖 domain 或 handlers 模块 |

```rust
// ✅ contracts/user.rs
pub trait IUserService: Send + Sync {
    fn create(&self, req: CreateUserRequest) -> Result<User, String>;
    fn find(&self, id: u64) -> Result<User, String>;
}

pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
}

pub struct User {
    pub id: u64,
    pub name: String,
}
```

### handlers 层（实现层）

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 实现 trait | 定义 trait |
| 在 `impl Trait for Type` 上加 `#[rust_dicore::inject]` | 直接 `singleton` 注册为具体类型 |
| 字段依赖 `Arc<dyn Trait>` | 字段依赖具体实现类型 |
| 自动注册到容器 | 在 main.rs 手动逐个注册 |

```rust
// ✅ handlers/user.rs
#[rust_dicore::inject(singleton)]
pub struct UserService {
    db: Arc<dyn IDbContext>,       // 接口依赖
    logger: Arc<dyn ILogger>,      // 接口依赖
}

#[rust_dicore::inject]
impl IUserService for UserService {
    fn create(&self, req: CreateUserRequest) -> Result<User, String> {
        // ...
    }
}
```

### composition_root（组合根）

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 注册基础设施（配置、DbContext） | 注册业务 Service |
| 注册第三方库适配器 | 注册 Handler |
| 调用 `from_injected()` | 手动注册所有服务 |
| 一次性构建 Provider | 多次重建容器 |

```rust
// ✅ composition_root/bootstrap.rs
pub fn configure(mut svc: ServiceCollection) -> ServiceCollection {
    let paths = AppPaths::resolve();
    svc = svc.singleton(move |_| Arc::new(paths));
    svc = svc.singleton(move |_| Arc::new(DbContext::new()));
    svc
}

// main.rs
let provider = Arc::new(
    ServiceCollection::from_injected()
        .singleton(|_| Arc::new(AppPaths::resolve()))
        .build()
        .unwrap()
);
```

## 二、注册规则

### 注册类型 = 解析类型

| 规则 | 正确示例 | 错误示例 |
|------|---------|---------|
| 注册为 trait，解析为 trait | `.singleton::<dyn ILogger>(...)` + `.get::<dyn ILogger>()` | `.singleton(|_| Arc::new(FileLogger))` + `.get::<dyn ILogger>()` |

```rust
// ✅ 正确
ServiceCollection::new()
    .singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
    .build()
    .unwrap();

let logger: Arc<dyn ILogger> = provider.get();  // OK

// ❌ 错误
ServiceCollection::new()
    .singleton(|_| Arc::new(FileLogger))  // 注册为 FileLogger
    .build()
    .unwrap();

let logger: Arc<dyn ILogger> = provider.get();  // panic: TypeId 不匹配
```

### 多实现用 keyed

| 场景 | 注册方式 |
|------|---------|
| 默认策略 | `.singleton::<dyn Trait>(...)` |
| 替代策略 | `.keyed::<dyn Trait>("name", ...)` |
| 批量获取 | `.get_all::<dyn Trait>()` |

```rust
// ✅ 多实现注册
ServiceCollection::new()
    .singleton::<dyn IPayGateway>(|_| Arc::new(WechatPay))  // 默认
    .keyed::<dyn IPayGateway>("wechat", |_| Arc::new(WechatPay))
    .keyed::<dyn IPayGateway>("alipay", |_| Arc::new(Alipay))
    .build()
    .unwrap();
```

### 多接口注册

一个实现注册为多个 trait：

```rust
// ✅ 一个实现暴露多个接口
#[rust_dicore::inject(singleton)]
pub struct FileStream {
    path: String,
}

#[rust_dicore::inject]
impl IReader for FileStream { ... }
#[rust_dicore::inject]
impl IWriter for FileStream { ... }

// 分别解析
let reader: Arc<dyn IReader> = provider.get();
let writer: Arc<dyn IWriter> = provider.get();
```

## 三、依赖规则

### Handler 依赖接口

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| `Arc<dyn IUserService>` | `Arc<UserService>` |
| `Arc<dyn ILogger>` | `Arc<FileLogger>` |
| `Option<Arc<dyn IService>>` | `Option<Arc<ServiceImpl>>` |

```rust
// ✅ 正确
#[rust_dicore::inject(singleton)]
pub struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,   // 接口
    #[rdi(optional)]
    logger: Option<Arc<dyn ILogger>>,  // 可选接口
}

#[rust_dicore::inject]
impl IRequestHandler<CreateUserRequest, User> for CreateUserHandler { /* ... */ }

// ❌ 错误
pub struct CreateUserHandler {
    user_svc: Arc<UserService>,  // 具体类型
}
```

### 生命周期依赖方向

| 外层 | 可依赖 | 不可依赖 |
|------|--------|---------|
| Singleton | Singleton | Scoped / Transient |
| Scoped | Singleton / Scoped | Transient |
| Transient | 所有 | 无限制 |

```rust
// ❌ 错误：Singleton 依赖 Scoped
#[rust_dicore::inject(singleton)]
pub struct GlobalService {
    session: Arc<Session>,  // Session 注册为 Scoped → 行为异常
}

// ✅ 正确：Scoped 依赖 Singleton
#[rust_dicore::inject(scoped)]
pub struct RequestHandler {
    config: Arc<AppConfig>,  // Singleton → OK
}
```

## 四、容器使用规则

### 构建一次，全局持有

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| main.rs 构建一次 | 每次请求重建 |
| `Arc::new(provider)` 传递 | 传递 ServiceCollection |
| 需要时解析服务 | 传递 Provider 到处使用 |

```rust
// ✅ 正确
fn main() {
    let provider = Arc::new(ServiceCollection::from_injected().build().unwrap());
    let handler: Arc<dyn IRequestHandler> = provider.get();
    handler.handle(req);
}

// ❌ 错误
fn handle(req: Request, provider: &ServiceProvider) {
    let svc = provider.get::<dyn IService>();  // Provider 传递过多
}
```

### 作用域正确使用

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 请求开始创建 Scope | 根 Provider 解析 Scoped |
| Scope 内缓存 Scoped 服务 | Scope 外缓存 Scoped 服务 |

```rust
// ✅ 正确：Web 请求处理
fn handle_request(provider: &Arc<ServiceProvider>, req: Request) {
    let scope = provider.create_scope();
    let uow: Arc<UnitOfWork> = scope.get();  // Scope 内共享
    let handler: Arc<dyn IRequestHandler> = scope.get();
    handler.handle(&uow, req);
}

// ❌ 错误：根 Provider 解析 Scoped
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

let session: Arc<Session> = provider.get();  // 等价于 Transient，Scope 语义丢失
```

## 五、测试规则

### Mock 替换策略

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 注册为 trait 才可替换 | 注册为具体类型无法替换 |
| 测试容器只改注册 | 修改业务代码 |
| Mock 实现 trait | Mock 继承具体类型 |

```rust
// ✅ 正确：测试中替换
#[cfg(test)]
fn test_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton::<dyn IUserService>(|_| Arc::new(MockUserService))
        .singleton::<dyn IDbContext>(|_| Arc::new(MockDbContext))
        .build()
        .unwrap()
}

#[test]
fn test_create_user() {
    let p = test_provider();
    let handler: Arc<dyn IRequestHandler> = p.get();
    // 使用 mock 验证
}
```

## 六、代码审查清单

审查代码时检查以下项：

| 类别 | 检查项 |
|------|--------|
| 分层 | trait 是否在 contracts / 实现是否在 handlers |
| 依赖 | 字段是否都是 `Arc<dyn Trait>` |
| 注册 | 是否在 `impl Trait for Type` 上加 `#[rust_dicore::inject]` |
| 多实现 | 是否用 keyed 注册替代实现 |
| 生命周期 | Singleton 是否依赖 Scoped |
| 容器 | 是否只构建一次 |
| 测试 | 是否可替换 mock |

---

下一节：[常见陷阱与排查](common-pitfalls.md)