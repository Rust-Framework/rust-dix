# Scope 生命周期修复 + panic 类型安全 + 多态注入 — 收尾计划

## 总结

本计划承接上一轮会话，完成三项改造的剩余收尾工作：

1. **Scope 生命周期 bug 修复**（方案 1：Scope 内部持有真 ServiceProvider 实例）— 主体已完成
2. **循环依赖检测与工厂 panic 的 catch_unwind + 类型安全 panic_any 处理**（避免锁中毒）— 主体已完成
3. **多态注入支持**（C# `IEnumerable<IFoo>` 等价语义）— 宏侧尚缺关键分支

## 当前状态分析（已通过 Read 验证）

### 已落盘的工作（无需重做）

| 任务 | 文件 | 状态 |
|------|------|------|
| 1. FactoryPanic 错误变体 | `crates/core/src/error.rs` | ✅ 已含 `FactoryPanic { service, payload }` |
| 2. IServiceResolver 加 `get_all_any` + `get_all<T>` | `crates/core/src/entry.rs` | ✅ 默认空 Vec，`get_all<T>` 走 `Self: Sized` |
| 3. ServiceProvider 字段 Arc 化 + `new_scope` | `crates/core/src/provider.rs` | ✅ `store`/`type_map`/`singleton_cache`/`named` 均 `Arc`，`root_scoped_cache`/`self_weak` 独立 |
| 4. ValidationResolver 改 `panic_any(RdiError::...)` + `validate_dependencies` 单分支 downcast | `crates/core/src/provider.rs` | ✅ 4 处 `panic_any` + 简化 downcast |
| 5. `run_factory_safely` 包装所有工厂调用 | `crates/core/src/provider.rs` | ✅ Singleton/Scoped/Transient 三分支 + `create_owned_any_by_entry` 均包装 |
| 6. Scope 重构为持有 `scope_provider: Arc<ServiceProvider>` | `crates/core/src/scope.rs` | ✅ 所有方法委托；`provider_arc()` 返回 scope 范围 provider |
| 7. Wrapper + Bridge 实现 `get_all_any` | `crates/core/src/wrapper.rs`、`crates/core/src/bridge.rs` | ✅ Wrapper 合并 child+root；RdiProvider 分发 Root/Wrapped |

### 待完成的工作（本计划范围）

| 任务 | 文件 | 缺失内容 |
|------|------|----------|
| 8. `gen_field_init` 加 `VecArc` 分支 | `crates/macros/src/lib.rs` | 当前 `#[inject]` 非_owned 分支的 `match kind` 没有 `VecArc` 臂，落到 `_ => Err(...)` 编译错误；需补 polymorphic 收集分支 |
| 9. README 加多态注入行 | `crates/macros/README.md`、`crates/macros/README.zh.md` | 字段属性表缺 `Vec<Arc<T>>` 行；严格类型检查说明缺 Vec 语义 |
| 10. 修复 `provider_inject.rs` 测试 | `crates/core/tests/provider_inject.rs` | 第 71 行注释 "delegates to parent" 已过时（Scope 现持 scope_provider）；缺 scope 缓存命中断言；缺 `scope_provider_resolves_scoped_within_scope` 用例 |
| 11. 新建 `poly_inject.rs` 测试 | `crates/core/tests/poly_inject.rs`（不存在） | 缺多态注入端到端测试 |
| 12. 全量构建 + 测试验证 | 全 workspace | 未运行 |

## 提议的修改

### 任务 8：`gen_field_init` 加 `VecArc` 分支

**文件**：`d:\GitCode\RF\rust-di\crates\macros\src\lib.rs`

**修改点**：在 `gen_field_init` 函数的 `if !attrs.owned { match kind { ... } }` 分支内，于 `FieldKind::Arc` 臂之后、`_ => Err(...)` 之前插入 `FieldKind::VecArc` 臂。

**插入代码**：

```rust
FieldKind::VecArc => {
    // 多态注入：收集某 trait/类型下所有注册（default + keyed），等价 C# IEnumerable<IFoo>。
    // key 不适用——key 是 per-implementation 的，由 store 决定哪些 entry 被收集。
    if attrs.key.is_some() {
        return Err(syn::Error::new_spanned(
            field,
            "#[inject] on Vec<Arc<T>> does not support key; key is per-implementation",
        ));
    }
    Ok(quote! {
        #fnm: resolver.get_all_any(::std::any::type_name::<#inner>())
            .into_iter()
            .map(|a| a.downcast::<::std::sync::Arc<#inner>>()
                .ok()
                .unwrap_or_else(|| ::std::panic!(
                    "polymorphic inject: type mismatch for {}",
                    ::std::any::type_name::<#inner>()
                )))
            .map(|d| ::std::sync::Arc::clone(&*d))
            .collect::<::std::vec::Vec<::std::sync::Arc<#inner>>>()
    })
}
```

**同步修改错误消息**：`!attrs.owned` 分支的 `_ =>` 错误消息从
```
"#[inject] requires Arc<T> or Option<Arc<T>> field type; use #[inject(owned)] for bare T"
```
改为
```
"#[inject] requires Arc<T>, Option<Arc<T>>, or Vec<Arc<T>> field type; use #[inject(owned)] for bare T"
```

**`#[inject(owned)]` 分支不动**：`VecArc` 自然落到 `_ => Err(...)`（owned 多态无意义，错误消息已足够说明）。

**为什么这样实现**：
- 复用 `resolver.get_all_any`（IServiceResolver 默认空 Vec，ServiceProvider 遍历 store 同 TypeId 下所有 entry）。
- downcast 失败时 panic 而非静默跳过，与 `Arc` 分支的 `unwrap_or_else(panic!("svc not registered"))` 一致——类型不匹配是注册错误，应早爆。
- 不支持 `key`：`Vec<Arc<T>>` 语义是"收集全部"，与单实例 keyed 选择互斥；编译期拒绝避免误用。
- `collect` 显式标注 `Vec<Arc<#inner>>` 确保类型推导不歧义。

### 任务 9：README 加多态注入行

**文件**：
- `d:\GitCode\RF\rust-di\crates\macros\README.md`（英文）
- `d:\GitCode\RF\rust-di\crates\macros\README.zh.md`（中文）

**修改点**：在 `#[derive(Inject)]` 章节的 "Field attributes" 表格中，于 `#[inject(provider)]` 行之前插入多态注入行。

**README.md 新增行**：

| Attribute | Field type | Behavior |
|-----------|-----------|----------|
| `#[inject]` | `Vec<Arc<T>>` | Polymorphic — collects all registered implementations (default + keyed); key not supported |

**README.zh.md 新增行**（需对照中文表格风格）：

| 属性 | 字段类型 | 行为 |
|------|----------|------|
| `#[inject]` | `Vec<Arc<T>>` | 多态注入 — 收集所有注册实现（default + keyed）；不支持 key |

**同步更新严格类型检查说明**：现有句子
> Type checking is strict: `#[inject]` only accepts `Arc<T>` / `Option<Arc<T>>`, ...

改为
> Type checking is strict: `#[inject]` accepts `Arc<T>`, `Option<Arc<T>>`, or `Vec<Arc<T>>`, ...

中文版同步。

### 任务 10：修复 `provider_inject.rs` 测试

**文件**：`d:\GitCode\RF\rust-di\crates\core\tests\provider_inject.rs`

**修改 1**：`provider_inject_works_through_scope` 测试（第 60-73 行）

- 注释从 `// Through scope, provider_arc delegates to parent — owned resolution bypasses cache` 改为 `// Scope 现持有 scope_provider；注入的字段是 scope 范围 provider，解析 Scoped 命中 scope 缓存`
- 在 `assert_eq!(h.resolve_dep().value, 99);` 之后追加 scope 缓存命中断言：通过同一 scope 二次解析 Scoped，验证 `Arc::ptr_eq` 为 true（证明命中 scope 而非 root）

**新增测试**：`scope_provider_resolves_scoped_within_scope`

验证 Scope 生命周期修复的核心契约：
- Scoped 服务在 scope 内缓存复用（命中 scope_provider 的 root_scoped_cache）
- 不同 scope 各自独立（序号不同）
- 经 `#[inject(provider)]` 注入的 provider 解析 Scoped 命中当前 scope，不泄漏到 root

实现思路：用 `AtomicU64` 计数器记录 Scoped 工厂调用次数，验证 scope1/scope2/root 三次解析产生三个不同实例，且 scope1 内多次解析复用同一实例。

### 任务 11：新建 `poly_inject.rs` 测试

**文件**：`d:\GitCode\RF\rust-di\crates\core\tests\poly_inject.rs`（新建）

**测试用例**：

1. `get_all_baseline` — 不用宏，直接调 `provider.get_all::<dyn IFoo>()` 收集 default + keyed 实现，验证数量与可调用性。作为多态注入的对照基线。

2. `vec_inject_collects_all_implementations` — 用 `#[derive(Inject)]` 在 `Vec<Arc<dyn IFoo>>` 字段上加 `#[inject]`，注册 1 default + 2 keyed 实现后解析，验证字段长度 = 3 且每个实现可调用。

3. `vec_inject_empty_when_unregistered` — 未注册任何实现时，`Vec<Arc<dyn IFoo>>` 字段为空 Vec（不 panic）。

4. `vec_inject_concrete_type` — 非 trait 类型 `Vec<Arc<MyService>>` 也能多态注入（收集同 TypeId 下所有 entry）。

5. `vec_inject_rejects_key` — `#[inject(key = "k")]` on `Vec<Arc<T>>` 应编译失败（compile_fail 测试或省略，仅靠宏内 `Err` 保证）。

**测试 trait/struct 定义**：复用 `common::IPlugin`/`TestPlugin`/`AltPlugin`（已存在于 `tests/common/mod.rs`），并新增局部 trait `IFoo` + 2-3 个实现以独立验证多态语义。

### 任务 12：全量构建 + 测试验证

**命令**：
- `cargo build --workspace` — 验证宏改动编译通过
- `cargo test --workspace` — 验证所有测试通过（含新增 poly_inject.rs 与修复后的 provider_inject.rs）
- 关注 `multi_impl.rs` 中既有 `get_all` 测试不回归

## 假设与决策

1. **VecArc 仅在 `#[inject]`（非 owned）分支支持**：owned 多态无意义（Vec 是集合语义，不是单实例所有权），落到错误分支即可，错误消息已足够。

2. **downcast 失败时 panic 而非返回空**：与现有 `Arc` 分支的 `unwrap_or_else(panic!("svc not registered"))` 一致；类型不匹配是注册错误，应早爆而非静默丢失数据。

3. **不修改 `#[inject]` 在 struct 上的注册语义**：`Vec<Arc<T>>` 字段注入是消费侧（resolver 调用），不影响 struct 自身被注册为某个 trait 的工厂。

4. **README 改动最小化**：仅在字段属性表加一行 + 更新严格类型检查句子，不扩展章节结构。

5. **`provider_inject.rs` 不删除任何现有测试**：仅修正过时注释 + 加 scope 缓存命中断言 + 加新测试，保持向后兼容。

6. **不引入 `compile_fail` 测试**：`vec_inject_rejects_key` 的编译期拒绝由宏内 `syn::Error::new_spanned` 保证，运行期无法测试；省略该用例，依赖宏自身的类型检查。

## 验证步骤

1. 任务 8 完成后，`cargo build -p rust-dicore-macros` 应通过（宏 crate 自身编译）。
2. 任务 11 完成后，`cargo test -p rust-dicore --test poly_inject` 应全部通过。
3. 任务 10 完成后，`cargo test -p rust-dicore --test provider_inject` 应全部通过。
4. 任务 12 全量 `cargo test --workspace` 应 0 失败，且测试总数应较改造前增加（新增 poly_inject 用例 + scope 缓存命中断言）。
5. 任务 9 README 改动不影响编译，仅文档审查。

## 执行顺序

8 → 11 → 10 → 12 → 9（README 最后改，避免被代码改动牵连返工）
