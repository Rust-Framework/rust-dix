# dyn Trait 注册与解析

## 核心规则：注册类型 = 解析类型

rust-dicore 使用 Rust 的 `TypeId` 作为服务标识。注册时的类型必须与解析时的类型一致。

### 正确示例

```rust
use rust_dicore::*;
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
let logger: Arc<dyn ILogger> = provider.get().unwrap();  // panic: TypeId 不匹配
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

### 必需解析（panic）

```rust
// get() / get_required_service()
let logger: Arc<dyn ILogger> = provider.get().unwrap();
// 未注册时 panic: "service not registered: dyn ILogger"
```

### 可选解析

```rust
// get_optional() / get_service()
let logger: Option<Arc<dyn ILogger>> = provider.get_optional();
if let Some(log) = logger {
    log.log("hello");
}
```

### 键控解析

```rust
// get_keyed()
let wechat: Arc<dyn IPayGateway> = provider.get_keyed("wechat").unwrap();

// try_get_keyed()
let maybe: Option<Arc<dyn IPayGateway>> = provider.try_get_keyed("unknown");
```

### 批量解析

```rust
// get_all() / get_services()
let all: Vec<Arc<dyn ILogger>> = provider.get_all();
for logger in &all {
    logger.log("broadcast");
}
```

## IServiceResolver trait

`IServiceResolver` 是核心解析 trait，由 `ServiceProvider`、`Scope`、`ServiceProviderWrapper` 实现：

```rust
pub trait IServiceResolver: Send + Sync {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    
    // 默认实现的方法
    fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Arc<T;
    fn try_get<T>(&self) -> Option<Arc<T>>;
    fn get_keyed<T>(&self, variant: &str) -> Arc<T>;
    fn try_get_keyed<T>(&self, variant: &str) -> Option<Arc<T>>;
}
```

## 工厂闭包中的依赖解析

工厂闭包接收 `&dyn IServiceResolver` 参数，可解析依赖：

```rust
ServiceCollection::new()
    .singleton::<dyn IConfig>(|_| Arc::new(AppConfig))
    .singleton::<dyn ILogger>(|r| {
        let config: Arc<dyn IConfig> = r.get().unwrap();  // 解析依赖
        Arc::new(FileLogger { level: config.level() })
    })
    .build()
    .unwrap();
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
| `#[inject]` | `Arc<T>` | 必选共享 — 未注册则 panic |
| `#[inject]` | `Option<Arc<T>>` | 可选共享 — 未注册返回 `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控共享，找不到 key 则 panic |
| `#[inject(key = "k")]` | `Option<Arc<T>>` | 可选键控共享 |
| `#[inject(owned)]` | 裸 `T` | 必选 owned — 未注册或 Singleton 则 panic |
| `#[inject(owned)]` | `Option<T>` | 可选 owned — 未注册或 Singleton 返回 `None` |
| `#[inject(owned, key = "k")]` | 裸 `T` / `Option<T>` | 键控 owned |
| `#[inject(provider)]` | `Arc<dyn IServiceResolver>` | 注入 resolver 自身 |

> 严格类型校验：`#[inject]` 只接受 `Arc<T>` / `Option<Arc<T>>`，
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
    resolver: Arc<dyn IServiceResolver>,       // 注入解析器
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