<p align="center">
  <img src="../assets/logo.svg" alt="rust-dix" width="80">
</p>

<h1 align="center">rust-dix-macros · 编译期依赖注入代码生成</h1>

<p align="center">
  <a href="https://crates.io/crates/rust-dix-macros"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-dix-macros.svg?style=flat-square"></a>
  <a href="https://docs.rs/rust-dix-macros"><img alt="Docs" src="https://img.shields.io/docsrs/rust-dix-macros?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/Rust-Framework/rust-dix/blob/main/crates/macros/README.md"><strong>English</strong></a>
</p>

`rust-dix-macros` 为 [`rust-dix`](../core/) DI 框架提供过程宏。
这些宏将工作从运行时移到编译期，消除样板代码，让你在类型定义处就近声明 DI 配置。

```toml
[dependencies]
rust-dix = "0.7"       # 所有宏都被重新导出——通常不需要直接依赖 rust-dix-macros
```

> **注意**：你很少需要直接添加 `rust-dix-macros` 依赖。`rust-dix` crate 已经重新导出了
> 所有宏（`Inject`、`module`、`inject`、`register`）。`use rust_dix::*` 即可使用。

---

### 宏

#### `#[rust_dix::inject]` — 基于属性宏的自动注册（推荐）

**功能**：在一个属性内同时完成构造函数生成和服务注册。该属性可以放在
**结构体或 trait impl 块**上，带一个可选的生命周期参数，缺省为 `singleton`。
`ServiceCollection::from_injected()` 在启动时自动收集所有注册。

**放置位置 1 — 在 struct 上**（注册为具体类型）：

```rust
use rust_dix::*;

#[rust_dix::inject]          // 默认 singleton
struct Config;

#[rust_dix::inject(transient)]
struct Worker { dep: Arc<Config> }
```

**放置位置 2 — 在 trait impl 上**（注册为 `dyn Trait`，自动推断）：

```rust
trait UserRepo { /* ... */ }

#[rust_dix::inject(transient)]   // 先在 struct 上
struct PgUserRepo { db: Arc<DbPool> }

#[rust_dix::inject]              // 再在 impl 上 — 注册为 `dyn UserRepo`
impl UserRepo for PgUserRepo { /* ... */ }
```

trait 类型直接从 `impl Trait for Type` 中读取，因此**无需**手写 `as = dyn Trait` ——
宏会自动推断。impl 块形式复用 struct 级标注（或显式 `#[derive(Inject)]`）生成的构造函数，
所以同一个类型可以注册到任意多个 trait 接口下：

```rust
#[rust_dix::inject]
struct BootLoader;

#[rust_dix::inject] impl EventHandler for BootLoader { /* ... */ }
#[rust_dix::inject] impl StartupTask   for BootLoader { /* ... */ }
```

**支持的语法**：

| 属性 | 放置位置 | 注册方式 |
|-----------|-----------|----------|
| `#[rust_dix::inject]` | struct | 具体类型，Singleton（默认） |
| `#[rust_dix::inject(singleton)]` | struct | 具体类型，Singleton |
| `#[rust_dix::inject(scoped)]` | struct | 具体类型，Scoped |
| `#[rust_dix::inject(transient)]` | struct | 具体类型，Transient |
| `#[rust_dix::inject]` | `impl Trait for T` | `dyn Trait`，Singleton（默认） |
| `#[rust_dix::inject(scoped)]` | `impl Trait for T` | `dyn Trait`，Scoped |
| `#[rust_dix::inject(transient)]` | `impl Trait for T` | `dyn Trait`，Transient |

同时支持命名结构体和单元结构体（零字段）：

```rust
#[rust_dix::inject]
struct UnitService;  // 单元结构体 — 支持！

#[rust_dix::inject(transient)]
struct NamedService {
    dep: Arc<UnitService>,
}
```

该宏包含 `#[derive(Inject)]` 的功能——放在 struct 上时，内部生成相同的构造函数
（`__rdi_construct_{类型}`），使用相同的字段属性语法
（`#[inject]`、`#[inject(owned)]`、`#[inject(key = "...")]` 等），
**无需同时使用两者**。impl 块放置方式**不会**生成构造函数——它复用 struct 放置方式已经生成的构造函数。

> **迁移说明**：旧的冗长语法 `#[rust_dix::inject_attr(singleton, as = dyn Trait<...>)]`
> 已移除。请改用 impl 块放置方式。

---

#### `#[derive(Inject)]` — 自动生成构造函数

**功能**：读取结构体字段及其 `#[inject(...)]` 属性，生成一个工厂函数
`__rdi_construct_{类型名}()`，自动从容器中解析每个**标记的**字段。
未标记字段使用 `Default::default()` 作为内部状态。

> **注意**：此派生宏已被 struct 上的 `#[rust_dix::inject]` 包含。仅当你需要生成
> 构造函数但不想自动注册时（例如手动用自定义工厂闭包注册），才使用 `#[derive(Inject)]`。

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
    #[inject]
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

**字段属性**（`#[inject(...)]`）：

显式注入策略：只有标记的字段才会从容器解析，未标记字段使用 `Default::default()`
作为内部状态。可选性由字段类型（`Option<...>`）决定，无需额外标记。

| 属性 | 字段类型 | 行为 |
|-----------|-----------|------|
| *(无标记)* | 任意（需 `Default`） | 内部字段 — 使用 `Default::default()` |
| `#[inject]` | `Arc<T>` | 必选共享 — 未注册返回 `Err(ServiceNotFound)`（try-construct 带字段 `context`） |
| `#[inject]` | `Option<Arc<T>>` | 可选共享 — 未注册返回 `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控共享 — 找不到 key 返回 `Err(KeyedServiceNotFound)` |
| `#[inject(key = "k")]` | `Option<Arc<T>>` | 可选键控共享 |
| `#[inject(owned)]` | 裸 `T` | 必选 owned — 未注册或 Singleton 返回 `Err` |
| `#[inject(owned)]` | `Option<T>` | 可选 owned — 未注册或 Singleton 返回 `None` |
| `#[inject(owned, key = "k")]` | 裸 `T` / `Option<T>` | 键控 owned |
| `#[inject]` | `Vec<Arc<T>>` | 多态注入 — 收集所有注册实现（default + keyed）；不支持 key |
| `#[inject(provider)]` | `Arc<ServiceProvider>` | 注入 provider 供按需解析 |

> 严格类型校验：`#[inject]` 接受 `Arc<T>`、`Option<Arc<T>>` 或 `Vec<Arc<T>>`，
> `#[inject(owned)]` 只接受裸 `T` / `Option<T>`。标记与类型不匹配会触发编译错误。

---

#### `#[rust_dix::module]` — 编译期模块扫描

**功能**：在编译期扫描 `mod` 块内的 `rust_dix::register!()` 声明，
自动生成 `__rdi_build_provider_{模块名}()` 函数，构建一个完整配置的 `ServiceProvider`。

**不使用宏**，需要在一个中心函数中逐个注册：

```rust
fn build_provider() -> ServiceProvider {
    ServiceCollection::new()
        .singleton(|_| Arc::new(MyService::default()))
        .singleton(|_| Arc::new(MyPlugin::default()))
        .keyed_singleton("verbose", |_| Arc::new(Logger::default()))
        .build().unwrap()
}
```

**使用宏**，直接在模块内部声明注册：

```rust
#[rust_dix::module]
mod services {
    rust_dix::register!(singleton: MyService);
    rust_dix::register!(singleton: dyn IPlugin => MyPlugin);
    rust_dix::register!(keyed "verbose": singleton: Logger);
}

// 生成: services::__rdi_build_provider_services() -> Result<Arc<ServiceProvider>>
let provider = services::__rdi_build_provider_services().unwrap();
```

**支持的声明**：

| 语法 | 含义 |
|------|------|
| `register!(singleton: T)` | 用 `Default` 注册 `T` 为 Singleton |
| `register!(scoped: T)` | 用 `Default` 注册 `T` 为 Scoped |
| `register!(transient: T)` | 用 `Default` 注册 `T` 为 Transient |
| `register!(singleton: dyn Trait => Impl)` | 注册 `Impl` 为 Singleton 实现 `Trait` |
| `register!(keyed "k": singleton: T)` | 注册 `T` 为键控 Singleton |
| `register!(keyed "k": scoped: T)` | 注册 `T` 为键控 Scoped |
| `register!(factory singleton: T => expr)` | 使用自定义工厂表达式注册 |

**什么时候用**：

- 你希望服务注册代码与服务模块放在一起
- 你在构建库或插件，希望提供预配置的 provider 构建器
- 你偏好声明式 DSL 胜于方法链

---

#### `rust_dix::register!` — 声明宏

这是在 `#[rust_dix::module]` 块内部使用的声明宏，用于声明服务注册。
它在项级别展开为空——其唯一作用是被外层的 `#[rust_dix::module]` 属性收集。

也可以单独使用：

```rust
rust_dix::register!(singleton: MyService);
```

> **迁移说明**：函数式宏从 `inject!()` 改名为 `register!()`，以规避过程宏命名空间冲突
> （Rust 不允许同一 crate 中属性宏和函数式宏同名）。这释放了 `inject` 这个名字给前面
> 描述的属性宏使用。

---

### 与 rust-dix 的关系

`rust-dix-macros` 是 [`rust-dix`](../core/) 的伴侣 crate。你可以完全不使用宏，
手动写工厂闭包，一切仍然正常工作。宏的存在是为了在服务数量多的时候消除重复代码，
或者让你用声明式的方式配置 DI。

| 方式 | 优点 | 缺点 |
|------|------|------|
| `#[rust_dix::inject]`（推荐） | 零样板，自动注册，impl 块 trait 推断 | 需要 `inventory`；未标记字段需 `Default` |
| 手动工厂闭包 | 完全可控，显式 | 服务多时样板代码多 |
| `#[derive(Inject)]` | 构造器零样板 | 仍需手动注册 |
| `#[rust_dix::module]` | 声明式，集中管理 | 动态注册灵活性较差 |

所有方式可以在同一个项目中自由混用。

---

### 许可

MIT。
