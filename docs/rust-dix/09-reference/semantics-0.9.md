# 参考 · 0.9 语义权威页

> **单一权威源**：本页是 rust-dix **0.9.x** 容器语义的唯一权威说明。其它章节只保留
> 情境摘要并链接回本页；两者冲突时以本页为准。
>
> 适用版本：**0.9.1**（0.9.0 行为 + 文档补丁；0.9.x 无破坏性变更）。与 MEDI 的逐条对照
> 见 [与 MEDI 的差异清单](../01-introduction/medi-differences.md)，从 0.8.x 升级的动作见
> [迁移速查](../01-introduction/medi-differences.md#12-迁移速查07x--09x)。

## 1. 类型化键与字符串 footgun

- keyed API 接受 `impl Into<ServiceKey>`：`&str` / `String` 保持源码兼容；
  `ServiceKey::new(value)` 支持任意 `Any + Send + Sync + Eq + Hash + Debug` 值（enum/整数/自定义）。
- 键相等**包含值的类型**：`ServiceKey::new(1u32) != ServiceKey::from("1")`。
- `ServiceKey::any_key()` / `KeyedService::any_key()` 是注册侧 `AnyKey` 哨兵；
  `ServiceKey::is_any()` 判别哨兵；单值解析**不能**用哨兵。
- **字符串 footgun**：字符串注册 API（`keyed_singleton("x", …)`）存的是 `String` key；
  `ServiceKey::new("x")` 是 `&str` typed key，**不相等**。字符串键请用
  `"x".into()` / `ServiceKey::from("x")`；`ServiceKey::new` 只用于有意使用 typed
  （enum / 整数 / `&'static str`）键的场景。

```rust
use rust_dix::*;

#[derive(Debug, PartialEq, Eq, Hash)]
enum DbKey { Read, Write }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IDb>(ServiceKey::new(DbKey::Read), |_| Arc::new(ReadPool))
    .keyed_singleton::<dyn IDb>("write", |_| Arc::new(WritePool))
    .build()?;

provider.get_keyed::<dyn IDb>(ServiceKey::new(DbKey::Read))?;
provider.get_keyed::<dyn IDb>(ServiceKey::from("write"))?; // 不是 ServiceKey::new("write")
```

## 2. `AnyKey` 兜底

- 注册：`keyed_any_singleton` / `keyed_any_scoped` / `keyed_any_transient`，工厂签名
  `Fn(&dyn IServiceResolver, &ServiceKey) -> Arc<T>`，接收**被请求的 key**。
- **精确 key 注册优先**；无精确注册时才使用兜底。
- **缓存维度 = (服务注册/类型, 被请求 key)**，不是仅请求 key：
  - 同一注册 + 同一 key 复用同一实例；不同 key、或不同 `AnyKey` 注册（即使同 key）各自独立、
    不串缓存；
  - scoped 为每 scope 每 (注册, key) 一份；transient 每次解析新建。
- **build validation 预填充**：build 期同步 singleton 依赖 AnyKey 并按请求 key 解析时，实例写入
  `(注册, key)` 缓存；运行时同注册 + 同 key 复用**同一实例**（单次初始化；多消费者共享一次
  构造，无二次构建）。
- **枚举排除**：`get_all_keyed` / `get_all_keyed_any` 只列显式 key，不含 AnyKey 兜底。
- **单值非法**：`get_keyed(any_key())` → `Err`；`is_registered_keyed(any_key())` → `false`。
- **build 选项**：`ServiceProviderOptions::new()` / `Default` 为严格校验；
  `with_validate_scopes(false)` 或 `lenient()` 恢复 0.7.x root-scope 行为（生产慎用）。
- **owned 例外**：AnyKey 的 scoped/transient `get_keyed_owned` / `try_get_keyed_owned` 在
  strict root 下**允许**（与 exact-key owned 一致：不进缓存/不跟踪，传递依赖仍受 scope 校验）。
- **嵌套 key**：AnyKey 工厂内嵌套解析**不同请求 key** 合法；只有同一注册 + 同一请求 key 的
  重入才是环（`CircularDependency`）。
- **key 读取**：工厂拿到 `&ServiceKey`，可用 `Debug` / `Display` / `Eq` / `Hash`；**有意不提供**
  typed downcast 访问器（擦除表示私有）。需要按 key 分支时用相等/hash，或在闭包中捕获类型信息。
- AnyKey 注册仅同步（`get_keyed_async` 回退同步路径）；不做 eager 初始化/单独校验；
  `keyed_any_scoped` 被同步 singleton 捕获仍按 captive dependency 拒绝。

```rust
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IStorage>("admin", |_| Arc::new(AdminStore)) // 精确优先
    .keyed_any_singleton::<dyn IStorage>(|_, key| Arc::new(TenantStore::new(key)))
    .build()?;

let a: Arc<dyn IStorage> = provider.get_keyed("tenant-1")?; // 兜底，(类型, key) 缓存
let b: Arc<dyn IStorage> = provider.get_keyed("tenant-2")?; // 另一实例
```

## 3. async 解析与环检测

- `build_async()` 初始化 async singleton：**依赖驱动、注册顺序无关**（依赖优先拓扑序）。
- `get_async` / `get_keyed_async` 返回 `impl Future`（构造时捕获解析链；`.await` 用法不变）。
- **链式环检测**：自引用/交叉递归返回 `RdiError::CircularDependency`，合法并发不误报。
- **跨任务边界**：在 async 工厂内 `sp.async_context()` 捕获 `AsyncFactoryContext`，`Clone` 到
  `tokio::spawn` 等任务并经 `ctx.get_async` / `ctx.get_keyed_async` 解析；否则完全脱离的任务
  仍可能死锁。`AsyncFactoryContext` 也可从 `Scope` 获取（`scope.async_context()`），并提供
  `provider()`。
- **同步 singleton 不得依赖 async 服务**：build 期返回
  `SyncSingletonDependsOnAsyncService { singleton, async_service }`。
- **同步解析 async 服务**：`get` 返回 `AsyncServiceRequiresGetAsync`；`get_optional` 返回
  `None`（不 panic）；`get_owned` / `try_get_owned` 同理按各自签名处理。
- 含 `async_*` 注册却调 `build()` → `AsyncServiceRequiresBuildAsync`（仅此场景）。

```rust
.async_singleton(|sp| Box::pin(async move {
    let ctx = sp.async_context().expect("built provider");
    let dep = tokio::spawn({
        let ctx = ctx.clone();
        async move { ctx.get_async::<DbPool>().await }
    }).await.expect("task")?;
    Arc::new(Service { dep })
}))
```

## 4. 枚举与 async

`get_all` / `get_all_keyed` / `#[inject] Vec<Arc<T>>`：

- **包含已初始化**的 async singleton（`build_async` 已构建并缓存）；
- 同步枚举**未初始化** async singleton/transient → `AsyncServiceRequiresGetAsync`：
  - `get_all` / `get_all_keyed`（签名不可失败）以 typed `RdiError` payload panic；
  - `get_all_any_result` / `Vec<Arc<T>>` 注入返回 `Err`（该路径先做 async 就绪检查）；
  - async **scoped** 在 root provider 上会被严格 scope 校验**静默省略**，在 `Scope` 内枚举才 panic；
- async scoped/transient 请用 `get_async` 逐项解析。

## 5. 错误与诊断

集中清单见 [RdiError 错误目录](error-catalog.md)。要点：

- `RdiError::is_circular()` 是**启发式**：直接 `CircularDependency` 或 `FactoryPanic` payload
  含 `CircularDependency` 文本都会命中，自定义 payload 可能误判；精确形态优先匹配 typed 变体。
- `AsyncServiceRequiresBuildAsync` 只用于"含 `async_*` 却调 `build()`"；同步解析 async 服务用
  `AsyncServiceRequiresGetAsync`。

## 6. 工程基线

| 项 | 值 |
|----|----|
| MSRV | Rust **1.95**（`rust-version`，CI matrix 覆盖 1.95 / stable） |
| 安全 | 两个 crate 均 `#![forbid(unsafe_code)]`；漏洞上报/支持版本/边界见 `SECURITY.md` |
| CI | `.github/workflows/ci.yml`：ubuntu + windows × 1.95/stable；stable fmt/clippy(-D warnings)/doc(-D warnings)；RustSec `cargo audit` |
| 行为 | 0.9.1 为**文档补丁**，无行为变更；升级无风险 |
| 释放 | `Scope` 的 Drop 会逆序释放 scope 内实例；`ServiceProvider` 需**显式** `dispose()` 才确定性释放 singleton/root scoped（幂等，之后解析返回 `ProviderDisposed`） |

## 7. 迁移与延伸

- 0.7.x / 0.8.x → 0.9.x：见
  [迁移速查](../01-introduction/medi-differences.md#12-迁移速查07x--09x)。
- 生产部署检查：见 [生产环境部署检查清单](production-checklist.md)（原
  `docs/production-deployment-checklist.md` 已迁入参考章）。
- API 细节：见 [crates/core README](../../../crates/core/README.md) 与 docs.rs API 文档。
