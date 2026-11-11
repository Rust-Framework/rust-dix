# 面向接口开发清单

本文档提供面向接口开发的规则速查表，供代码审查和开发参考。

> 本文示例采用简短路径风格，假定文件顶部已 `use rust_dix::*;`。导入约定详见 [用法指南](usage-guide.md#1-导入约定路径简化)。

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
| 在 `impl Trait for Type` 上加 `#[inject]` | 直接 `singleton` 注册为具体类型 |
| 字段依赖 `Arc<dyn Trait>` | 字段依赖具体实现类型 |
| 自动注册到容器 | 在 main.rs 手动逐个注册 |
| struct 用 `#[derive(Inject)]` 仅生成构造函数 | struct 与 impl 同时用 `#[inject]`（双重注册） |

```rust
// ✅ handlers/user.rs
// struct 用 #[derive(Inject)] 仅生成构造函数（不注册），impl 用 #[inject] 注册为 dyn IUserService
#[derive(Inject)]
pub struct UserService {
    db: Arc<dyn IDbContext>,       // 接口依赖
    logger: Arc<dyn ILogger>,      // 接口依赖
}

#[inject]
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

let logger: Arc<dyn ILogger> = provider.get()?;  // Err: TypeId 不匹配
```

### 多实现用 keyed

| 场景 | 注册方式 |
|------|---------|
| 默认策略 | `.singleton::<dyn Trait>(...)` |
| 替代策略 | `.keyed_singleton::<dyn Trait>("name", ...)` |
| 类型化键 | `.keyed_singleton::<dyn Trait>(ServiceKey::new(EnumKey::A), ...)` |
| 任意 key 兜底 | `.keyed_any_singleton::<dyn Trait>(\|_, key\| ...)`（精确 key 优先、按 (服务注册/类型, 请求 key) 缓存） |
| 批量获取（非 keyed） | `.get_all::<dyn Trait>()` |
| 批量获取（显式 keyed） | `.get_all_keyed::<dyn Trait>()`（不含 AnyKey 兜底） |

```rust
// ✅ 多实现注册（字符串或类型化 ServiceKey 均可）
ServiceCollection::new()
    .singleton::<dyn IPayGateway>(|_| Arc::new(WechatPay))  // 默认
    .keyed_singleton::<dyn IPayGateway>("wechat", |_| Arc::new(WechatPay))
    .keyed_singleton::<dyn IPayGateway>(ServiceKey::new(PayKey::Alipay), |_| Arc::new(Alipay))
    .keyed_any_singleton::<dyn IPayGateway>(|_, _key| Arc::new(DefaultGateway)) // 兜底
    .build()
    .unwrap();
```

> 注意：`get_all_keyed` 不含 AnyKey 兜底；`get_keyed(ServiceKey::any_key())` 非法。

### 多接口注册

一个实现注册为多个 trait：

```rust
// ✅ 一个实现暴露多个接口：struct 用 #[derive(Inject)] 仅生成构造函数（不注册），两个 impl 块各注册一个 trait
#[derive(Inject)]
pub struct FileStream {
    path: String,
}

#[inject]
impl IReader for FileStream { ... }
#[inject]
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
// ✅ 正确：Handler 推荐放 impl 上——struct 用 #[derive(Inject)] 仅生成构造函数，impl 用 #[inject] 注册为 trait
#[derive(Inject)]
pub struct CreateUserHandler {
    #[inject]
    user_svc: Arc<dyn IUserService>,   // 接口
    #[inject]
    logger: Option<Arc<dyn ILogger>>,  // 可选接口
}

#[inject]
impl IRequestHandler<CreateUserRequest, User> for CreateUserHandler { /* ... */ }

// ❌ 错误
pub struct CreateUserHandler {
    user_svc: Arc<UserService>,  // 具体类型
}
```

### 生命周期依赖方向

| 外层 | 可依赖 | 不可依赖 |
|------|--------|---------|
| Singleton（同步） | Singleton / Transient（二者都非 captive） / 内建 `ScopeFactory`（弱引用） | Scoped；**async 服务**（build 期 `SyncSingletonDependsOnAsyncService`） |
| Singleton（async） | 其它 async singleton（依赖优先初始化） / 同步 singleton | — |
| Scoped | Singleton / Scoped / Transient | — |
| Transient | 所有 | 无限制 |

```rust
// ❌ 错误：Singleton 依赖 Scoped
#[inject(singleton)]
pub struct GlobalService {
    session: Arc<Session>,  // Session 注册为 Scoped → build 阶段拒绝
}

// ❌ 错误：同步 singleton 依赖 async 服务（build_async 返回 SyncSingletonDependsOnAsyncService）
#[inject(singleton)]
pub struct Repository {
    db: Arc<AsyncDb>,       // async_singleton 注册 → 同步构造无法 await
}

// ✅ 正确：Scoped 依赖 Singleton
#[inject(scoped)]
pub struct RequestHandler {
    config: Arc<AppConfig>,  // Singleton → OK
}
```

### 异步解析规则

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| async 服务用 `get_async` / `get_keyed_async` 解析 | 同步 `get` 解析 async 服务（`AsyncServiceRequiresGetAsync`） |
| async scoped/transient 用 `get_async` 逐项解析 | 同步 `get_all`/`Vec` 枚举未初始化 async（panic 或 Err；async scoped 在 root 被 scope 校验省略） |
| async 工厂内需要 spawn 时传 `AsyncFactoryContext` | 完全不传 ctx 的跨任务解析（可能死锁） |
| 用 `err.is_circular()` 判定直接/包裹的环（**启发式**） | 只匹配 `CircularDependency` 而漏掉 `FactoryPanic` 包裹；或仅凭启发式做精确分支 |
| 含 `async_*` 注册时用 `build_async()` | 用 `build()`（`AsyncServiceRequiresBuildAsync`） |

## 四、容器使用规则

### 构建一次，全局持有

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| main.rs 构建一次 | 每次请求重建 |
| `Arc::new(provider)` 传递 | 传递 ServiceCollection |
| 需要时解析服务 | 传递 Provider 到处使用 |
| 后台任务注入 `Arc<dyn ScopeFactory>` 每 tick 建 scope | 让 Singleton 持有 Scoped 实例或 `Scope` |
| 应用关闭显式 `provider.dispose()`（逆序释放） | 依赖 `Arc` 最后一个引用释放来清理 |

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
| 请求开始创建子 Scope | 从 root Provider 解析 Scoped（默认严格校验会报 `ScopedFromRootProvider`） |
| 子 Scope 内缓存 Scoped 服务 | 在根 Provider 上解析 Scoped 期望请求级隔离 |
| 请求结束显式 `scope.dispose()` 或让其 Drop | 长期持有 `Scope` 或 scoped 实例 |

```rust
// ✅ 正确：Web 请求处理
fn handle_request(provider: &Arc<ServiceProvider>, req: Request) {
    let scope = provider.scope();
    let uow: Arc<UnitOfWork> = scope.get().unwrap();  // Scope 内共享
    let handler: Arc<dyn IRequestHandler> = scope.get().unwrap();
    handler.handle(&uow, req);
} // scope Drop → 按创建逆序释放 scoped 实例

// ❌ 错误：0.8.0 默认严格 scope 校验下，root provider 解析 Scoped 返回 Err
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

let session = provider.get::<Session>();        // Err(ScopedFromRootProvider)
let session2 = provider.get::<Session>();       // 同样 Err

// ✅ 需要请求级隔离：用 scope()
let scope = provider.scope();
let session: Arc<Session> = scope.get().unwrap();
// 同一 scope 内缓存；跨 scope 独立

// ⚠️ 仅当确需 0.7.x 的 root-scope 语义（root 解析 Scoped 并缓存）时：
let provider = ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build_with(ServiceProviderOptions::lenient())
    .unwrap();
let s1: Arc<Session> = provider.get().unwrap();
let s2: Arc<Session> = provider.get().unwrap(); // s1 == s2（根 scope 缓存）
```

## 五、测试规则

### Mock 替换策略

| ✅ 应该 | ❌ 避免 |
|--------|--------|
| 注册为 trait 才可替换 | 注册为具体类型无法替换 |
| 测试容器只改注册 | 修改业务代码 |
| Mock 实现 trait | Mock 继承具体类型 |
| 覆盖注册放在最后（**最后注册优先**） | 依赖"先注册优先"的旧行为 |

```rust
// ✅ 正确：测试中替换（后注册的 mock 覆盖默认实现）
#[cfg(test)]
fn test_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton::<dyn IUserService>(|_| Arc::new(RealUserService)) // 默认
        .singleton::<dyn IUserService>(|_| Arc::new(MockUserService)) // 生效
        .singleton::<dyn IDbContext>(|_| Arc::new(MockDbContext))
        .build()
        .unwrap()
}

#[test]
fn test_create_user() {
    let p = test_provider();
    let handler: Arc<dyn IRequestHandler> = p.get().unwrap();
    // 使用 mock 验证
}
```

## 六、代码审查清单

审查代码时检查以下项：

| 类别 | 检查项 |
|------|--------|
| 分层 | trait 是否在 contracts / 实现是否在 handlers |
| 依赖 | 字段是否都是 `Arc<dyn Trait>` |
| 注册 | 是否在 `impl Trait for Type` 上加 `#[inject]` |
| 放置位置 | struct 与 impl 是否同时用 `#[inject]`（应二选一；面向接口时 struct 用 `#[derive(Inject)]`） |
| 多实现 | 是否用 keyed 注册替代实现；key 是否需要类型化（`ServiceKey::new`）或 AnyKey 兜底；字符串键是否用 `from/into`（`ServiceKey::new("x")` 与 `String` key 不等） |
| 生命周期 | Singleton 不依赖 Scoped；同步 singleton 不依赖 async 服务（build 阶段自动检测） |
| 异步 | async 服务用 `get_async`；跨任务解析是否传 `AsyncFactoryContext` |
| 错误处理 | 环错误是否用 `RdiError::is_circular()` 判定（而非只匹配 `CircularDependency`） |
| 工程基线 | 工具链 ≥ **1.95**；CI 的 fmt/clippy/doc/`cargo audit` 通过；无 `unsafe`；见 `SECURITY.md` |
| 容器 | 是否只构建一次 |
| 测试 | 是否可替换 mock |

### 工程基线（0.9.0）

| 项 | 要求 |
|----|------|
| MSRV | Rust **1.95**（`rust-version`），CI matrix 覆盖 1.95 / stable |
| CI | `.github/workflows/ci.yml`：ubuntu+windows × 1.95/stable 测试；stable 跑 fmt/clippy/doc（`-D warnings`）+ RustSec `cargo audit` |
| 安全 | 两个 crate `#![forbid(unsafe_code)]`；漏洞上报/支持版本/安全边界见 `SECURITY.md` |

---

下一节：[常见陷阱与排查](common-pitfalls.md)