<p align="center">
  <img src="../assets/logo.svg" alt="LRDI" width="80">
</p>

<h1 align="center">lrdi-macros · 编译期依赖注入代码生成</h1>

<p align="center">
  <a href="https://crates.io/crates/lrdi-macros"><img alt="Crates.io" src="https://img.shields.io/crates/v/lrdi-macros.svg?style=flat-square"></a>
  <a href="https://docs.rs/lrdi-macros"><img alt="Docs" src="https://img.shields.io/docsrs/lrdi-macros?style=flat-square"></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
</p>

`lrdi-macros` 为 [`lrdi`](../lrdi/) DI 框架提供过程宏。
这些宏将工作从运行时移到编译期，消除样板代码，让你在类型定义处就近声明 DI 配置。

```toml
[dependencies]
lrdi = "0.1"       // 所有宏都被重新导出——通常不需要直接依赖 lrdi-macros
```

> **注意**：你很少需要直接添加 `lrdi-macros` 依赖。`lrdi` crate 已经重新导出了
> 所有三个宏（`Inject`、`module`、`inject`）。`use lrdi::*` 即可使用。

---

### 宏

#### `#[derive(Inject)]` — 自动生成构造函数

**功能**：读取结构体字段及其 `#[inject(...)]` 属性，生成一个工厂函数
`__lrdi_construct_{类型名}()`，自动从容器中解析每个字段。

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
use lrdi_macros::Inject;

#[derive(Inject)]
struct MyService {
    logger: Arc<Logger>,
    #[inject(key = "main")]
    cache: Arc<Cache>,
}
// 生成: __lrdi_construct_MyService(resolver) -> Arc<MyService>
```

生成的函数可以像工厂闭包一样使用：

```rust
let provider = ServiceCollection::new()
    .singleton(__lrdi_construct_MyService)
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

**什么时候用**：

- 你有很多服务结构体，手动拼接依赖重复且繁琐
- 你希望 DI 配置与结构体定义放在一起
- 你希望编译期验证所有字段都可注入

---

#### `#[lrdi::module]` — 编译期模块扫描

**功能**：在编译期扫描 `mod` 块内的 `lrdi::inject!()` 声明，
自动生成 `__lrdi_build_provider_{模块名}()` 函数，构建一个完整配置的 `ServiceProvider`。

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
#[lrdi::module]
mod services {
    lrdi::inject!(singleton: MyService);
    lrdi::inject!(singleton: dyn IPlugin => MyPlugin);
    lrdi::inject!(keyed "verbose": singleton: Logger);
}

// 生成: services::__lrdi_build_provider_services() -> Result<Arc<ServiceProvider>>
let provider = services::__lrdi_build_provider_services().unwrap();
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

#### `lrdi::inject!` — 声明宏

这是在 `#[lrdi::module]` 块内部使用的声明宏，用于声明服务注册。
它在项级别展开为空——其唯一作用是被外层的 `#[lrdi::module]` 属性收集。

也可以单独使用：

```rust
lrdi::inject!(singleton: MyService);
```

---

### 与 lrdi 的关系

`lrdi-macros` 是 [`lrdi`](../lrdi/) 的伴侣 crate。你可以完全不使用宏，
手动写工厂闭包，一切仍然正常工作。宏的存在是为了在服务数量多的时候消除重复代码，
或者让你用声明式的方式配置 DI。

| 方式 | 优点 | 缺点 |
|------|------|------|
| 手动工厂闭包 | 完全可控，显式 | 服务多时样板代码多 |
| `#[derive(Inject)]` | 构造器零样板 | 仅支持命名结构体字段 |
| `#[lrdi::module]` | 声明式，就近配置 | 动态注册灵活性较差 |

三种方式可以在同一个项目中自由混用。

---

### 许可

MIT。
