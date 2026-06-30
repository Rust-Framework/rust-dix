# Scope 生命周期修复 + 类型安全 panic + 多态注入

## Context

本次改动解决审查发现的三个问题：

1. **Scope 桥接生命周期 bug**：[scope.rs:216](file:///d:/GitCode/RF/rust-di/crates/core/src/scope.rs#L216) `Scope::provider_arc()` 返回 `Arc::clone(&self.parent)`（根 provider）。scope 内解析的带 `#[inject(provider)]` 字段的服务拿到根 provider，经该字段 `get::<ScopedX>()` 走根的 `root_scoped_cache`，**绕过子 scope 隔离**——scoped 泄漏，违反 MEDI 语义。现有测试 [provider_inject.rs:61](file:///d:/GitCode/RF/rust-di/crates/core/tests/provider_inject.rs#L61) 把泄漏当预期（注释 "delegates to parent"）。

2. **循环依赖 + 工厂 panic 不类型安全**：[provider.rs:163/222/270/310](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L163) 用 `panic!("CIRCULAR_DEPENDENCY: ...")` 字符串抛错，[provider.rs:436/442](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L436) 用 `strip_prefix` 在 `catch_unwind` 中识别——payload 类型一变就漏判。captive 路径已用 `panic_any(RdiError)`，循环路径没同等待遇。运行期工厂 panic 直接传播，无类型安全包装。

3. **多态注入缺口**：运行时 `get_all::<T>()` 已支持，但宏不支持自动注入 `Vec<Arc<dyn Trait>>` 字段（[macros/lib.rs:56-86](file:///d:/GitCode/RF/rust-di/crates/macros/src/lib.rs#L56) `classify_field` 无 Vec 分支），且 `IServiceResolver` trait 无 `get_all_any` 方法。等价 C# `IEnumerable<IFoo>` 构造注入能力缺失。

用户已否决接口方案（注入 `Arc<dyn IServiceProvider>`），保留 `Arc<ServiceProvider>` 桥接，故 Scope 修复走方案 1（内嵌真 ServiceProvider 实例）。

## 任务 1：Scope 内嵌 ServiceProvider（方案 1）

### 改动文件
- [crates/core/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs)
- [crates/core/src/scope.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/scope.rs)

### ServiceProvider 字段改为 Arc 共享

```rust
pub struct ServiceProvider {
    store: Arc<ServiceStore>,                                          // 改 Arc
    type_map: Arc<HashMap<&'static str, TypeId>>,                      // 改 Arc
    singleton_cache: Arc<LazyCache>,                                   // 改 Arc
    root_scoped_cache: LazyCache,                                      // 每个 provider 独立
    named: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,  // 改 Arc
    self_weak: OnceLock<Weak<ServiceProvider>>,                        // 每个 provider 独立
}
```

理由：scope 派生时需共享 store/singleton_cache/named/type_map（只读或全局语义），独立 root_scoped_cache（per-scope 隔离）和 self_weak（scope 自己的弱引用）。

### ServiceProvider::new() 调整

- `let store = Arc::new(store);` 包装
- `let type_map = Arc::new(type_map);` 包装
- `let singleton_cache = Arc::new(LazyCache::new());` 包装
- `let named = Arc::new(RwLock::new(HashMap::new()));` 包装
- `validate_dependencies(&store, &singleton_cache)` —— 函数签名改为接 `&Arc<ServiceStore>` 和 `&Arc<LazyCache>`（或解引用传 `&**store`/`&**singleton_cache` 保持原签名，二选一，倾向后者改动小）
- Phase 2 eager init 中 `sp.singleton_cache.get_or_init_with(...)` 通过 Arc deref 调用，不变

### 新增 ServiceProvider::new_scope()

```rust
/// 创建一个 scope 范围的 ServiceProvider，共享 store/type_map/singleton_cache/named，
/// 独立 root_scoped_cache 和 self_weak。
/// 供 Scope::new() 调用，使 #[inject(provider)] 字段拿到的是 scope 范围的 provider。
pub(crate) fn new_scope(parent: &Arc<Self>) -> Arc<Self> {
    let sp = Self {
        store: Arc::clone(&parent.store),
        type_map: Arc::clone(&parent.type_map),
        singleton_cache: Arc::clone(&parent.singleton_cache),
        root_scoped_cache: LazyCache::new(),
        named: Arc::clone(&parent.named),
        self_weak: OnceLock::new(),
    };
    let arc = Arc::new(sp);
    let this: &Self = &arc;
    this.self_weak.get_or_init(|| Arc::downgrade(&arc));
    arc
}
```

### Scope 重构为薄包装

```rust
pub struct Scope {
    scope_provider: Arc<ServiceProvider>,
}

impl Scope {
    pub(crate) fn new(parent: Arc<ServiceProvider>) -> Self {
        Self { scope_provider: ServiceProvider::new_scope(&parent) }
    }
    pub fn scope_provider(&self) -> &Arc<ServiceProvider> { &self.scope_provider }
}
```

Scope 所有公共方法（`get`/`get_optional`/`get_keyed`/`get_owned`/`try_get_owned`/`get_keyed_owned`/`try_get_keyed_owned`/`get_all`/`get_named_any`/MEDI 别名 `get_service`/`get_required_service`/`get_services`）委托给 `self.scope_provider`。

`impl IServiceResolver for Scope` 全部委托：

```rust
fn get_any(&self, key: &str) -> Option<...> { self.scope_provider.get_any(key) }
fn get_keyed_any(&self, key: &str, variant: &str) -> Option<...> { self.scope_provider.get_keyed_any(key, variant) }
fn get_owned_any(&self, key: &str) -> Option<...> { self.scope_provider.get_owned_any(key) }
fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<...> { self.scope_provider.get_keyed_owned_any(key, variant) }
fn provider_arc(&self) -> Option<Arc<ServiceProvider>> { Some(Arc::clone(&self.scope_provider)) }
```

删除 Scope 中的 `scoped_cache` 字段、`get_any_by_entry`、`create_owned_any_by_entry` —— 这些已被 scope_provider 内部接管。

`rdi_register_named`/`rdi_remove_named` 委托给 `self.scope_provider`。

### 修复测试

- [provider_inject.rs:61](file:///d:/GitCode/RF/rust-di/crates/core/tests/provider_inject.rs#L61) `provider_inject_works_through_scope`：注释 "delegates to parent" 改为 "delegates to scope provider"，并新增断言验证 `scope.get_owned::<HandlerWithProvider>().provider` 经由该 provider 取 Scoped 服务命中 scope 缓存（不是 root）。
- 新增测试 `scope_provider_resolves_scoped_within_scope`：scope 内 resolve 带 `#[inject(provider)]` 字段的 Scoped 服务，从注入的 provider 多次 `get::<ScopedX>()` 应返回同一 Arc（scope 缓存命中），不同 scope 的 provider 取出应不同。

## 任务 2：catch_unwind + 类型安全 panic_any

### 改动文件
- [crates/core/src/error.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/error.rs)
- [crates/core/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs)

### error.rs 新增变体

```rust
#[error("factory panicked while resolving service '{service}': {payload}")]
FactoryPanic { service: String, payload: String },
```

### ValidationResolver 循环依赖改 panic_any

4 处 `panic!("CIRCULAR_DEPENDENCY: {}", cycle_desc)` → `std::panic::panic_any(RdiError::CircularDependency(cycle_desc))`

位置：[provider.rs:163](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L163)、222、270、310。

### validate_dependencies downcast 简化

[provider.rs:426-448](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L426) 简化为：

```rust
Err(panic_info) => {
    if let Some(err) = panic_info.downcast_ref::<RdiError>() {
        return Err(err.clone());
    }
    // 未知 panic 重新抛出
    std::panic::panic_any(panic_info);
}
```

删除 String/`&str` 前缀匹配分支——现在循环依赖和 captive 都走 RdiError。

### 工厂调用点加 catch_unwind

新增辅助函数（在 provider.rs 内）：

```rust
/// 在 catch_unwind 中执行工厂，捕获 panic 并转换为类型安全的 RdiError。
/// - 若 panic payload 是 RdiError（循环依赖/captive），保持原样重新抛出（上层 validate 处理）。
/// - 若是其他 panic，包装为 RdiError::FactoryPanic 重新抛出（类型安全，运行期可识别）。
fn run_factory_safely<F>(service_name: &'static str, factory: F) -> AnyService
where F: FnOnce() -> AnyService,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(factory)) {
        Ok(v) => v,
        Err(payload) => {
            // RdiError 直接重新抛出，保留原始错误类型
            if payload.is::<RdiError>() {
                std::panic::panic_any(payload);
            }
            let msg = payload
                .downcast_ref::<&str>().map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().map(|s| s.clone()))
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            std::panic::panic_any(RdiError::FactoryPanic {
                service: service_name.to_string(),
                payload: msg,
            });
        }
    }
}
```

调用点改造：

- [provider.rs:609](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L609) Singleton 分支：`get_or_init_with(ck, move || run_factory_safely(entry.type_name, || factory(resolver)))`
- [provider.rs:619](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L619) Scoped 分支：同上
- [provider.rs:622](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L622) Transient 分支：`Some(run_factory_safely(entry.type_name, || (entry.factory)(self)))`
- [provider.rs:638](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L638) `create_owned_any_by_entry` Scoped/Transient 分支：同上

Scope 委托后不再有独立工厂调用点（已被 scope_provider 接管）。

### 关于"避免锁中毒"的说明

LazyCache 已把工厂执行移出 RwLock 临界区（[cache.rs:88-90](file:///d:/GitCode/RF/rust-di/crates/core/src/cache.rs#L88) 注释明确），工厂 panic 不会让 LazyCache 的 RwLock 中毒。catch_unwind 主要价值是**类型安全错误传递**——上层 `validate_dependencies` 可统一 downcast RdiError，运行期用户也可通过 `catch_unwind` 识别 `FactoryPanic`。RwLock 中毒的根本防护仍是 LazyCache 的无锁工厂执行设计。

## 任务 3：多态注入（接口集合）

### 改动文件
- [crates/core/src/entry.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/entry.rs)
- [crates/core/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs)
- [crates/core/src/scope.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/scope.rs)
- [crates/core/src/wrapper.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/wrapper.rs)
- [crates/core/src/bridge.rs](file:///d:/GitCode/RF/rust-di/crates/core/src/bridge.rs)（RdiProvider 实现）
- [crates/macros/src/lib.rs](file:///d:/GitCode/RF/rust-di/crates/macros/src/lib.rs)
- [crates/macros/README.md](file:///d:/GitCode/RF/rust-di/crates/macros/README.md) + README.zh.md（文档表更新）

### IServiceResolver 新增 get_all_any

[entry.rs:9](file:///d:/GitCode/RF/rust-di/crates/core/src/entry.rs#L9) trait 加方法（带默认实现）：

```rust
/// 类型擦除的批量解析：返回某类型所有注册项（default + keyed）。
/// 用于 `Vec<Arc<T>>` 字段的多态注入。
/// 默认空 Vec 保持外部实现向后兼容。
fn get_all_any(&self, _key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
    Vec::new()
}
```

### 各实现

- `ServiceProvider::get_all_any`：复用 `entries_by_tid`，遍历所有 entry 调 `get_any_by_entry` 收集
- `Scope::get_all_any`：委托 `self.scope_provider.get_all_any(key)`
- `ServiceProviderWrapper::get_all_any`：合并 child + root 的结果
- `RdiProvider::get_all_any`：match 两个变体委托
- `ValidationResolver::get_all_any`：可选实现（build 阶段遍历依赖图，调用各 entry 工厂），但 validation 主要走 `get_any`，可暂用默认空 Vec

同时新增泛型便捷方法到 `IServiceResolver`（默认方法，带 `Self: Sized`）：

```rust
fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>>
where Self: Sized,
{
    self.get_all_any(std::any::type_name::<T>())
        .into_iter()
        .filter_map(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
        .collect()
}
```

注意：`ServiceProvider` 和 `Scope` 已有 inherent `get_all<T>` 方法（[provider.rs:525](file:///d:/GitCode/RF/rust-di/crates/core/src/provider.rs#L525)、scope 委托），inherent 方法优先于 trait 方法，调用方不受影响。

### 宏支持 Vec<Arc<T>>

[macros/lib.rs:49](file:///d:/GitCode/RF/rust-di/crates/macros/src/lib.rs#L49) `FieldKind` 新增 `VecArc`。

[macros/lib.rs:56](file:///d:/GitCode/RF/rust-di/crates/macros/src/lib.rs#L56) `classify_field` 新增分支：

```rust
"Vec" => {
    if let Some(inner) = single_generic_type(last) {
        if let syn::Type::Path(ip) = inner {
            if let Some(ilast) = ip.path.segments.last() {
                if ilast.ident == "Arc" {
                    if let Some(t) = single_generic_type(ilast) {
                        return (quote! {#t}, FieldKind::VecArc);
                    }
                }
            }
        }
    }
}
```

[macros/lib.rs:142](file:///d:/GitCode/RF/rust-di/crates/macros/src/lib.rs#L142) `gen_field_init` `#[inject]` 分支新增 `FieldKind::VecArc`：

```rust
FieldKind::VecArc => {
    if attrs.key.is_some() {
        return Err(syn::Error::new_spanned(field,
            "#[inject] on Vec<Arc<T>> does not support key; key is per-implementation"));
    }
    Ok(quote! {
        #fnm: resolver.get_all_any(::std::any::type_name::<#inner>())
            .into_iter()
            .map(|a| a.downcast::<::std::sync::Arc<#inner>>().ok().unwrap_or_else(|| {
                ::std::panic!("polymorphic inject: type mismatch for {}", ::std::any::type_name::<#inner>())
            }))
            .map(|d| ::std::sync::Arc::clone(&*d))
            .collect::<::std::vec::Vec<::std::sync::Arc<#inner>>>()
    })
}
```

`#[inject(owned)]` 分支拒绝 VecArc（owned 不支持集合，编译错误）。

### 文档更新

[crates/macros/README.md:180](file:///d:/GitCode/RF/rust-di/crates/macros/README.md#L180) 字段标记表新增一行：

```
| `#[inject]` | `Vec<Arc<T>>` / `Vec<Arc<dyn Trait>>` | Polymorphic injection — collects all implementations |
```

同步 README.zh.md。

### 测试

新增 `crates/core/tests/poly_inject.rs`：

- `Vec<Arc<dyn Trait>>` 字段，注册 3 个实现（含 default + keyed），宏注入后 Vec 长度 = 3
- `Vec<Arc<dyn Trait>>` 字段，全 keyed 注册，宏注入后 Vec 长度 = N
- 空注册返回空 Vec（不 panic）
- 通过 Scope 解析带集合字段的服务，验证集合内每个实现正确

## 验证

```powershell
cargo build --workspace
cargo test --workspace
cargo test -p rust-dicore --test provider_inject
cargo test -p rust-dicore --test scope
cargo test -p rust-dicore --test poly_inject
cargo test -p rust-dicore --test validation
cargo test -p rust-dicore --test multi_impl
```

预期：现有 157 测试全部通过（除 provider_inject::provider_inject_works_through_scope 注释/断言更新），新增 poly_inject 测试通过，scope lifecycle 新测试通过。

## 风险与权衡

1. **ServiceProvider 字段 Arc 化**：会触发 ServiceProvider 内部所有字段访问点改动，但 Arc<T> 自动 deref，访问语法基本不变。store/singleton_cache/named 的所有权语义从"独占"变为"共享"，符合 root scope 与子 scope 共享全局状态的语义。
2. **Scope 不再持有 parent Arc**：当前 Scope 持 `parent: Arc<ServiceProvider>`，重构后持 `scope_provider: Arc<ServiceProvider>`（由 new_scope 派生）。scope_provider 内部共享 parent 的 store/singleton_cache/named，所以 Scoped 服务的依赖（Singleton 等）仍正确解析自全局缓存。
3. **catch_unwind 性能开销**：每次工厂调用多一次 catch_unwind。但工厂调用本身是 Arc<dyn Fn> 动态分发 + 闭包逻辑，catch_unwind 开销相对可忽略。MEDI 等价场景同样有异常处理开销。
4. **多态注入的 type_name 路径**：宏生成代码走 `type_name::<T>()` 字符串路径（与现有 `#[inject]` 一致），比 `ServiceProvider::get_all` 直接 TypeId 多一次字符串 hash。属一致性妥协，可后续优化。
5. **FieldKind::VecArc 不支持 key**：集合注入语义是"收集所有实现"，key 用于单个实现的策略选择，二者正交。若需"按 key 收集子集"未来再扩展。
