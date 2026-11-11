# 参考 · 扩展契约与边界

> **唯一权威的"扩展边界"页**：说明 rust-dix 0.10 的官方扩展路径（本地 key 类型、
> 自定义 `IServiceResolver`、显式注册 + 自定义 resolver）与**尚未提供**的能力（async AnyKey）。
>
> 总原则：**不要复制注入宏、不要重复造轮子**。优先用现有宏/注册 API；只有语义确实超出
> keyed 服务表达范围时，才用自定义 resolver 作为受控补充。

## 1. 官方扩展路径：`impl From<LocalKey> for ServiceKey` + 现有宏

下游框架若有自己的 key 类型（`LocalKey`），**推荐**实现
`From<LocalKey> for ServiceKey`，然后直接用现有 `#[inject(key = <expr>)]` /
`register!(keyed <expr>: ...)` / 显式 `keyed_*` 注册——不需要复制或重写注入宏。

```rust
use rust_dix::*;
use std::sync::Arc;

// 下游 key 类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantKey(pub String);

// 官方扩展点：From<LocalKey> for ServiceKey
impl From<TenantKey> for ServiceKey {
    fn from(k: TenantKey) -> Self {
        ServiceKey::new(k) // 或转成字符串：ServiceKey::from(format!("tenant:{}", k.0))
    }
}

// 注册侧：显式 keyed 注册可用任意 Into<ServiceKey>
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn ITenantDb>(TenantKey("acme".into()), |_| Arc::new(AcmeDb))
    .build()?;

// 注入侧：宏 key 接受表达式；`TenantKey` 已实现 From<...> for ServiceKey
#[derive(Inject)]
pub struct ReportService {
    #[inject(key = TenantKey("acme".into()))]
    db: Arc<dyn ITenantDb>,
}

// module 宏同样接受表达式（module 内用 super:: 引用外层类型；keyed 注册要求
// 类型实现 Default）
#[rust_dix::module]
mod tenant_module {
    rust_dix::register!(keyed super::TenantKey("acme".into()): singleton: super::AcmeDb);
}
```

要点：
- `#[inject(key = <expr>)]` / `#[inject(owned, key = <expr>)]` 接受任意
  `impl Into<ServiceKey>`；字符串字面量仍可用且仍为 `String` key。
- `register!(keyed <expr>: <lifetime>: <Type>)` 接受表达式；`keyed` 后是任意
  `syn::Expr`，再跟 `:`、生命周期、`:`、类型。0.10 起 `register!(keyed <expr>: <lt>:
  dyn Trait => Impl)` 会**真正注册 `Impl`**（此前 `=> Impl` 可能被静默丢弃、错误使用
  trait 的 `Default`）。
- 字段形态不变：`Arc<T>`/`Option<Arc<T>>`（`key`）与 bare `T`/`Option<T>`
  （`owned, key`）；`Vec<Arc<T>>` **仍不支持** key（key 是逐实现而非集合维度）。
- `rdi-E004` 重复 key 检测**仅对字符串字面量**生效（编译期）；表达式 key 可能动态，
  重复按运行时"最后注册优先"。注意 `rdi-E004` 是**宏编译错误**（`compile_error!`），
  不是 `RdiError` 变体；[错误目录](error-catalog.md)只列运行时 `RdiError`。
- **key 表达式求值时机/副作用（0.10）**：每个消费者构造时求值一次；singleton 在
  build/validation 期求值一次；transient 每次解析各求值一次；`register!(keyed <expr>)`
  每次 build 求值一次。key 表达式应为稳定、无副作用的值（建议 `const`/局部变量/显式注册）；
  不要在表达式里做昂贵计算或 I/O。
- 字符串 footgun：字符串注册存 `String` key；`ServiceKey::new("x")` 是 `&str` typed key，
  不相等。字符串键用 `ServiceKey::from("x")` / `"x".into()`。

## 2. 自定义 `IServiceResolver`

`IServiceResolver` 0.10 新增 4 个 typed keyed 方法（都有默认实现）：

| 方法 | 用途 | 默认实现 |
|------|------|----------|
| `get_keyed_any_key(ty, &ServiceKey)` | typed key 类型擦除解析 | 仅当 `key.as_str()` 为 `Some` 时回退 `get_keyed_any` |
| `try_get_keyed_any_result_key(ty, &ServiceKey)` | 可失败 typed 解析 | 回退 `get_keyed_any_key` |
| `get_keyed_owned_any_key(ty, &ServiceKey)` | typed owned 解析 | 仅 string-like key 回退 `get_keyed_owned_any` |
| `try_get_keyed_owned_any_key(ty, &ServiceKey)` | 可失败 typed owned | 回退 `get_keyed_owned_any_key` |

- `ServiceProvider` / `Scope` / `ServiceProviderWrapper` / 构建期 ValidationResolver 已实现
  **完整 typed key**（AnyKey 兜底、owned、validation captive 检查）。
- **外部自定义实现**：默认实现只支持 string-like key；要支持 typed key 必须 override 上述
  方法（并保持 trait 级 panic/`Err` 契约）。
- **对象安全**：trait object（`&dyn IServiceResolver`）上不能加泛型方法；扩展点方法保持
  `&ServiceKey`，不要改成泛型 `Into<ServiceKey>`（宏路径在调用点完成 `Into`）。
- `ServiceKey::as_str()`：`String` / `&'static str`（含 `ServiceKey::new("literal")` 的
  &str typed key）→ `Some`；enum/整数/自定义 typed key、`AnyKey` → `None`。默认 resolver
  对 string-like key 回退旧字符串方法，因此 typed `&'static str` key 会按 **`String` key**
  解析（完整 typed 容器中二者不相等）；typed key 请用 `*_key` 方法与完整 typed 容器。
- **`#[inject(provider, key = ...)]` 是编译错误**：provider 字段不能同时带 key。需要按需
  解析时注入 `#[inject(provider)] provider: Arc<ServiceProvider>`，在方法内用
  `provider.get_keyed(...)` / typed key 解析。

```rust
// 自定义 resolver 的最小 override 示意（仅当确实需要自定义存储/解析时）
impl IServiceResolver for MyResolver {
    fn get_any(&self, ty: &str) -> Option<Arc<dyn Any + Send + Sync>> { /* ... */ }
    fn get_keyed_any(&self, ty: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> { /* ... */ }

    fn get_keyed_any_key(&self, ty: &str, key: &ServiceKey)
        -> Option<Arc<dyn Any + Send + Sync>>
    {
        // 支持 typed key；否则可省略（默认仅 string-like）
        self.my_typed_get(ty, key)
    }
}
```

## 3. 显式注册 + 自定义 Resolver（合法补充）

当 key 语义不适合暴露成 `ServiceKey`（例如 key 含复杂结构、需要手动解析/组合），或需要
“显式注册表 + 自定义解析策略”时，可以使用显式注册 + 自定义 resolver（示例模式：
`IDbContextKey` + `DbContextResolver`）：

| 适用场景 | 推荐路径 |
|----------|----------|
| 有限、可哈希、可枚举的 key | keyed 注册 + `#[inject(key = ...)]`（typed/字符串） |
| 下游本地 key 类型 | `impl From<LocalKey> for ServiceKey` + 宏（官方扩展，见 §1） |
| 复杂 key 语义 / 手动解析 / 非注册表来源 | 显式注册 + 自定义 resolver（本节），并记录为**上游扩展契约** |
| 动态、不可枚举 key 的异步构建 | 同步 AnyKey + 内部 per-key 缓存（见 §4），或评估 async AnyKey |

自定义 resolver 与宏路径**不冲突**：宏负责字段注入与注册形态，resolver 负责解析来源；
但不要复制注入宏来“顺便”实现 keyed 解析——把差异放到 resolver/自定义 key 类型即可。

## 4. async AnyKey 边界（0.10 仍未提供）

- 0.10 **不提供** `async_any_singleton/scoped/transient`；AnyKey 兜底仍是同步注册。
- `get_keyed_async` 对 AnyKey 兜底**回退同步路径**；AnyKey 工厂里不能 `await` 异步初始化。
- 推荐替代：
  1. **有限 key**：用 `async_keyed_singleton/scoped/transient` 显式注册每个 key；
  2. **动态 key**：同步 AnyKey 服务 + 内部 per-key 缓存
     （`tokio::sync::OnceCell` / keyed map，自行单飞与失败重试）；
  3. **async singleton + keyed map**：先异步构建可枚举的 key 集合，再在服务内按 key 查表。
- 触发再评估（提供 async AnyKey）的条件：≥2 个真实消费方需要不可枚举的异步 keyed 构建、
  且无法前移为可枚举初始化；在此之前保持同步兜底语义。

## 5. 相关

- [当前版本语义（0.10）](semantics.md) — 类型化键 / AnyKey / async / 枚举 / 诊断
- [RdiError 错误目录](error-catalog.md) — 运行时 `RdiError`（`KeyedServiceNotFound` 等）；
  `rdi-E004` 是宏编译错误，见 [crates/macros README 行为说明](https://github.com/Rust-Framework/rust-dix/blob/main/crates/macros/README.md)
- [多实现与键控服务](../03-interface-oriented/multiple-implementations.md)
- [crates/macros README](https://github.com/Rust-Framework/rust-dix/blob/main/crates/macros/README.md)
- [crates/core README](https://github.com/Rust-Framework/rust-dix/blob/main/crates/core/README.md)
