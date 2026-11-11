# dyn Trait 注册与解析

## 核心规则：注册类型 = 解析类型

rust-dix 使用 Rust 的 `TypeId` 作为服务标识。注册时的类型必须与解析时的类型一致。

### 正确示例

```rust
use rust_dix::*;
use std::sync::Arc;

// 定义 trait
trait ILogger: Send + Sync {
    fn log(&self, msg: &str);
}

// 定义实现
struct FileLogger;
impl ILogger for FileLogger {
    fn log(&self, msg: &str) {
        println!("[FILE] {}", msg);
    }
}

// 注册为 dyn Trait
let provider = ServiceCollection::new()
    .singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
    .build()
    .unwrap();

// 解析为 dyn Trait
let logger: Arc<dyn ILogger> = provider.get().unwrap();
logger.log("hello");
```

### 错误示例

```rust
// ❌ 注册为具体类型
let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(FileLogger))  // TypeId = FileLogger
    .build()
    .unwrap();

// ❌ 解析为 trait
let logger: Arc<dyn ILogger> = provider.get().unwrap();  // Err: TypeId 不匹配
```

## 注册语法详解

### 手动注册

```rust
ServiceCollection::new()
    // Singleton
    .singleton::<dyn Trait>(|_| Arc::new(Impl))
    // Scoped
    .scoped::<dyn Trait>(|_| Arc::new(Impl))
    // Transient
    .transient::<dyn Trait>(|_| Arc::new(Impl))
    // Keyed
    .keyed_singleton::<dyn Trait>("name", |_| Arc::new(Impl))
    .build()
    .unwrap();
```

### 过程宏自动注册

`#[inject]` 放置位置二选一：面向接口时 struct 上用 `#[derive(Inject)]`（仅生成构造函数、不注册），impl 上用 `#[inject]` 注册为 `dyn Trait`。**禁止** struct 与 impl 同时用 `#[inject]`（会双重注册，详见 [用法指南](../08-best-practices/usage-guide.md#放置位置struct-还是-impl二选一避免双重注册)）。

```rust
// struct 上用 #[derive(Inject)] 仅生成构造函数（不注册），impl 上用 #[inject] 注册为 dyn ILogger
#[derive(Inject)]
struct FileLogger;

#[inject]
impl ILogger for FileLogger {
    fn log(&self, msg: &str) { println!("[FILE] {}", msg); }
}

// 使用 from_injected() 自动收集
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let logger: Arc<dyn ILogger> = provider.get().unwrap();
```

### 多接口注册

一个实现注册为多个 trait：

```rust
trait IReader: Send + Sync { fn read(&self) -> String; }
trait IWriter: Send + Sync { fn write(&self, data: &str); }

// struct 上用 #[derive(Inject)] 仅生成构造函数（不注册），两个 impl 块各注册一个 trait
#[derive(Inject)]
struct FileStream {
    path: String,
}

#[inject]
impl IReader for FileStream { fn read(&self) -> String { "data" } }
#[inject]
impl IWriter for FileStream { fn write(&self, data: &str) { println!("{}", data); } }

// 分别解析
let reader: Arc<dyn IReader> = provider.get().unwrap();
let writer: Arc<dyn IWriter> = provider.get().unwrap();
```

## 解析 API

### 必需解析（`Result`）

```rust
// get() 返回 Result — 未注册时 Err(ServiceNotFound)，不 panic
let logger: Arc<dyn ILogger> = provider.get()?;
// 或: provider.get().expect("ILogger registered at composition root");
```

### 可选解析

```rust
// try_get()
let logger: Option<Arc<dyn ILogger>> = provider.try_get();
if let Some(log) = logger {
    log.log("hello");
}
```

### 键控解析

```rust
// get_keyed() —— 接受 impl Into<ServiceKey>：字符串或类型化键
let wechat: Arc<dyn IPayGateway> = provider.get_keyed("wechat").unwrap();
let typed: Arc<dyn IPayGateway> = provider.get_keyed(ServiceKey::new(GwKey::Wechat)).unwrap();

// try_get_keyed()
let maybe: Option<Arc<dyn IPayGateway>> = provider.try_get_keyed("unknown");
```

> 精确 key 优先于 `keyed_any_*` 兜底；`ServiceKey::any_key()` 不是合法的单值 key
> （`get_keyed(any_key())` → `Err`）。字符串键用 `"wechat".into()` / `ServiceKey::from("wechat")`；
> `ServiceKey::new("wechat")` 是 `&str` typed key，与字符串注册的 `String` key **不相等**。
> 详见 [多实现与键控服务](multiple-implementations.md#类型化键与-anykey-兜底090)。

### 批量解析

```rust
// get_all() 只返回“非 keyed”注册，按注册顺序（对齐 MEDI GetServices）
// 含已初始化的 async singleton；同步枚举未初始化 async singleton/transient
// 会得到 AsyncServiceRequiresGetAsync（get_all typed panic，Vec 注入 Err；
// async scoped 在 root 被 scope 校验省略、Scope 内枚举才 panic）
let all: Vec<Arc<dyn ILogger>> = provider.get_all();
for logger in &all {
    logger.log("broadcast");
}

// keyed 多实现：用 get_all_keyed() 取该类型的显式 keyed 注册（不含 AnyKey 兜底）
let keyed: Vec<Arc<dyn ILogger>> = provider.get_all_keyed();
```

> 单服务解析（`get` / `get_keyed` 等）为 MEDI 的"最后注册优先"：
> 同一类型（或同一 key）注册多次时，后注册生效。

## IServiceResolver trait

`IServiceResolver` 是核心解析 trait，由 `ServiceProvider`、`Scope`、`ServiceProviderWrapper` 实现：

```rust
pub trait IServiceResolver: Send + Sync {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    // TypeId 直查（容器覆盖为 O(1)；外部实现默认 None → 回退字符串路径）
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    // 类型擦除的批量解析：非 keyed / 显式 keyed（不含 AnyKey 兜底）
    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>>;
    fn get_all_keyed_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>>;
    // 可失败探测：容器覆盖为"不可解析 → None/Err"，不 panic（供 try_get）
    fn try_get_any_result(&self, key: &str) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError>;
    fn try_get_keyed_any_result(&self, key: &str, variant: &str) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError>;

    // 默认实现的方法（均带 Self: Sized，不能在 trait object 上调用）
    fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>;
    fn try_get<T>(&self) -> Option<Arc<T>>;
    fn get_keyed<T>(&self, variant: &str) -> Result<Arc<T>, RdiError>;
    fn try_get_keyed<T>(&self, variant: &str) -> Option<Arc<T>>;
    // … get_owned / get_all / get_all_keyed 等；
    // is_registered 只作为 ServiceProvider / Scope / Wrapper 的固有方法提供
}
```

> **失败语义契约（0.9.x，写自定义 `IServiceResolver` 实现时必读）**：
> - **trait 层 Option 方法**（`get_any` / `get_keyed_any` / `get_by_type_id` /
>   `get_keyed_by_type_id`）签名不可失败：**已注册但当前不可解析**（strict root scope
>   校验、已 dispose、循环依赖、工厂 panic 等）时，必须以 **typed `RdiError` payload
>   panic**；required API（`get` / `get_owned` / `get_async` / `get_keyed*`）会用
>   `catch_unwind` 捕获并转为 `Err`，不要改为静默 `None`。
> - **公开 keyed API 与 trait 层 key 表示**：`ServiceProvider` / `Scope` /
>   `ServiceProviderWrapper` 的 keyed 方法接受 `impl Into<ServiceKey>`（typed keys）；
>   本 trait 的类型擦除方法仍以 `&str` 表示 key（供宏生成代码的字符串路径使用），
>   两者内部映射为同一 `ServiceKey`。
> - **`try_get` / `try_get_keyed`**：容器实现下对"未注册 / strict root scope 校验 /
>   已 dispose / 解析失败"统一返回 `None`，**不 panic**；失败原因需用 `get` 系列区分。
> - **`get_all` / `get_all_keyed`**：签名不可失败；已 dispose 返回空、root scope 校验导致的
>   不可解析被省略，但其它解析错误（如工厂 panic）仍以 typed `RdiError` payload panic 传播。
> - **异步解析诊断**：同步 `get` / `get_optional` 解析 async 服务返回
>   `RdiError::AsyncServiceRequiresGetAsync`（请用 `get_async`）；含 `async_*` 注册的集合
>   调用 `build()` 仍返回 `AsyncServiceRequiresBuildAsync`。
> - 需要"探测而不 panic"的外部实现请覆盖 `try_get_any_result` / `try_get_keyed_any_result`
>   （默认实现会退回 `get_any`，可能继承其 panic 契约）。

## 工厂闭包中的依赖解析

工厂闭包接收 `&dyn IServiceResolver` 参数；该 trait 的泛型 `get` 带 `Self: Sized`
约束，不能直接在 trait object 上调用，推荐用 `#[derive(Inject)]` 生成构造函数，
或通过 `get_any` + downcast：

```rust
ServiceCollection::new()
    .singleton::<dyn IConfig>(|_| Arc::new(AppConfig))
    .singleton::<dyn ILogger>(|r| {
        let any = r
            .get_any(std::any::type_name::<dyn IConfig>())
            .expect("IConfig must be registered");
        let config: Arc<dyn IConfig> =
            Arc::clone(&*any.downcast::<Arc<dyn IConfig>>().expect("IConfig downcast"));
        Arc::new(FileLogger { level: config.level() })
    })
    .build()
    .unwrap();
```

更简洁的方式是使用 `#[derive(Inject)]`，由生成的 `__rdi_construct_*` 完成依赖解析
（该函数是 `#[doc(hidden)]` 内部面，宏用它支撑自动注册；应用代码优先用
`#[inject]` / `from_injected()`，只有自定义工厂闭包才直接调用）：

```rust
#[derive(Inject)]
struct FileLogger {
    #[inject]
    config: Arc<dyn IConfig>,
}
// .singleton::<dyn ILogger>(|r| __rdi_construct_FileLogger(r))
```

过程宏自动处理字段依赖：

```rust
// struct 上用 #[derive(Inject)] 仅生成构造函数（不注册），impl 上用 #[inject] 注册为 dyn ILogger
#[derive(Inject)]
struct FileLogger {
    #[inject]
    config: Arc<dyn IConfig>,  // 自动解析
    #[inject]
    fallback: Option<Arc<dyn ILogger>>,
    name: String,  // 未标记 → 使用 Default::default()
}

#[inject]
impl ILogger for FileLogger { /* ... */ }
```

## 字段属性详解

显式注入策略：只有标记了 `#[inject(...)]` 的字段才从容器解析，未标记字段使用 `Default::default()`。
可选性由字段类型（`Option<...>`）决定，无需额外标记。

| 属性 | 字段类型 | 说明 |
|------|---------|------|
| *(无标记)* | 任意（需 `Default`） | 内部字段 — 使用 `Default::default()` |
| `#[inject]` | `Arc<T>` | 必选共享 — 未注册返回 `Err(ServiceNotFound)` |
| `#[inject]` | `Option<Arc<T>>` | 可选共享 — 未注册返回 `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控共享 — 找不到 key 返回 `Err(KeyedServiceNotFound)` |
| `#[inject(key = "k")]` | `Option<Arc<T>>` | 可选键控共享 |
| `#[inject(owned)]` | 裸 `T` | 必选 owned — 未注册或 Singleton 返回 `Err` |
| `#[inject(owned)]` | `Option<T>` | 可选 owned — 未注册或 Singleton 返回 `None` |
| `#[inject(owned, key = "k")]` | 裸 `T` / `Option<T>` | 键控 owned |
| `#[inject]` | `Vec<Arc<T>>` | 多态注入 — **仅非 keyed** 注册，按注册顺序（0.8.0 起对齐 MEDI `GetServices`）；keyed 用 `get_all_keyed` |
| `#[inject(provider)]` | `Arc<ServiceProvider>` | 注入容器自身（按需解析） |
| `#[inject]` | `Arc<dyn ScopeFactory>` | 内建 scope 工厂 — 每任务/每 tick 建独立 scope |
| `#[inject]` | `Arc<ServiceProvider>` | 内建容器自身（等价普通写法，对齐 MEDI `IServiceProvider`） |

> 内建服务无需注册，且在 `build()`/`build_async()` 期间构建的 eager singleton 中同样
> 可注入（0.7.1+）。内置 `ScopeFactory` 为弱引用单例，singleton 持有它不会形成
> `容器 → singleton → 容器` 的 `Arc` 环；但持有 `Arc<ServiceProvider>` 会形成强引用环，
> singleton 内优先用 `ScopeFactory`。在构造函数内通过注入的 provider 解析服务会被拒绝，
> 请保存句柄并在方法中延迟解析。

> 严格类型校验：`#[inject]` 只接受 `Arc<T>` / `Option<Arc<T>>` / `Vec<Arc<T>>`，
> `#[inject(owned)]` 只接受裸 `T` / `Option<T>`。标记与类型不匹配会触发编译错误。

```rust
#[inject(transient)]
struct Handler {
    #[inject]
    user_svc: Arc<dyn IUserService>,           // 必选共享
    #[inject]
    logger: Option<Arc<dyn ILogger>>,          // 可选共享
    #[inject(key = "wechat")]
    pay: Arc<dyn IPayGateway>,                 // 键控
    #[inject(provider)]
    resolver: Arc<ServiceProvider>,            // 注入容器（按需解析）
    name: String,                              // 未标记 → Default
}
```

## 最佳实践

| ✅ 推荐 | ❌ 避免 |
|--------|--------|
| `singleton::<dyn Trait>(...)` | `singleton(|_| Arc::new(Impl))` |
| `provider.get::<dyn Trait>()` | `provider.get::<Impl>()` |
| 字段声明 `Arc<dyn Trait>` | 字段声明 `Arc<Impl>` |
| 面向接口：struct 用 `#[derive(Inject)]` + impl 用 `#[inject]` | struct 与 impl 同时用 `#[inject]`（双重注册） |
| 仅注册为 trait（`#[inject] impl Trait for Type`） | 仅 `#[inject(singleton)]` 在 struct 上（不注册为 trait） |

---

下一节：[多实现与键控服务](multiple-implementations.md)