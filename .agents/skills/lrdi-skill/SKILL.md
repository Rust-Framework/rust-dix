---
name: lrdi-skill
description: >
  使用 LRDI（Rust 依赖注入框架）构建松耦合、可测试、可插拔的 Rust 应用程序。
  涵盖 DI 容器注册/解析、生命周期管理、键控服务、分层容器、跨 DLL 插件、
  过程宏（#[derive(Inject)]、#[rust_dicore::inject]、#[rust_dicore::module]）以及策略、工厂、装饰器等
  设计模式的 LRDI 实现。当用户需要在 Rust 项目中引入依赖注入、构建插件系统、
  设计多层架构或实施可测试的服务解耦时使用此技能。
---

# LRDI 技能 · Agent 操作指令

你是 LRDI（Rust 依赖注入框架）专家。本文件告诉你如何使用 LRDI 解决用户问题。
详细 API 签名、架构原理、设计模式示例存放在本目录的支持文件中——在需要时加载它们。

---

## 何时激活此技能

当用户请求涉及以下任一场景时，加载此技能：

- 在 Rust 项目中引入 LRDI 依赖注入
- 设计或重构服务层架构（Controller → Service → Repository 等）
- 为已有 Rust 项目添加 DI 容器配置
- 实现插件系统、键控服务、分层容器
- 编写使用 LRDI 的单元测试或集成测试
- 使用 `#[derive(Inject)]`、`#[rust_dicore::inject]` 或 `#[rust_dicore::module]` 过程宏
- 跨 DLL/cdylib 边界的服务共享
- 从 .NET MEDI 迁移到 Rust
- 实施策略、工厂、装饰器等设计模式

**不激活的场景：** 极简脚本（< 3 个服务）、no_std 项目、纯算法问题。

---

## 支持文件索引

在回答问题前，根据问题类型加载相应的支持文件获取详细知识：

| 文件 | 加载条件 | 内容 |
|------|---------|------|
| `reference/api.md` | 用户询问具体 API 签名、方法参数、返回值 | 所有公开 API 的完整签名、行为语义、代码示例 |
| `guides/architecture.md` | 用户问"怎么设计架构"、"如何分层" | 架构原理、类型关系图、生命周期决策树、注册/解析流程图 |
| `guides/macros.md` | 用户使用 `#[derive(Inject)]`、`#[rust_dicore::inject]` 或 `#[rust_dicore::module]` | 过程宏的字段属性、声明语法、生成代码结构、编译错误排查 |
| `guides/testing.md` | 用户写测试或 mock 服务 | 测试 provider 构建、mock 替换策略、测试隔离模式 |
| `guides/plugin-system.md` | 用户构建插件系统、跨 DLL 服务 | 分层容器、命名服务、IServiceLocator、cdylib 插件生命周期 |
| `patterns/design-patterns.md` | 用户问"怎么实现策略/工厂/装饰器..." | 12 种设计模式的完整 LRDI 实现代码 |
| `patterns/recipes.md` | 用户需要完整的端到端示例 | Web 服务、CLI 工具、微内核插件、多租户 SaaS 等完整示例 |

使用 `read_file` 加载支持文件，路径为相对于本 SKILL.md 所在目录的路径，例如 `reference/api.md`。

---

## 核心工作流程

### 当用户要求"添加 LRDI 到我的项目"

1. 检查 `Cargo.toml`，添加 `rust-dicore = "0.4"` 到 `[dependencies]`
2. 在 `main.rs` 或 `lib.rs` 中添加 `use rust_dicore::*;`
3. 构建 ServiceProvider：
   - 确定需要哪些服务（列出所有 trait 和实现）
   - 为每个服务选择生命周期（见 `guides/architecture.md` 中的决策树）
   - 在应用启动时构建一次，用 `Arc::new(build_provider().unwrap())` 持有
4. 在需要服务的地方通过 `provider.get::<T>()` 解析，不要传递 provider 本身

### 当用户要注册服务

1. 确认服务类型：具体类型还是 `dyn Trait`？
2. **首选方案**：使用 `#[rust_dicore::inject]` 属性宏标注在 struct 上，自动完成注册和构造
   - `#[rust_dicore::inject(singleton)]` — 具体类型 Singleton
   - `#[rust_dicore::inject(transient)]` — 具体类型 Transient
   - 接口注册：在 struct 上用 `#[rust_dicore::inject(transient)]`，再在 `impl Trait for Type` 上用 `#[rust_dicore::inject]`
   - 支持自动解析字段依赖（通过 `#[rdi(...)]` 字段属性控制注入行为）
3. **备选方案**：手动 `ServiceCollection` API
   - 具体类型 + 全局唯一 → `.singleton(|_| Arc::new(T))`
   - 具体类型 + 每次新建 → `.transient(|_| Arc::new(T))`  
   - `dyn Trait` → `.singleton::<dyn Trait>(|_| Arc::new(Impl))`
   - 同类型多实例 → `.keyed_singleton("name", |_| Arc::new(T))`
   - 已有 Arc 实例 → `.instance(arc)`
4. **关键规则**：注册时的 `T` 类型决定了解析时的 `TypeId`。注册为 `dyn Trait` 才能以 `dyn Trait` 解析，注册为具体类型只能以具体类型解析。

### 注册语法参考（面向接口开发）

| 语法 | 含义 |
|------|------|
| `#[rust_dicore::inject]` (no arg) | 具体类型 Singleton（默认） |
| `#[rust_dicore::inject]` 在 `impl Trait for T` 上 | `dyn Trait` Singleton（默认） |
| `#[rust_dicore::inject(scoped)]` 在 `impl Trait for T` 上 | `dyn Trait` Scoped |
| `#[rust_dicore::inject(singleton)]` | 注册为具体类型 Singleton |
| `#[rust_dicore::inject(transient)]` | 注册为具体类型 Transient |
| `#[rust_dicore::inject(scoped)]` | 注册为具体类型 Scoped |

### 当用户要解析服务

1. 首选 `provider.get::<T>()`（简洁），失败时 panic
2. 可选服务用 `provider.get_optional::<T>()` 或 `provider.get_service::<T>()`
3. 键控服务用 `provider.get_keyed::<T>("key")`
4. 所有实例用 `provider.get_all::<T>()`
5. 在测试中，用 mock 实现替换真实服务（见 `guides/testing.md`）

### 当用户要设计架构

1. 加载 `guides/architecture.md`
2. 按层组织服务：基础设施（Singleton）→ 数据访问（Transient）→ 业务逻辑（Transient）
3. Web 服务：每个请求创建 Scope，请求级数据用 Scoped 生命周期
4. 插件系统：用 ServiceProviderWrapper 实现子优先/根兜底，用命名服务跨 DLL

### 当用户使用过程宏

1. 加载 `guides/macros.md`
2. **第一步（首选）**：`#[rust_dicore::inject(singleton)]` 直接标注在 struct 上，自动注册和构造
   - 标注生命周期参数（`singleton` / `transient` / `scoped`）
   - 字段通过 `#[rdi(...)]` 属性控制依赖注入行为
3. **第二步**：面向接口编程时，先在 struct 上用 `#[rust_dicore::inject]`（或 `#[derive(Inject)]`）生成构造函数，再在 `impl Trait for T` 块上用 `#[rust_dicore::inject]` 完成接口注册
4. **第三步（备选）**：复杂场景（简单工厂、键控服务、跨模块组装）用 `#[rust_dicore::module]` + `rust_dicore::register!()`
5. 检查编译错误：重复 key、非命名结构体、缺失 `Default` 实现

---

## 必须遵守的规则

### 规则 0：面向接口开发（最重要）

```rust
// ✅ 正确：依赖 trait，注册为 trait
trait IUserService: Send + Sync { fn create(&self, name: &str) -> User; }

#[rust_dicore::inject(singleton)]
struct UserService { db: Arc<dyn IDbContext> }      // 依赖接口

#[rust_dicore::inject]                              // 在 impl 上：注册为 dyn IUserService
impl IUserService for UserService { ... }

// Handler 依赖接口
#[rust_dicore::inject(transient)]
struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,  // 接口依赖
}

// ❌ 错误：依赖具体类型
struct CreateUserHandler {
    user_svc: Arc<UserService>,  // 具体类型，无法替换 mock
}
```

**contracts 层**：放 trait 定义 + DTO
**handlers 层**：放 trait 实现 + #[inject]

### 规则 1：注册类型 = 解析类型
```rust
// ✅ 正确
.singleton::<dyn Logger>(|_| Arc::new(FileLogger))  // 注册为 dyn Logger
let logger: Arc<dyn Logger> = provider.get();        // 解析为 dyn Logger

// ❌ 错误
.singleton(|_| Arc::new(FileLogger))                 // 注册为 FileLogger
let logger: Arc<dyn Logger> = provider.get();        // panic: TypeId 不匹配
```

### 规则 2：ServiceProvider 构建一次，全局持有
```rust
// ✅ 正确：main 中构建，Arc 持有
let provider = Arc::new(ServiceCollection::new()/*...*/.build().unwrap());

// ❌ 错误：每次请求重建
fn handle(req: Request) { let p = ServiceCollection::new()/*...*/.build().unwrap(); }
```

### 规则 3：不要传递 ServiceProvider 到处使用
```rust
// ✅ 正确：入口处解析依赖，注入到构造函数
let db: Arc<DbPool> = provider.get();
let svc = Arc::new(UserService::new(db));

// ❌ 错误：把 provider 传得到处都是
fn do_work(provider: &ServiceProvider) { let db = provider.get::<DbPool>(); }
```

### 规则 4：生命周期选择
- 全局共享不可变资源 → Singleton（配置、连接池、事件总线）
- 请求级别状态 → Scoped（请求上下文、事务、工作单元）
- 无状态轻量服务 → Transient（Repository、Service、Controller）
- **禁止**：Singleton 依赖 Scoped（build 阶段自动检测并拒绝）

### 规则 5：测试中替换服务
```rust
#[cfg(test)]
fn test_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MockDatabase::new()))  // 替换真实 DB
        .transient(|p| Arc::new(MyService::new(p.get())))
        .build().unwrap()
}
```

---

## 常见任务模板

### 模板 1：最小化 LRDI 项目（`#[rust_dicore::inject]` 属性宏）
```rust
use rust_dicore::*;
use std::sync::Arc;

#[rust_dicore::inject(singleton)]
struct Config;

#[rust_dicore::inject(transient)]
struct AppService {
    config: Arc<Config>,
}

// handlers 层：实现接口并注册
#[rust_dicore::inject(singleton)]
struct UserService {
    db: Arc<dyn IDbContext>,  // 依赖接口
}

#[rust_dicore::inject]  // 在 impl 上：注册为接口
impl IUserService for UserService {
    fn create(&self, name: &str) -> User { ... }
}

// Handler 依赖接口
#[rust_dicore::inject(transient)]
struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,  // 接口依赖，可测试可替换
}

fn main() {
    let provider = Arc::new(
        ServiceCollection::from_injected()
            .build()
            .unwrap()
    );
    let handler: Arc<CreateUserHandler> = provider.get();
    handler.user_svc.create("Alice");
}
```

### 模板 2：策略模式（keyed 多实现）
```rust
// 多实现策略
#[rust_dicore::inject(singleton)]
struct CreditCardGateway;

#[rust_dicore::inject]  // 在 impl 上：注册为默认 dyn IPayGateway
impl IPayGateway for CreditCardGateway { ... }

// 组合根补充 keyed 注册
let provider = ServiceCollection::from_injected()
    .keyed_singleton::<dyn IPayGateway>("wechat", |_| Arc::new(WechatGateway))
    .keyed_singleton::<dyn IPayGateway>("alipay", |_| Arc::new(AlipayGateway))
    .build()
    .unwrap();

// 运行时路由
let gateway: Arc<dyn IPayGateway> = provider
    .try_get_keyed(method)
    .unwrap_or_else(|| provider.get());  // 回退默认
```

### 模板 3：带作用域的 Web 请求处理
```rust
fn handle_request(provider: &Arc<ServiceProvider>, req: Request) -> Response {
    let scope = provider.create_scope();
    let ctx: Arc<RequestContext> = scope.get();
    let handler: Arc<RequestHandler> = scope.get();
    handler.handle(ctx, req)
}
```

### 模板 3：带 mock 的测试
```rust
#[test]
fn test_business_logic() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(MockRepo::new()))
        .transient(|p| Arc::new(MyService::new(p.get())))
        .build().unwrap();
    let svc: Arc<MyService> = p.get();
    let result = svc.do_something();
    assert!(result.is_ok());
}
```

---

## 常见错误排查

当用户遇到错误时，按以下顺序排查：

1. **panic: "service not registered: xxx"** → 检查注册时的类型是否与解析时的类型一致（规则 1）
2. **panic: "keyed service not registered: xxx:yyy"** → 检查 key 名称拼写，或用 `try_get_keyed` 替代
3. **`#[derive(Inject)]` 编译失败** → 检查是否用了元组结构体（不支持），加载 `guides/macros.md`
4. **`#[rust_dicore::inject]` 编译失败** → 检查 struct 是否为命名结构体或单元结构体（不支持元组结构体）
5. **`#[rust_dicore::inject]` 无法推断依赖** → 检查字段类型是否已注册，或使用 `#[rdi(provider)]` 手动获取
6. **`#[rust_dicore::module]` 编译失败** → 检查是否有重复 key，检查 `register!` 语法
7. **服务未按预期缓存** → 检查生命周期设置，Singleton 在 provider 构建时即初始化

---

## 处理流程总结

```
用户问题 → 判断场景 → 加载对应支持文件 → 理解 API/模式 → 生成代码 → 验证
    │
    ├── "怎么注册服务？" → 加载 reference/api.md → 选生命周期、选方法 → 写注册代码
    ├── "怎么设计架构？" → 加载 guides/architecture.md → 分层 → 选生命周期 → 写完整配置
    ├── "怎么用宏？"     → 加载 guides/macros.md → 写 #[derive]、#[rust_dicore::inject] 或 #[rust_dicore::module] → 检查编译
    ├── "怎么测试？"     → 加载 guides/testing.md → 构建 mock provider → 写测试
    ├── "做插件系统？"   → 加载 guides/plugin-system.md → 设计接口 → 分层容器 → 命名服务
    └── "实现设计模式？" → 加载 patterns/design-patterns.md → 选模式 → 套用代码模板

始终：
- 注册时明确 T 的类型（具体类型或 dyn Trait）
- 选择正确的生命周期
- 构建一次 provider，Arc 持有
- 在测试中替换实现以验证行为
```
