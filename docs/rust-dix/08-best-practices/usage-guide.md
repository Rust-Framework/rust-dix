# 用法指南与决策矩阵

本文档是 rust-dix 的推荐用法权威指南：如何简化导入路径、在不同场景下选择哪种注册方式与构造函数策略，以及让代码更简洁的实用技巧。

> 阅读本文前，建议先浏览 [面向接口开发清单](interface-checklist.md) 建立整体认知。

---

## 1. 导入约定（路径简化）

rust-dix 的宏与类型都从 `rust_dix` crate 重新导出。**推荐在每个使用 DI 的文件顶部加两行 use**，之后所有宏和类型可直接短名引用：

```rust
use rust_dix::*;        // 引入所有宏与类型
use std::sync::Arc;        // 共享依赖的 Arc 包装
```

`use rust_dix::*` 后可直接使用：

| 类别 | 可用名称 |
|------|---------|
| 属性宏 | `#[inject]`、`#[module]` |
| 派生宏 | `#[derive(Inject)]` |
| 函数式宏 | `register!()` |
| 类型 | `ServiceCollection`、`ServiceProvider`、`Scope`、`IServiceResolver`、`RdiError` |

**简化对照表：**

| 旧写法（全限定） | 推荐写法（简短） |
|------------------|-----------------|
| `#[rust_dix::inject]` | `#[inject]` |
| `#[rust_dix::inject(scoped)]` | `#[inject(scoped)]` |
| `#[derive(rust_dix::Inject)]` | `#[derive(Inject)]` |
| `#[rust_dix::module]` | `#[module]` |
| `rust_dix::register!(...)` | `register!(...)` |
| `rust_dix::ServiceCollection::new()` | `ServiceCollection::new()` |

**大型多 crate 项目**：若担心 glob 引入污染命名空间，可用别名：

```rust
use rust_dix as di;    // 之后写 #[di::inject]、di::register!()
```

> 约定：本文档及最佳实践章节的所有示例均采用 `use rust_dix::*` 简短风格。参考文档（如宏详解）为明确宏来源可能使用全限定路径，实际编码请用简短形式。

---

## 2. 注册方式决策矩阵

框架提供四种注册途径，按场景选择：

| 场景 | 推荐方式 | 一句话理由 |
|------|---------|-----------|
| 标准服务自动注册 | `#[inject]` 属性宏 | 零样板，注册 + 构造一步到位 |
| 面向接口 trait 注册 | `#[inject]` 在 `impl Trait for T` 块 | trait 自动推断，无需 `as =` |
| 多实现运行时路由 | keyed + 属性宏/闭包 | 一个接口多实现，策略模式 |
| 类型化键 / 任意 key 兜底 | `ServiceKey::new(value)` + `keyed_any_*` | 键语义稳定（enum/整数）或需要 AnyKey 兜底 |
| 集中声明式配置 | `#[module]` + `register!()` | 注册声明与服务模块放在一起 |
| 已有实例 / 配置值 | `singleton_value(T)` / `instance(Arc<T>)` | 免写工厂闭包 |
| 条件可选注册 | `try_add(...)` | 已注册则跳过，不覆盖 |
| 复杂工厂 / 运行时参数 | 手动闭包 `\|p\| Arc::new(...)` | 需要计算逻辑或外部参数 |

**示例对照：**

```rust
use rust_dix::*;
use std::sync::Arc;

// 场景 1：标准服务 —— 属性宏零样板
#[inject(singleton)]
struct Config { port: u16 }

// 场景 2：接口注册 —— impl 块自动推断 trait
struct PgUserRepo;
#[inject] impl IUserRepo for PgUserRepo { /* ... */ }

// 场景 3：多实现路由 —— keyed 手动注册（keyed 不支持 #[inject] 自动注册）
// 字符串键用 "x".into()/ServiceKey::from("x")；ServiceKey::new("x") 是 &str typed key，
// 与 keyed_singleton("x", …) 的 String key 不相等。
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IPayGateway>("wechat", |_| Arc::new(WechatPay))
    .keyed_singleton::<dyn IPayGateway>("alipay", |_| Arc::new(AlipayPay))
    .build()?;

// 场景 5：已有实例 —— singleton_value 自动 Arc
let provider = ServiceCollection::new()
    .singleton_value(Config { port: 8080 })
    .build()?;

// 场景 7：复杂工厂 —— 手动闭包。闭包参数是 &dyn IServiceResolver；
// 泛型 get::<T>() 带 Self: Sized，不能直接在 trait object 上调用，
// 这里用 get_any + downcast 解析配置（或让 #[derive(Inject)] 生成构造函数）。
let provider = ServiceCollection::new()
    .singleton(|r| {
        let cfg = r
            .get_any(std::any::type_name::<Config>())
            .expect("Config registered at composition root");
        let cfg: Arc<Config> = Arc::clone(&*cfg.downcast::<Arc<Config>>().unwrap());
        Arc::new(Pool::connect(&cfg.url))
    })
    .build()?;
```

### 放置位置：struct 还是 impl？（二选一，避免双重注册）

`#[inject]` 放在 struct 上 = 注册为**具体类型**；放在 `impl Trait for T` 上 = 注册为 **`dyn Trait`**。两者同时用会让同一 struct 被注册两次（具体类型 + trait），通常不是预期——消费者能绕过 trait 直接 `get::<具体类型>()`，破坏面向接口原则。

关键区分：`#[derive(Inject)]` 与 `#[inject]` 都能在 struct 侧生成构造函数，但**只有 `#[inject]` 会向容器提交注册**。因此面向接口时 struct 上应选 `#[derive(Inject)]`（仅构造、不注册）。

| 目标注册身份 | struct 上 | impl 上 | 结果 |
|------------|----------|---------|------|
| 仅具体类型（无 trait 契约） | `#[inject]` | *不放* | ✅ 单一注册 |
| 仅 `dyn Trait`（面向接口，**推荐**） | `#[derive(Inject)]` | `#[inject]` | ✅ 单一注册 |
| 两者都要（罕见，确需同时暴露） | `#[inject]` | `#[inject]` | ⚠️ 双重注册，需注释说明意图 |
| 仅 impl、struct 无构造函数 | *不放* | `#[inject]` | ❌ 编译失败（无 `__rdi_construct`） |

```rust
use rust_dix::*;
use std::sync::Arc;

// ✅ Handler 面向接口：struct 用 #[derive(Inject)] 仅生成构造函数，impl 用 #[inject] 注册为 trait
#[derive(Inject)]
struct UserHandler {
    #[inject]
    user_svc: Arc<dyn IUserService>,
}

#[inject]  // 仅注册为 dyn IUserHandler
impl IUserHandler for UserHandler {
    fn handle(&self, req: Req) -> Resp { /* ... */ }
}

// ❌ 双重注册：struct 和 impl 都用 #[inject] → 容器里既有 UserHandler 又有 dyn IUserHandler
#[inject(transient)]
struct UserHandler { /* ... */ }

#[inject]
impl IUserHandler for UserHandler { /* ... */ }  // 消费者可 get::<UserHandler>() 绕过接口

// ✅ 无 trait 契约的简单服务（配置、值对象）：只在 struct 上用 #[inject]
#[inject(singleton)]
struct Config { port: u16 }
```

**何时放 struct 上**：服务作为具体类型对外暴露——无 trait，或消费者直接用具体类型（如配置、值对象）。
**何时放 impl 上**：服务有 trait 契约，面向接口便于 mock 替换——Handler / Service / Repository 均属此类，**推荐**。此时 struct 上用 `#[derive(Inject)]` 而非 `#[inject]`。
**Handler 推荐**：放 impl 上（注册为 trait），struct 上用 `#[derive(Inject)]`。

---

## 3. 构造函数策略：手写 vs 宏标记 vs 闭包

这是最常见的设计抉择。三种方式各有适用场景，**不互相排斥，可在同一项目混用**。

> **工厂闭包的解析约束**：注册闭包（`.singleton(f)` 等）的参数类型是
> `&dyn IServiceResolver`，而泛型 `get::<T>()` / `get_owned::<T>()` 带
> `Self: Sized` 约束，**不能直接在 trait object 上调用**。闭包内解析依赖有三种正确
> 方式：① 用 `#[derive(Inject)]` / `#[inject]` 生成的构造函数（tests 中的
> `|r| __rdi_construct_T(r)`，该函数为 `#[doc(hidden)]` 内部面）；② 用
> `get_any` + `downcast` 手动解析；③ 在闭包外解析好依赖，以 `Arc` 捕获进闭包。

### 三种方式对比

| 方式 | 代码量 | 结构体干净度 | 适用场景 |
|------|--------|-------------|---------|
| 手写构造函数 + 闭包 | 中 | 最干净（字段零属性） | 初始化逻辑复杂、字段需计算 |
| `#[inject]` 字段标记 | 最少 | 中（依赖字段有标记） | 依赖都是可直接解析的服务 |
| 手动闭包工厂 | 中 | 最干净 | 已有实例、条件构造、运行时参数 |

### 方式 A：手写构造函数（结构体最干净）

结构体字段不带任何 DI 属性，保持纯净。在闭包中调用自定义构造函数：

```rust
use rust_dix::*;
use std::sync::Arc;

pub struct OrderService {
    repo: Arc<dyn IOrderRepo>,    // 字段无任何属性，结构体干净
    threshold: u32,               // 内部状态
}

impl OrderService {
    // 自定义构造函数，完全掌控初始化逻辑
    pub fn new(repo: Arc<dyn IOrderRepo>, threshold: u32) -> Self {
        Self { repo, threshold }
    }
}

// 注册时用闭包调用构造函数。闭包参数是 &dyn IServiceResolver：
// 依赖在闭包外解析好、以 Arc 捕获（也可用 get_any + downcast，或让
// #[derive(Inject)] 生成的构造函数内部完成解析）。
let repo: Arc<dyn IOrderRepo> = Arc::new(PgOrderRepo);
let provider = ServiceCollection::new()
    .singleton(move |_| Arc::new(OrderService::new(Arc::clone(&repo), 100)))
    .build()?;
```

**适用**：初始化需要计算、字段不适合直接注入、想保持结构体完全无关 DI。

### 方式 B：`#[inject]` 字段标记（零样板）

用 `#[derive(Inject)]` 或 `#[inject]` 属性宏自动生成构造函数，依赖字段显式标记：

```rust
use rust_dix::*;
use std::sync::Arc;

#[inject(singleton)]
pub struct OrderService {
    #[inject]
    repo: Arc<dyn IOrderRepo>,    // 标记 → 自动注入
    threshold: u32,               // 未标记 → Default::default()
}

// 无需手写构造函数，无需手动注册 —— 属性宏一步到位
let provider = ServiceCollection::from_injected().build()?;
```

**适用**：依赖都是容器可直接解析的服务，字段可 `Default` 或可注入。这是**推荐的首选方式**——零样板、声明式、类型安全。

> 未标记字段（如 `threshold: u32`）自动走 `Default::default()`，保持结构体相对干净。若该字段需要注入而非默认值，加 `#[inject]`（`Arc<T>`）或 `#[inject(owned)]`（裸 `T`）。

### 方式 C：闭包工厂

手动写闭包，适合已有实例或需要运行时参数：

```rust
use rust_dix::*;
use std::sync::Arc;

let existing_db = Arc::new(DbPool::connect("..."));
let provider = ServiceCollection::new()
    .instance(existing_db)                          // 已有 Arc 实例
    .singleton_value(Config { port: 8080 })         // 自动 Arc 包装
    .build()?;
```

**适用**：已有 `Arc<T>` 实例、配置值、需要运行时参数构造。

### 决策树

```
依赖字段能否被容器直接解析？
├─ 能（都是注册的服务）→ 用方式 B：#[inject] 字段标记（推荐）
└─ 不能（需计算 / 已有实例 / 运行时参数）
   ├─ 初始化逻辑复杂 → 用方式 A：手写构造函数
   └─ 简单已有实例 → 用方式 C：闭包工厂 / instance / singleton_value
```

---

## 4. 生命周期选择

参见 [第八章索引 · 生命周期选择](INDEX.md#生命周期选择)：

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池、缓存 | Singleton |
| 请求上下文、事务、用户会话 | Scoped |
| 无状态轻量服务、DTO | Transient |
| **禁止** Singleton 依赖 Scoped | build 阶段检测并拒绝 |

> Owned 语义：需要独占修改（`&mut self`）的服务用 `#[inject(owned)]` 注入裸 `T`，每次解析新建实例，Singleton 不可 owned。
> Owned 解析是严格 scope 校验的**例外**：`get_owned`/`try_get_owned`（含 keyed owned）
> 允许从 root 解析 Scoped（实例归调用方、不进缓存/不被跟踪）；但该服务构造函数内部
> 解析的 Scoped 依赖仍受校验——有依赖时仍需在 `provider.scope()` 内取。

---

## 5. 字段注入标记速查

仅在采用方式 B（`#[inject]` 字段标记）时适用：

| 标记 | 字段类型 | 行为 | 未注册时 |
|------|---------|------|---------|
| *(无标记)* | 任意（需 `Default`） | `Default::default()` | N/A |
| `#[inject]` | `Arc<T>` | 共享注入 | `Err(ServiceNotFound)`（含字段 `context`） |
| `#[inject]` | `Option<Arc<T>>` | 可选共享 | `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控共享 | `Err(KeyedServiceNotFound)` |
| `#[inject(owned)]` | 裸 `T` | owned 注入 | `Err`（未注册或 Singleton） |
| `#[inject(owned)]` | `Option<T>` | 可选 owned | `None` |
| `#[inject(provider)]` | `Arc<ServiceProvider>` | 注入容器自身（按需解析） | N/A（内建） |
| `#[inject]` | `Arc<dyn ScopeFactory>` | 内建 scope 工厂（每任务/每 tick 独立 scope） | N/A（内建） |
| `#[inject]` | `Arc<ServiceProvider>` | 内建容器自身（普通写法） | N/A（内建） |

> 严格类型校验：`#[inject]` 接受 `Arc<T>` / `Option<Arc<T>>` / `Vec<Arc<T>>`，`#[inject(owned)]` 只接受裸 `T`/`Option<T>`，不匹配触发编译错误。可选性由 `Option<...>` 类型决定，无需额外标记。
> **`Vec<Arc<T>>` 枚举与 async**：含**已初始化** async singleton；同步枚举**未初始化** async singleton/transient 返回 `Err(RdiError::AsyncServiceRequiresGetAsync)`（注入失败）；async scoped 在 root 枚举会被严格 scope 校验省略（枚举本身即 Err 的路径以 `get_all_any_result` 为准）；async scoped/transient 请用 `get_async` 逐项解析。
> **`Option` 注入不抑制 scope 校验**：`#[inject] Option<Arc<Scoped>>` 在 strict root provider 下**不会**变成 `None`，而是以 typed
> `Err(RdiError::ScopedFromRootProvider)` 构造失败（容器方法 `provider.get_optional::<Scoped>()` 才返回 `None`）。
> 可选 Scoped 依赖必须在 `provider.scope()` 内构造/解析。
> 内建服务（`ServiceProvider` / `dyn ScopeFactory`）无需注册，eager singleton 与懒解析服务均可用；显式注册仍优先。singleton 内优先持有 `Arc<dyn ScopeFactory>`（弱引用单例，无 `Arc` 环），而不是 `Arc<ServiceProvider>`（强引用环）；在构造函数内通过注入的 provider 解析服务会被拒绝，请在方法中延迟解析。

---

## 6. 代码简洁性技巧

以下是让 DI 代码更简洁、可读性更高的实用技巧：

1. **`use rust_dix::*` 统一引入** —— 消除所有 `rust_dix::` 前缀，示例见 §1。
2. **`#[inject]` 属性宏零样板** —— 一步完成构造 + 注册，对比手动 `ServiceCollection` 逐个 `.singleton()` 大幅减少样板。
3. **impl 块 trait 自动推断** —— `#[inject] impl Trait for T` 自动注册为 `dyn Trait`，无需 `as = dyn Trait`。
4. **`singleton_value(T)` 自动 Arc** —— 直接传值，框架自动 `Arc::new`，比 `instance(Arc::new(T))` 更简洁。
5. **`from_injected()` 一行收集** —— 所有 `#[inject]` 标注的注册自动收集，无需逐个 `.singleton(__rdi_construct_X)`。
6. **`Option<Arc<T>>` 可选依赖** —— 类型即语义，未注册返回 `None`，无需额外标记或 `try_get` 样板。
7. **单元结构体直接支持** —— `struct Config;` 零字段服务无需任何特殊处理。
8. **`try_add` 条件注册** —— 已注册则跳过，实现可选功能模块时不覆盖默认注册。
9. **未标记字段自动 `Default`** —— 内部状态字段无需任何属性，结构体保持干净（旧的 `#[rdi(skip)]` 已移除）。
10. **keyed 服务实现策略模式** —— 一个接口多实现，运行时 `get_keyed::<dyn Trait>("key")` 路由。

---

## 7. 完整示例：从冗长到简洁

同一 `OrderService`，三种写法的代码量对比：

> 为聚焦代码量对比，写法 1 / 2 的闭包体内省略了依赖解析细节；真实闭包内不能调用
> `p.get::<T>()`，请按 §3 的说明改用生成的构造函数、`get_any` + downcast，或在闭包外
> 解析好依赖后以 `Arc` 捕获。

### 写法 1：全限定路径 + 手动闭包（最冗长，不推荐）

```rust
use std::sync::Arc;

pub struct OrderService {
    repo: Arc<dyn IOrderRepo>,
    logger: Arc<dyn ILogger>,
}

impl OrderService {
    pub fn new(repo: Arc<dyn IOrderRepo>, logger: Arc<dyn ILogger>) -> Self {
        Self { repo, logger }
    }
}

fn build() -> Result<rust_dix::ServiceProvider, rust_dix::RdiError> {
    rust_dix::ServiceCollection::new()
        .singleton(|p| Arc::new(OrderService::new(
            p.get::<dyn IOrderRepo>().unwrap(),
            p.get::<dyn ILogger>().unwrap(),
        )))
        .singleton::<dyn IOrderRepo>(|_| Arc::new(PgOrderRepo))
        .singleton::<dyn ILogger>(|_| Arc::new(ConsoleLogger))
        .build()
}
```

### 写法 2：`use *` + 手动闭包（简洁，方式 A）

```rust
use rust_dix::*;
use std::sync::Arc;

pub struct OrderService {            // 结构体干净，零 DI 属性
    repo: Arc<dyn IOrderRepo>,
    logger: Arc<dyn ILogger>,
}

impl OrderService {
    pub fn new(repo: Arc<dyn IOrderRepo>, logger: Arc<dyn ILogger>) -> Self {
        Self { repo, logger }
    }
}

fn build() -> Result<ServiceProvider, RdiError> {
    ServiceCollection::new()
        .singleton(|p| Arc::new(OrderService::new(p.get().unwrap(), p.get().unwrap())))
        .singleton::<dyn IOrderRepo>(|_| Arc::new(PgOrderRepo))
        .singleton::<dyn ILogger>(|_| Arc::new(ConsoleLogger))
        .build()
}
```

### 写法 3：`#[inject]` 属性宏（最简洁，推荐，方式 B）

```rust
use rust_dix::*;
use std::sync::Arc;

#[inject(singleton)]
pub struct OrderService {
    #[inject] repo: Arc<dyn IOrderRepo>,
    #[inject] logger: Arc<dyn ILogger>,
}

#[inject] impl IOrderRepo for PgOrderRepo { /* ... */ }
#[inject] impl ILogger for ConsoleLogger { /* ... */ }

fn build() -> Result<ServiceProvider, RdiError> {
    ServiceCollection::from_injected().build()
}
```

**对比**：写法 3 比写法 1 减少约 60% 代码量，无样板、无手动闭包、无全限定路径，且类型安全由宏在编译期保证。

---

## 8. 0.9.x 语义要点（必读）

0.8.0 起容器语义对齐 MEDI；**0.9.0** 新增类型化键 / `AnyKey` 兜底 / `AsyncFactoryContext` /
精确诊断，并把 async 环检测升级为解析链。以下要点直接影响本文的注册与解析建议：

| 主题 | 0.9.x 行为 | 对本指南的影响 |
|------|-----------|----------------|
| 单服务解析 | **最后注册优先**（`get` / `get_keyed` / `get_owned` / `get_async`） | 覆盖 mock / 默认实现：后注册的生效；要"未注册才添加"用 `try_add*` |
| 多实现收集 | `get_all` / `#[inject] Vec<Arc<T>>` **仅非 keyed**，按注册顺序；含**已初始化** async singleton，同步枚举未初始化 async singleton/transient → `AsyncServiceRequiresGetAsync`（`get_all` typed panic，`Vec` 注入 `Err`；async scoped 在 root 被 scope 校验省略） | keyed 策略实现用 `get_all_keyed`；观察者广播用非 keyed 注册；async scoped/transient 用 `get_async` 逐项解析 |
| 类型化键 / AnyKey | keyed API 接受 `impl Into<ServiceKey>`；`keyed_any_*` 兜底（精确 key 优先、按 (服务注册/类型, 请求 key) 缓存、枚举不含兜底、单值 any_key 非法）；`ServiceKey::new("x")`（&str）≠ 字符串注册的 `String` key | 语义稳定的 key 用 `ServiceKey::new(enum)`；需要"任意 key"时用 `keyed_any_*` 而非手写回退；字符串键用 `"x".into()` |
| Scope 校验 | 默认严格：root 解析 Scoped 返回 `Err(ScopedFromRootProvider)`（`get_owned`/`try_get_owned` 例外：owned 不缓存，可从 root 取） | 请求级服务在 `provider.scope()` 内解析；不要再依赖"根 Provider 即 root scope" |
| 旧行为兼容 | `build_with(ServiceProviderOptions::lenient())` | 仅确需 root 级 Scoped 单例化时使用 |
| 释放契约 | `Scope::dispose()` / Drop 按创建逆序释放；新增 `ServiceProvider::dispose()`；之后解析返回 `ScopeDisposed` / `ProviderDisposed` | 应用关闭时显式 `provider.dispose()`；请求级 scope 显式 dispose 或依赖 Drop |
| async singleton | 依赖驱动递归初始化、**注册顺序无关**；有环返回 `CircularDependency`（不 hang） | 异步基础设施按依赖关系注册即可；确保依赖图无环 |
| async 解析/诊断 | 同步解析 async 服务报 `AsyncServiceRequiresGetAsync`；同步 singleton 不能依赖 async 服务；跨任务传 `AsyncFactoryContext` | 用 `get_async` 解析 async 服务；跨任务环检测必须传 ctx |
| 构建顺序 | **同步 singleton 按注册顺序创建**；async singleton 按依赖优先拓扑序，两者均确定 | 不再需要为"HashMap 顺序"做防御性设计 |
| 工程基线 | MSRV **1.95**、`#![forbid(unsafe_code)]`、CI matrix + RustSec `cargo audit`、`SECURITY.md` | CI/发布环境按 1.95 配置；安全策略见 `SECURITY.md` |

```rust
use rust_dix::*;

// 覆盖默认实现（后注册生效）；需要"未注册才添加"时改 try_add_singleton
let provider = ServiceCollection::new()
    .singleton::<dyn ILogger>(|_| Arc::new(ConsoleLogger))     // 默认
    .singleton::<dyn ILogger>(|_| Arc::new(StructuredLogger))  // 生效（最后注册）
    .build()
    .unwrap();

// 严格 scope：Scoped 服务在 scope 内解析
let provider = ServiceCollection::new()
    .scoped::<dyn IUnitOfWork>(|_| Arc::new(UnitOfWork::new()))
    .build()
    .unwrap();
let scope = provider.scope();
let uow: Arc<dyn IUnitOfWork> = scope.get().unwrap();
scope.dispose(); // 逆序释放；之后解析返回 Err(ScopeDisposed)
```

> 逐条对照（含 MEDI 默认值差异与迁移动作）见
> [与 MEDI 的差异清单](../01-introduction/medi-differences.md)。

---

## 下一步

- [常见陷阱与排查](common-pitfalls.md) —— 高频错误规避
- [反模式警示](anti-patterns.md) —— 错误模式与修正
- [面向接口开发清单](interface-checklist.md) —— 核心规则速查
