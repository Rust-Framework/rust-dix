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
let logger: Arc<dyn ILogger> = provider.get();
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
let logger: Arc<dyn ILogger> = provider.get();  // panic: TypeId 不匹配
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
    .keyed::<dyn Trait>("name", |_| Arc::new(Impl))
    .build()
    .unwrap();
```

### 过程宏自动注册

```rust
// #[rust_dicore::inject] 属性宏
#[rust_dicore::inject(singleton, as = dyn ILogger)]
struct FileLogger;

impl ILogger for FileLogger {
    fn log(&self, msg: &str) { println!("[FILE] {}", msg); }
}

// 使用 from_injected() 自动收集
let provider = ServiceCollection::from_injected()
    .build()
    .unwrap();

let logger: Arc<dyn ILogger> = provider.get();
```

### 多接口注册

一个实现注册为多个 trait：

```rust
trait IReader: Send + Sync { fn read(&self) -> String; }
trait IWriter: Send + Sync { fn write(&self, data: &str); }

#[rust_dicore::inject(singleton, as = [dyn IReader, dyn IWriter])]
struct FileStream {
    path: String,
}

impl IReader for FileStream { fn read(&self) -> String { "data" } }
impl IWriter for FileStream { fn write(&self, data: &str) { println!("{}", data); } }

// 分别解析
let reader: Arc<dyn IReader> = provider.get();
let writer: Arc<dyn IWriter> = provider.get();
```

## 解析 API

### 必需解析（panic）

```rust
// get() / get_required_service()
let logger: Arc<dyn ILogger> = provider.get();
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
let wechat: Arc<dyn IPayGateway> = provider.get_keyed("wechat");

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
        let config: Arc<dyn IConfig> = r.get();  // 解析依赖
        Arc::new(FileLogger { level: config.level() })
    })
    .build()
    .unwrap();
```

过程宏自动处理字段依赖：

```rust
#[rust_dicore::inject(singleton, as = dyn ILogger)]
struct FileLogger {
    config: Arc<dyn IConfig>,  // 自动解析
    #[inject(optional)]
    fallback: Option<Arc<dyn ILogger>>,
    #[inject(skip)]
    name: String,  // 使用 Default::default()
}
```

## 字段属性详解

| 属性 | 说明 |
|------|------|
| `#[inject(skip)]` | 该字段不解析，使用 `Default::default()` |
| `#[inject(optional)]` | 该字段解析为 `Option<Arc<T>>` |
| `#[inject(provider)]` | 该字段注入 `dyn IServiceResolver` |
| `#[inject(key = "...")]` | 该字段按 key 解析键控服务 |

```rust
#[rust_dicore::inject(transient)]
struct Handler {
    user_svc: Arc<dyn IUserService>,           // 默认解析
    #[inject(optional)]
    logger: Option<Arc<dyn ILogger>>,          // 可选
    #[inject(key = "wechat")]
    pay: Arc<dyn IPayGateway>,                 // 键控
    #[inject(provider)]
    resolver: Arc<dyn IServiceResolver>,       // 注入解析器
    #[inject(skip)]
    name: String,                              // 不解析
}
```

## 最佳实践

| ✅ 推荐 | ❌ 避免 |
|--------|--------|
| `singleton::<dyn Trait>(...)` | `singleton(|_| Arc::new(Impl))` |
| `provider.get::<dyn Trait>()` | `provider.get::<Impl>()` |
| 字段声明 `Arc<dyn Trait>` | 字段声明 `Arc<Impl>` |
| `#[inject(as = dyn Trait)]` | 仅 `#[inject(singleton)]` |

---

下一节：[多实现与键控服务](multiple-implementations.md)