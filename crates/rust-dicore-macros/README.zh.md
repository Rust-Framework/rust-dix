<p align="center">
  <img src="../assets/logo.svg" alt="rust-dicore" width="80">
</p>

<h1 align="center">rust-dicore-macros · 编译期依赖注入代码生成</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dicore-macros"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dicore-macros.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dicore-macros"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dicore-macros?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
</p>

`rust-dicore-macros` 为 [`rust-dicore`](../rust-dicore/) DI 框架提供过程宏。
这些宏将工作从运行时移到编译期，消除样板代码，让你在类型定义处就近声明 DI 配置。

```toml
[dependencies]
rust-dicore = "0.1"       # 所有宏都被重新导出——通常不需要直接依赖 rust-dicore-macros
```

> **注意**：你很少需要直接添加 `rust-dicore-macros` 依赖。`rust-dicore` crate 已经重新导出了
> 所有三个宏（`Inject`、`module`、`inject`）。`use rust_dicore::*` 即可使用。

---

### 宏

#### `#[rust_dicore::inject(...)]` — 基于属性宏的自动注册（推荐）

**功能**：在一个属性内同时完成构造函数生成和服务注册。标注生命周期和可选的
接口绑定，宏即可生成构造函数和 `inventory` 注册条目。
`ServiceCollection::from_injected()` 在启动时自动收集所有条目。

```rust
use rust_dicore::*;

// 具体类型，Singleton
#[rust_dicore::inject(singleton)]
struct Config;

// 接口绑定，Transient
#[rust_dicore::inject(transient, as = dyn UserRepo)]
struct PgUserRepo { db: Arc<DbPool> }

// 多接口绑定
#[rust_dicore::inject(singleton, as = [dyn EventHandler, dyn StartupTask])]
struct BootLoader;

// 一步收集并构建
let provider = ServiceCollection::from_injected().build().unwrap();
```

**支持的语法**：

| 属性 | 注册方式 |
|-----------|-------------|
| `#[rust_dicore::inject(singleton)]` | 具体类型，Singleton |
| `#[rust_dicore::inject(transient)]` | 具体类型，Transient |
| `#[rust_dicore::inject(scoped)]` | 具体类型，Scoped |
| `#[rust_dicore::inject(transient, as = dyn Trait)]` | `dyn Trait`，Transient |
| `#[rust_dicore::inject(singleton, as = [dyn A, dyn B])]` | 多 trait，Singleton |

同时支持命名结构体和单元结构体（零字段）：

```rust
#[rust_dicore::inject(singleton)]
struct UnitService;  // 单元结构体 — 支持！

#[rust_dicore::inject(transient)]
struct NamedService {
    dep: Arc<UnitService>,
}
```

该宏包含 `#[derive(Inject)]` 的功能——内部生成相同的构造函数
（`__rdi_construct_{类型}`），使用相同的字段属性语法
（`#[inject(skip)]`、`#[inject(optional)]`、`#[inject(key = "...")]` 等），
**无需同时使用两者**。

---

#### `#[derive(Inject)]` — 自动生成构造函数

**功能**：读取结构体字段及其 `#[inject(...)]` 属性，生成一个工厂函数
`__rdi_construct_{类型名}()`，自动从容器中解析每个字段。

> **注意**：此派生宏已被 `#[rust_dicore::inject(...)]` 包含。仅当你需要生成
> 构造函数但不想自动注册时才使用 `#[derive(Inject)]`。

**不使用宏**，需要手动写工厂闭包：

```rust
let provider = ServiceCollection::new()
    .singleton(|p| {
        Arc::new(MyService::new(
            p.get::<Logger>(),
            p.get_keyed::<Cache>("main"),
        ))
    })
    .build().unwrap();
```

**使用宏**，自动生成完全相同的代码：

```rust
#[derive(Inject)]
struct MyService {
    logger: Arc<Logger>,
    #[inject(key = "main")]
    cache: Arc<Cache>,
}
// 生成: __rdi_construct_MyService(resolver) -> Arc<MyService>
```

生成的函数可以像工厂闭包一样使用：

```rust
let provider = ServiceCollection::new()
    .singleton(__rdi_construct_MyService)
    .build().unwrap();
```

**字段属性**：

| 属性 | 字段类型 | 行为 |
|-----------|-----------|------|
| *(无标记)* | `Arc<T>` | 必选 — 未注册则 panic |
| `#[inject(skip)]` | 任意 | 使用 `Default::default()` |
| `#[inject(optional)]` | `Option<Arc<T>>` | 未注册返回 `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控解析，找不到 key 则 panic |
| `#[inject(optional, key = "k")]` | `Option<Arc<T>>` | 可选键控解析 |
| `#[inject(provider)]` | `Arc<dyn IServiceResolver>` | 注入容器自身 |

---

#### `#[rust_dicore::module]` — 编译期模块扫描

**功能**：在编译期扫描 `mod` 块内的 `rust_dicore::inject!()` 声明，
自动生成 `__rdi_build_provider_{模块名}()` 函数，构建一个完整配置的 `ServiceProvider`。

**不使用宏**，需要在一个中心函数中逐个注册：

```rust
fn build_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MyService::default()))
        .singleton(|_| Arc::new(MyPlugin::default()))
        .keyed("verbose", |_| Arc::new(Logger::default()))
        .build().unwrap()
}
```

**使用宏**，直接在模块内部声明注册：

```rust
#[rust_dicore::module]
mod services {
    rust_dicore::inject!(singleton: MyService);
    rust_dicore::inject!(singleton: dyn IPlugin => MyPlugin);
    rust_dicore::inject!(keyed "verbose": singleton: Logger);
}

// 生成: services::__rdi_build_provider_services() -> Result<Arc<ServiceProvider>>
let provider = services::__rdi_build_provider_services().unwrap();
```

**支持的声明**：

| 语法 | 含义 |
|------|------|
| `inject!(singleton: T)` | 用 `Default` 注册 `T` 为 Singleton |
| `inject!(scoped: T)` | 用 `Default` 注册 `T` 为 Scoped |
| `inject!(transient: T)` | 用 `Default` 注册 `T` 为 Transient |
| `inject!(singleton: dyn Trait => Impl)` | 注册 `Impl` 为 Singleton 实现 `Trait` |
| `inject!(keyed "k": singleton: T)` | 注册 `T` 为键控 Singleton |
| `inject!(keyed "k": scoped: T)` | 注册 `T` 为键控 Scoped |
| `inject!(factory singleton: T => expr)` | 使用自定义工厂表达式注册 |

**什么时候用**：

- 你希望服务注册代码与服务模块放在一起
- 你在构建库或插件，希望提供预配置的 provider 构建器
- 你偏好声明式 DSL 胜于方法链

---

#### `rust_dicore::inject!` — 声明宏

这是在 `#[rust_dicore::module]` 块内部使用的声明宏，用于声明服务注册。
它在项级别展开为空——其唯一作用是被外层的 `#[rust_dicore::module]` 属性收集。

也可以单独使用：

```rust
rust_dicore::inject!(singleton: MyService);
```

---

### 与 rust-dicore 的关系

`rust-dicore-macros` 是 [`rust-dicore`](../rust-dicore/) 的伴侣 crate。你可以完全不使用宏，
手动写工厂闭包，一切仍然正常工作。宏的存在是为了在服务数量多的时候消除重复代码，
或者让你用声明式的方式配置 DI。

| 方式 | 优点 | 缺点 |
|------|------|------|
| `#[rust_dicore::inject]`（推荐） | 零样板，自动注册 | 需要 `inventory`；`String` 字段需 `#[inject(skip)]` |
| 手动工厂闭包 | 完全可控，显式 | 服务多时样板代码多 |
| `#[derive(Inject)]` | 构造器零样板 | 仍需手动注册 |
| `#[rust_dicore::module]` | 声明式，集中管理 | 动态注册灵活性较差 |

所有方式可以在同一个项目中自由混用。

---

### 许可

MIT。
