# 业务场景实战

本文档提供真实业务场景下的 rust-dix 完整使用方案，涵盖从项目初始化到上线部署的全流程。

> 本文示例采用简短路径风格，假定文件顶部已 `use rust_dix::*;`。导入约定详见 [用法指南](usage-guide.md#1-导入约定路径简化)。

---

## 场景 1：Web 应用完整架构（Axum / Actix-web）

### 项目结构

```
src/
├── main.rs                  # 组合根：构建容器、启动服务器
├── contracts/               # 接口层（trait + DTO）
│   ├── mod.rs
│   ├── user.rs              # IUserService trait
│   └── order.rs             # IOrderService trait
├── services/                # 业务实现层
│   ├── mod.rs
│   ├── user_service.rs
│   └── order_service.rs
├── repositories/            # 数据访问层
│   ├── mod.rs
│   ├── user_repo.rs
│   └── order_repo.rs
└── infrastructure/          # 基础设施
    ├── mod.rs
    ├── db.rs                # DbPool
    └── config.rs            # AppConfig
```

### 合约层：contracts/user.rs

```rust
use std::sync::Arc;

pub trait IUserService: Send + Sync {
    fn create(&self, name: &str, email: &str) -> Result<User, String>;
    fn find(&self, id: u64) -> Result<User, String>;
}

pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
}
```

### 实现层：services/user_service.rs

```rust
use rust_dix::*;
use std::sync::Arc;
use crate::contracts::user::*;
use crate::repositories::user_repo::IUserRepo;

// struct 用 #[derive(Inject)] 仅生成构造函数（不注册），impl 用 #[inject] 注册为 trait
#[derive(Inject)]
pub struct UserService {
    #[inject]
    repo: Arc<dyn IUserRepo>,
}

#[inject(transient)]
impl IUserService for UserService {
    fn create(&self, name: &str, email: &str) -> Result<User, String> {
        self.repo.insert(name, email)
    }
    fn find(&self, id: u64) -> Result<User, String> {
        self.repo.find_by_id(id)
    }
}
```

### 组合根：main.rs

```rust
use rust_dix::*;
use std::sync::Arc;

mod contracts;
mod services;
mod repositories;
mod infrastructure;

#[tokio::main]
async fn main() {
    // 1. 从过程宏自动收集所有 #[inject] 标注的服务
    let provider = ServiceCollection::from_injected()
        // 2. 补充手动注册的基础设施
        .async_singleton(|_| Box::pin(async {
            Arc::new(DbPool::connect("pg://localhost/app").await)
        }))
        .singleton(|_| Arc::new(AppConfig::from_env()))
        .build_async()
        .await
        .unwrap();

    // 3. 启动 Web 服务器，将 provider 注入 Axum State
    let app = axum::Router::new()
        .route("/users", axum::routing::post(create_user))
        .with_state(provider);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Axum Handler：每个请求创建独立 Scope
async fn create_user(
    axum::extract::State(provider): axum::extract::State<Arc<ServiceProvider>>,
    axum::extract::Json(req): axum::extract::Json<CreateUserRequest>,
) -> axum::Json<User> {
    let scope = provider.scope();
    let user_svc: Arc<dyn IUserService> = scope.get().unwrap();
    let user = user_svc.create(&req.name, &req.email).unwrap();
    axum::Json(user)
}
```

### 关键设计决策

| 决策 | 原因 |
|------|------|
| `build_async()` | 数据库连接池需要异步初始化 |
| 每个请求 `provider.scope()` | 请求级隔离，Scoped 服务（如 UnitOfWork）独立 |
| Handler 从 Scope 解析 | 确保 Scoped 服务绑定到当前请求 Scope |
| `#[inject(transient)]` 在 impl 上 | UserService 无状态，每次新建，便于 mock |
| struct 上用 `#[derive(Inject)]` | 仅生成构造函数，不注册为具体类型 |

---

## 场景 2：电商订单处理全链路

### 业务需求

订单处理涉及多个服务协作：用户验证 → 库存检查 → 支付 → 物流，每个环节可独立替换。

### 合约层

```rust
use std::sync::Arc;

// 支付网关接口
pub trait IPaymentGateway: Send + Sync {
    fn charge(&self, amount: u64) -> Result<String, String>;
    fn name(&self) -> &'static str;
}

// 库存服务接口
pub trait IInventoryService: Send + Sync {
    fn reserve(&self, product_id: u64, quantity: u32) -> Result<(), String>;
    fn release(&self, product_id: u64, quantity: u32);
}

// 订单服务接口
pub trait IOrderService: Send + Sync {
    fn place_order(&self, req: PlaceOrderRequest) -> Result<Order, String>;
}

pub struct PlaceOrderRequest {
    pub user_id: u64,
    pub product_id: u64,
    pub quantity: u32,
    pub payment_method: String,
}

pub struct Order {
    pub id: u64,
    pub status: OrderStatus,
    pub amount: u64,
}
```

### 实现：策略模式 + 工厂模式

```rust
use rust_dix::*;
use std::sync::Arc;

// 支付网关：键控服务实现策略模式
#[derive(Inject)]
struct WechatPay;

#[inject]
impl IPaymentGateway for WechatPay {
    fn charge(&self, amount: u64) -> Result<String, String> {
        Ok(format!("微信支付 ¥{}", amount))
    }
    fn name(&self) -> &'static str { "wechat" }
}

#[derive(Inject)]
struct Alipay;

#[inject]
impl IPaymentGateway for Alipay {
    fn charge(&self, amount: u64) -> Result<String, String> {
        Ok(format!("支付宝 ¥{}", amount))
    }
    fn name(&self) -> &'static str { "alipay" }
}

// 订单服务：依赖注入 + 策略路由
#[derive(Inject)]
pub struct OrderService {
    #[inject]
    user_svc: Arc<dyn IUserService>,
    #[inject]
    inventory_svc: Arc<dyn IInventoryService>,
    #[inject(provider)]
    provider: Arc<ServiceProvider>,  // 按需解析支付策略
}

#[inject(transient)]
impl IOrderService for OrderService {
    fn place_order(&self, req: PlaceOrderRequest) -> Result<Order, String> {
        // 1. 验证用户
        let user = self.user_svc.find(req.user_id)?;

        // 2. 预留库存
        self.inventory_svc.reserve(req.product_id, req.quantity)?;

        // 3. 按支付方式选择网关（策略路由）
        let gateway: Arc<dyn IPaymentGateway> = self.provider
            .try_get_keyed(&req.payment_method)
            .unwrap_or_else(|| self.provider.get().unwrap());

        let amount = req.quantity as u64 * 100;
        let tx_id = gateway.charge(amount)?;

        // 4. 创建订单
        Ok(Order {
            id: 1,
            status: OrderStatus::Paid,
            amount,
        })
    }
}
```

### 组合根

```rust
let provider = ServiceCollection::from_injected()
    // 键控服务：多支付网关
    .keyed_singleton::<dyn IPaymentGateway>("wechat", |_| Arc::new(WechatPay))
    .keyed_singleton::<dyn IPaymentGateway>("alipay", |_| Arc::new(Alipay))
    .build()
    .unwrap();
```

### 测试：Mock 替换策略

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct MockPaymentGateway;
    impl IPaymentGateway for MockPaymentGateway {
        fn charge(&self, _amount: u64) -> Result<String, String> {
            Ok("mock_tx_001".into())
        }
        fn name(&self) -> &'static str { "mock" }
    }

    #[test]
    fn test_place_order() {
        // 0.8.0：后注册的 mock 覆盖默认实现（最后注册优先）；
        // OrderService 由 #[inject(transient)] impl 自动注册，无需手写工厂。
        let provider = ServiceCollection::from_injected()
            .singleton::<dyn IUserService>(|_| Arc::new(MockUserService))
            .singleton::<dyn IInventoryService>(|_| Arc::new(MockInventoryService))
            .keyed_singleton::<dyn IPaymentGateway>("wechat", |_| Arc::new(MockPaymentGateway))
            .build()
            .unwrap();

        let order_svc: Arc<dyn IOrderService> = provider.get().unwrap();
        let result = order_svc.place_order(PlaceOrderRequest {
            user_id: 1,
            product_id: 100,
            quantity: 2,
            payment_method: "wechat".into(),
        });
        assert!(result.is_ok());
    }
}
```

---

## 场景 3：多租户 SaaS 架构

### 架构特点

- 每个租户独立数据库（或 schema）
- 请求时根据租户 ID 动态选择数据源
- 租户间服务共享，数据隔离

### 实现

```rust
use rust_dix::*;
use std::sync::Arc;
use std::collections::HashMap;

// 多租户连接池：按租户存储连接
struct TenantDbPool {
    pools: HashMap<String, Arc<DbPool>>,
    default: Arc<DbPool>,
}

impl TenantDbPool {
    fn resolve(&self, tenant_id: &str) -> Arc<DbPool> {
        self.pools.get(tenant_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.default))
    }
}

// 仓储：根据租户上下文动态选择数据源
#[derive(Inject)]
struct TenantAwareRepo {
    #[inject(provider)]
    provider: Arc<ServiceProvider>,
}

impl TenantAwareRepo {
    fn get_db(&self) -> Arc<DbPool> {
        let tenant_pool: Arc<TenantDbPool> = self.provider.get().unwrap();
        let tenant_id = self.get_current_tenant();
        tenant_pool.resolve(&tenant_id)
    }
}

// 请求级 Scope：每个请求绑定租户上下文
#[inject(scoped)]
struct TenantContext {
    tenant_id: String,
    tenant_db: Arc<DbPool>,
}

// 组合根
let provider = ServiceCollection::new()
    .async_singleton(|_| Box::pin(async {
        let mut pools = HashMap::new();
        pools.insert("tenant_a".into(), Arc::new(DbPool::connect("pg://tenant-a").await));
        pools.insert("tenant_b".into(), Arc::new(DbPool::connect("pg://tenant-b").await));
        Arc::new(TenantDbPool {
            pools,
            default: Arc::new(DbPool::connect("pg://default").await),
        })
    }))
    .scoped(|r| {
        let any = r
            .get_any(std::any::type_name::<TenantDbPool>())
            .expect("TenantDbPool must be registered");
        let tenant_pool: Arc<TenantDbPool> =
            Arc::clone(&*any.downcast::<Arc<TenantDbPool>>().expect("TenantDbPool downcast"));
        let tenant_id = "tenant_a".into();  // 实际从请求头解析
        Arc::new(TenantContext {
            tenant_db: tenant_pool.resolve(&tenant_id),
            tenant_id,
        })
    })
    .build_async()
    .await
    .unwrap();

// 请求处理
fn handle_request(provider: &Arc<ServiceProvider>, tenant_id: &str) {
    let scope = provider.scope();
    let ctx: Arc<TenantContext> = scope.get().unwrap();
    // ctx.tenant_db 指向该租户的数据库
    let repo: Arc<TenantAwareRepo> = scope.get().unwrap();
    // repo 可通过 provider 获取 ctx 拿到租户数据库
}
```

### 关键设计决策

| 决策 | 原因 |
|------|------|
| `TenantContext` 注册为 Scoped | 每个请求绑定一个租户，Scope 内共享 |
| `TenantDbPool` 注册为 Singleton | 全局共享连接池映射 |
| `#[inject(provider)]` 注入 Provider | 仓储需要动态解析当前租户上下文 |

---

## 场景 4：插件系统架构

### 需求

- 宿主程序提供核心功能（事件总线、配置）
- 插件通过 `cdylib` 动态加载
- 每个插件有独立 DI 容器，但可访问宿主服务

### 宿主端

```rust
use rust_dix::*;
use std::sync::Arc;

// 定义插件接口（在宿主 crate 中）
pub trait IPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&self, host: Arc<dyn IHostServices>);
    fn execute(&self, input: &str) -> String;
}

pub trait IHostServices: Send + Sync {
    fn get_event_bus(&self) -> Arc<EventBus>;
    fn get_config(&self) -> Arc<AppConfig>;
}

// 宿主容器
let host_provider = ServiceCollection::new()
    .singleton(|_| Arc::new(EventBus::new()))
    .singleton(|_| Arc::new(AppConfig::from_env()))
    .build()
    .unwrap();

// 注册命名服务供插件访问（跨 DLL 必须用命名服务）
host_provider.register_named("event_bus", host_provider.get::<EventBus>().unwrap());
host_provider.register_named("config", host_provider.get::<AppConfig>().unwrap());

// 插件容器（每个插件独立）
let plugin_provider = ServiceCollection::new()
    .singleton::<dyn IPlugin>(|_| Arc::new(MyPlugin::new()))
    .build()
    .unwrap();

// 分层容器：插件优先，宿主回退
let wrapper = ServiceProviderWrapper::new(plugin_provider, host_provider);

// 插件解析：自己的服务优先，找不到回退宿主
let plugin: Arc<dyn IPlugin> = wrapper.get().unwrap();  // 从插件容器解析
let bus: Arc<EventBus> = wrapper.get_named("event_bus").unwrap();  // 回退宿主的命名服务
```

### 插件端（cdylib）

```rust
// 插件 crate 中
use rust_dix::*;
use std::sync::Arc;

#[derive(Inject)]
struct MyPlugin {
    // 插件自己的服务
    #[inject]
    handler: Arc<PluginHandler>,
}

#[inject]
impl IPlugin for MyPlugin {
    fn name(&self) -> &'static str { "my_plugin" }
    fn init(&self, host: Arc<dyn IHostServices>) {
        let bus = host.get_event_bus();
        bus.subscribe("order.created", |data| self.handler.process(data));
    }
    fn execute(&self, input: &str) -> String {
        self.handler.process(input)
    }
}
```

### 关键设计决策

| 决策 | 原因 |
|------|------|
| 命名服务而非 TypeId | 跨 DLL 编译单元 TypeId 不同，无法用类型解析 |
| `ServiceProviderWrapper` 分层 | 插件优先，宿主回退，天然隔离 |
| 插件接口定义在宿主 | 宿主控制插件契约，插件只实现 |

---

## 场景 5：测试驱动开发（TDD）

### 测试策略层次

```rust
#[cfg(test)]
mod tests {
    use rust_dix::*;
    use std::sync::Arc;

    // 1. Mock 实现
    struct MockUserRepo {
        users: std::sync::Mutex<Vec<User>>,
    }

    impl IUserRepo for MockUserRepo {
        fn find_by_id(&self, id: u64) -> Result<User, String> {
            self.users.lock().unwrap()
                .iter()
                .find(|u| u.id == id)
                .cloned()
                .ok_or("not found".into())
        }
        fn insert(&self, name: &str, email: &str) -> Result<User, String> {
            let user = User { id: 1, name: name.into(), email: email.into() };
            self.users.lock().unwrap().push(user.clone());
            Ok(user)
        }
    }

    // 2. 测试容器工厂：from_injected() 收集 #[inject] 自动注册，
    //    后注册的 mock 覆盖默认实现（0.8.0 最后注册优先）。
    fn test_provider() -> ServiceProvider {
        ServiceCollection::from_injected()
            .singleton::<dyn IUserRepo>(|_| Arc::new(MockUserRepo {
                users: std::sync::Mutex::new(vec![]),
            }))
            .build()
            .unwrap()
    }

    #[test]
    fn test_create_user() {
        let provider = test_provider();
        let user_svc: Arc<dyn IUserService> = provider.get().unwrap();

        let user = user_svc.create("Alice", "alice@example.com").unwrap();
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "alice@example.com");
    }

    #[test]
    fn test_find_user_not_found() {
        let provider = test_provider();
        let user_svc: Arc<dyn IUserService> = provider.get().unwrap();

        let result = user_svc.find(999);
        assert!(result.is_err());
    }
}
```

### 测试策略决策表

| 测试层次 | 如何构建容器 | Mock 策略 |
|---------|------------|----------|
| 单元测试（Service） | 手动构建，只注册被测服务 + Mock | 所有依赖替换为 Mock |
| 集成测试（Handler） | 手动构建，注册真实 Repo + Mock 外部服务 | 外部服务 Mock，内部服务真实 |
| 端到端测试 | `from_injected()` + 覆盖基础设施 | 仅替换外部 API 适配器 |

---

## 场景 6：后台任务 / 定时器（每 tick 独立 scope）

### 架构约束

- 后台 worker（`IHostedService`、定时器、消息消费者）是 **Singleton**，不能持有
  `DbContext` 等 Scoped 实例。
- 每个 tick / 每个任务创建一个独立 `Scope`，在 scope 内解析 Scoped 服务，结束即释放。
- 推荐注入内置 `Arc<dyn ScopeFactory>`：无需手动注册，0.7.1+ 在 eager singleton
  中也可注入；内置实现持 `Weak` 引用，不会形成 `容器 → worker → 容器` 的 `Arc` 环。

### 实现

```rust
use rust_dix::*;
use std::sync::Arc;

#[derive(Inject)]
struct OutboxWorker {
    #[inject]
    scope_factory: Arc<dyn ScopeFactory>, // 内建服务，无需注册
}

impl OutboxWorker {
    fn tick(&self) {
        let scope = self.scope_factory.create_scope();
        let mut ctx: DbContext = scope.get_owned().unwrap(); // 每 tick 新实例
        // ... 处理一批消息 ...
        // scope 结束：scope.get() 缓存的 scoped 资源随 scope 释放；
        // get_owned 绕过缓存、由调用方持有（本函数末尾 drop），不会进入 scope 缓存。
    }
}
```

### 检查点

| 项 | 要求 |
|----|------|
| 生命周期 | worker = Singleton；`DbContext` 等工作单元 = Scoped；每 tick 一个 scope |
| 捕获 | worker 不得保存 `DbContext` / `Scope` / 任何 Scoped 服务 |
| 清理 | 每个 scope 结束（显式 `dispose()` 或 drop） |
| 注入 | 优先 `Arc<dyn ScopeFactory>`；用 `#[inject(provider)]` 时自行建 scope，并注意强引用环 |
| 关闭 | 先停止 worker，再 `provider.dispose()`（按创建逆序释放 singleton/root scoped） |
| 测试 | 直接调用 tick / 处理函数，验证跨 tick 状态不泄漏 |

---

## 0.8.0 语义要点（贯穿所有场景）

| 主题 | 0.8.0 行为 | 场景中的影响 |
|------|-----------|--------------|
| 单服务解析 | 最后注册优先 | 测试/组合根用"后注册 mock"覆盖默认实现；需要"未注册才添加"用 `try_add*` |
| 多实现收集 | `get_all` 仅非 keyed；keyed 用 `get_all_keyed` | 观察者广播（场景 4/5）把广播目标注册为非 keyed |
| Scope 校验 | 默认严格：root 解析 Scoped 报 `ScopedFromRootProvider` | 场景 1/3 的请求级服务必须在 `provider.scope()` 内解析 |
| 释放契约 | `Scope::dispose()` / Drop 逆序释放；新增 `ServiceProvider::dispose()` | Web 服务器关闭、后台 worker 停止时显式释放 |
| 构建顺序 | eager singleton 按注册顺序创建 | 异步基础设施（场景 1 的 DbPool/Redis）初始化顺序可依赖 |

> 逐条差异与迁移见 [与 MEDI 的差异清单](../01-introduction/medi-differences.md)。

## 生命周期选择速查

| 服务类型 | 生命周期 | 示例 |
|---------|---------|------|
| 数据库连接池 | Singleton (async) | `DbPool`、`RedisClient` |
| 全局配置 | Singleton | `AppConfig`、`FeatureFlags` |
| 事件总线 | Singleton | `EventBus`、`MessageQueue` |
| 请求上下文 | Scoped | `TenantContext`、`RequestContext` |
| 工作单元 / 事务 | Scoped | `UnitOfWork`、`DbContext` |
| 业务服务（无状态） | Transient | `UserService`、`OrderService` |
| 校验器 | Transient | `EmailValidator`、`PasswordValidator` |
| 策略实现 | 按需（Singleton/Transient） | 支付网关、通知渠道 |

---

## 反模式速查

贯穿上述所有场景的通用反模式：

| 反模式 | 错误做法 | 正确做法 |
|--------|---------|---------|
| 依赖具体类型 | `Arc<UserService>` | `Arc<dyn IUserService>` |
| 注册为具体类型 | `.singleton(\|_\| Arc::new(Impl))` | `.singleton::<dyn Trait>(...)` |
| 传递 Provider 到处使用 | `provider: Arc<ServiceProvider>` 当服务定位器到处传 | 显式依赖字段；仅按需路由/后台 worker 建 scope 时注入 `provider` 或 `ScopeFactory` |
| 每次请求重建容器 | 请求内 `build()` | 构建一次，`scope()` 创建 Scope |
| Singleton 依赖 Scoped | `#[inject(singleton)]` 包含 Scoped 字段 | Singleton 只依赖 Singleton |
| struct 与 impl 同时 `#[inject]` | 双重注册 | struct 用 `#[derive(Inject)]`，impl 用 `#[inject]` |

---

下一节：性能优化建议（**规划中**，尚未发布；可先阅读 [用法指南](usage-guide.md) 的容器使用规则）。