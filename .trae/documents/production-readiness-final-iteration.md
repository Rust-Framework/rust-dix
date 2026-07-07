# DI 框架生产就绪最终迭代计划

## Summary

前序迭代（1.x + 2.x + 3.x）的 7 个核心任务已全部完成，184+ 测试通过。本次最终迭代聚焦剩余的"最后一公里"问题：1 个 clippy 阻塞错误 + 多处面向用户的文档与已删除 API 不一致。

**核心原则**：拒绝冗余代码、拒绝妥协设计、拒绝兼容代码；架构简单清晰、逻辑严谨、功能全面；严格遵循 SRP/OCP/LSP/ISP/DIP。

**迭代范围**：仅修复 clippy 阻塞 + 文档一致性。不引入新功能、不重构现有架构、不修改历史文档（CHANGELOG、.trae/documents/ 过程产物、.agents/skills/ skill 文档）。

---

## Current State Analysis

### 已完成（无需再动）
| 能力 | 状态 |
|------|------|
| 三生命周期 + 键控 + Owned + 跨 DLL + 异步 | ✅ 184+ 测试通过 |
| ScopeFactory trait 可注入 singleton | ✅ |
| TypeId O(1) 直查（trait 默认 + ServiceProvider/Scope/Wrapper 覆盖） | ✅ |
| IProvider 统一 trait（删除 IServiceLocator/INamedRegistrar/ServiceLocatorBridge/impl_service_locator!） | ✅ |
| dispose_scoped 用 try_lock 避免并发死锁 | ✅ |
| RdiError::ServiceNotFound 带 context 字段 | ✅ |
| ServiceProviderWrapper 补齐 owned/async 方法 | ✅ |
| build()/build_async() 统一返回 Arc<ServiceProvider> | ✅ |

### 待修复（按优先级）

| 优先级 | 问题 | 位置 | 影响 |
|--------|------|------|------|
| **P0** | clippy `collapsible_match` 错误 | `crates/macros/src/field_init.rs` L46-56 | 阻塞 `cargo clippy -- -D warnings` |
| **P1** | README 引用已删除的 `Disposable` trait | `README.md` L89 | 误导用户 |
| **P1** | README 引用已删除的 `get_required_service` | `README.md` L182-183 | 误导用户 |
| **P1** | core README 引用已删除的 `IServiceLocator`/`ServiceLocatorBridge`/`RdiProvider` | `crates/core/README.md` L372-379 | 误导用户 |
| **P1** | core README.zh 同上 | `crates/core/README.zh.md` L394 附近 | 误导用户 |
| **P1** | macros README.zh 引用已删除的 `into_arc()` | `crates/macros/README.zh.md` L167 | 误导用户 |
| **P1** | API 映射表引用已删除的 `.create_scope()` | `docs/rust-dicore/01-introduction/INDEX.md` L50 | 误导用户 |
| **P2** | 源码注释引用已删除的 `IServiceLocator` | `crates/core/src/provider.rs` L17 | 代码可读性 |
| **P2** | 源码注释引用已删除的 `IServiceLocator` | `crates/core/src/entry.rs` L247 | 代码可读性 |
| **P2** | ScopeFactory 文档注释提到不存在的 `instance` 注册方式 | `crates/core/src/scope.rs` L13-14 | 误导用户（`instance` 要求 `T: Sized`，`dyn ScopeFactory` 是 `?Sized`） |

### 明确不修复
- `CHANGELOG.md` — 历史日志，记录当时的真实状态，不回溯修改
- `.trae/documents/*.md` — 过程计划文档，历史产物
- `.agents/skills/lrdi-skill/reference/api.md` — skill 参考文档，独立维护周期
- `.trae/documents/di-framework-quality-review.md` — 历史审查报告

---

## P0 任务：修复 clippy collapsible_match

**问题**：`crates/macros/src/field_init.rs` L46-56 的 `Vec` 分支有嵌套 `if let`，clippy 要求折叠。

**当前代码**（L45-57）：
```rust
"Vec" => {
    if let Some(inner) = single_generic_type(last) {
        if let Type::Path(ip) = inner {
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

**修改方案**：将 `if let Some(inner) = single_generic_type(last)` 与 `if let Type::Path(ip) = inner` 折叠为一个模式：

```rust
"Vec" => {
    if let Some(Type::Path(ip)) = single_generic_type(last) {
        if let Some(ilast) = ip.path.segments.last() {
            if ilast.ident == "Arc" {
                if let Some(t) = single_generic_type(ilast) {
                    return (quote! {#t}, FieldKind::VecArc);
                }
            }
        }
    }
}
```

**技术约束**：
- `single_generic_type` 返回 `Option<&Type>`，`Type::Path(ip)` 模式匹配 `&Type` 时需用 `&Type::Path(ip)` 或 `Type::Path(ip)`（Rust 模式匹配自动解引用 `Option<&Type>` 内的 `&Type`）
- 同样的折叠模式已用于 L31-43 的 `"Option"` 分支（`if let Some(inner_ty) = single_generic_type(last)` 内嵌 `if let Type::Path(ip) = inner_ty`），但 clippy 未报告该处——因为该处 inner block 有 `else` 分支，无法直接折叠。`"Vec"` 分支无 `else`，可折叠

**验证**：`cargo clippy --all-targets -- -D warnings` 无警告；`cargo test --all` 全量通过。

---

## P1 任务：修复面向用户的文档不一致

### 1.2.1 README.md L89 — Disposable trait 已删除

**当前**：
```
| **Scoped cleanup** | `Disposable` trait + automatic `Drop` for scoped resource release |
```

**改为**：
```
| **Scoped cleanup** | Cleanup closures + automatic `Drop` for scoped resource release |
```

**理由**：`Disposable` trait 在迭代 1.2 中已删除（`&mut self` 签名在 `Arc<T>` 缓存下不可用），改为纯闭包注册模式（`provider.register_disposable(Box::new(|| { ... }))`）。

### 1.2.2 README.md L177-184 — get_required_service 已删除

**当前**（L177-184）：
```markdown
### Error handling

- Prefer `try_get::<T>()` / `get_optional::<T>()` (returns `Option`) for
  optional dependencies
- Use `get::<T>()` (returns `Result`) when the dependency is required
- `get_required_service::<T>()` panics on missing — use only when absence is
  a programming error
```

**改为**：
```markdown
### Error handling

- Prefer `try_get::<T>()` / `get_optional::<T>()` (returns `Option`) for
  optional dependencies
- Use `get::<T>()` (returns `Result`) when the dependency is required —
  absence is a recoverable error, not a panic
```

**理由**：`get_required_service` 是已删除的 MEDI 别名。Rust 习惯用 `Result`，panic 是反模式。

### 1.2.3 crates/core/README.md L372-379 — IServiceLocator/ServiceLocatorBridge/RdiProvider 已删除

**当前**（L372-379）：
```markdown
#### 🔹 External system integration via IServiceLocator

​```rust
let locator: Arc<dyn IServiceLocator> = Arc::new(ServiceLocatorBridge::new(
    Arc::new(RdiProvider::Root(provider)),
));
// Pass to third-party code or FFI boundary
​```
```

**改为**：
```markdown
#### 🔹 External system integration via IProvider

​```rust
let provider: Arc<dyn IProvider> = provider as Arc<dyn IProvider>;
// Pass to third-party code or FFI boundary
​```
```

**理由**：`IServiceLocator`/`ServiceLocatorBridge`/`RdiProvider` 已在迭代 3.2 中删除，统一为 `IProvider` trait。`ServiceProvider`/`Scope`/`ServiceProviderWrapper` 均实现 `IProvider`，可直接作为 `Arc<dyn IProvider>` 跨边界传递。

### 1.2.4 crates/core/README.zh.md L394 附近 — 同上中文版

**操作**：读取 `crates/core/README.zh.md`，找到对应的 `IServiceLocator`/`ServiceLocatorBridge`/`RdiProvider` 段落，做与 1.2.3 相同的替换（中文描述保留，代码块同步更新）。

### 1.2.5 crates/macros/README.zh.md L167 — into_arc() 已删除

**当前**：
```
| `#[inject(provider)]` | `Arc<ServiceProvider>` | 注入 provider 供按需解析（需 `into_arc()`） |
```

**改为**：
```
| `#[inject(provider)]` | `Arc<ServiceProvider>` | 注入 provider 供按需解析 |
```

**理由**：`into_arc()` 在迭代 2.1 中已删除，`build()` 直接返回 `Arc<ServiceProvider>`，`provider_arc()` 开箱即用。

### 1.2.6 docs/rust-dicore/01-introduction/INDEX.md L50 — create_scope() 已删除

**当前**：
```
| `CreateScope()` | `.create_scope()` |
```

**改为**：
```
| `CreateScope()` | `.scope()` |
```

**理由**：`create_scope()` 是已删除的 MEDI 别名，统一用 `scope()`。

---

## P2 任务：修复源码注释不一致

### 1.3.1 crates/core/src/provider.rs L17 — IServiceLocator → IServiceResolver

**当前**（L17）：
```rust
/// String → TypeId lookup for IServiceLocator string-based resolution.
```

**改为**：
```rust
/// String → TypeId lookup for IServiceResolver string-based resolution.
```

### 1.3.2 crates/core/src/entry.rs L247 — IServiceLocator → IServiceResolver

**当前**（L247）：
```rust
pub type_name: &'static str, // kept for IServiceLocator string-based resolution
```

**改为**：
```rust
pub type_name: &'static str, // kept for IServiceResolver string-based resolution
```

### 1.3.3 crates/core/src/scope.rs L13-14 — ScopeFactory 文档不准确

**当前**（L12-14）：
```rust
/// `ServiceProvider` 实现此 trait，通过 [`ServiceProvider::scope`] 创建 scope。
/// 注册方式：`provider.singleton::<dyn ScopeFactory>(|_| provider_arc().unwrap().clone() as Arc<dyn ScopeFactory>)`，
/// 或更简洁地用 `instance` 注册：`.instance(provider_arc().unwrap() as Arc<dyn ScopeFactory>)`。
```

**改为**：
```rust
/// `ServiceProvider` 实现此 trait，通过 [`ServiceProvider::scope`] 创建 scope。
/// 注册方式：`provider.singleton::<dyn ScopeFactory>(move |_| provider.clone() as Arc<dyn ScopeFactory>)`。
```

**理由**：`instance<T: Send + Sync + 'static>` 要求 `T: Sized`，而 `dyn ScopeFactory` 是 `?Sized`，无法用 `instance` 注册。文档不应建议不可用的 API。实际测试中用的是 `singleton::<dyn ScopeFactory>`（见 `tests/scope.rs::scope_factory_injected_into_singleton`）。

---

## Assumptions & Decisions

### 设计决策
1. **仅修复，不重构** — 前序迭代已完成架构清理，本次只修"最后一公里"问题
2. **不修改历史文档** — CHANGELOG、.trae/documents/、.agents/skills/ 是过程产物，独立维护
3. **clippy 折叠用最小改动** — 只折叠 clippy 报告的 L47，不重构整个 `classify_field` 函数（其他嵌套有 `else` 分支无法折叠）
4. **文档同步中英文** — core README 和 core README.zh 同步更新

### 技术约束
- `single_generic_type` 返回 `Option<&Type>`，`if let Some(Type::Path(ip)) = single_generic_type(last)` 模式合法
- `instance<T: Send + Sync + 'static>` 要求 `T: Sized`，`dyn ScopeFactory` 是 `?Sized` 不可用
- `IProvider` trait 已统一，`ServiceProvider`/`Scope`/`ServiceProviderWrapper` 均实现，可直接 `as Arc<dyn IProvider>`

### 依赖变更
- 无

---

## Verification Steps

### 完成标准
- [ ] `cargo clippy --all-targets -- -D warnings` 零警告零错误
- [ ] `cargo test --all` 全量通过（184+ 测试）
- [ ] `README.md` 无 `Disposable trait` / `get_required_service` 引用
- [ ] `crates/core/README.md` + `README.zh.md` 无 `IServiceLocator` / `ServiceLocatorBridge` / `RdiProvider` 引用
- [ ] `crates/macros/README.zh.md` 无 `into_arc()` 引用
- [ ] `docs/rust-dicore/01-introduction/INDEX.md` 无 `.create_scope()` 引用
- [ ] 源码注释无 `IServiceLocator` 引用（`provider.rs` L17、`entry.rs` L247）
- [ ] `scope.rs` ScopeFactory 文档删除不可用的 `instance` 注册建议

### 执行顺序
1. **P0 修复 clippy** → 运行 `cargo clippy -- -D warnings` 验证
2. **P1 修复面向用户文档**（README.md、core README、macros README.zh、INDEX.md）→ 人工检查
3. **P2 修复源码注释**（provider.rs、entry.rs、scope.rs）→ 运行 `cargo doc --no-deps` 验证无断链
4. **最终验证**：`cargo clippy --all-targets -- -D warnings` + `cargo test --all` + `cargo doc --no-deps`
