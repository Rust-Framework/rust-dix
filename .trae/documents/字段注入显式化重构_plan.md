# 字段注入显式化重构

## Context

当前 `#[derive(Inject)]` 和 `#[rust_dicore::inject]` 宏对**所有字段默认尝试注入**，仅靠字段类型推断意图：

- `Arc<T>` → 调 `get_any` 共享注入（未注册 panic）
- 裸 `T` → 调 `get_owned_any` owned 注入（未注册或 Singleton panic）
- 只有 `#[rdi(skip)]` 才用 `Default::default()`

**问题**：
1. 字段是"DI 依赖"还是"内部状态"语义模糊，全靠类型推断
2. 裸 `T` 字段歧义最严重——`ctx: DbContext` 既可能是 owned 服务，也可能是内部状态，宏一律按 owned 注入处理，运行时 panic
3. 内部字段（如 `name: String`）必须实现 `Default` 且加 `#[rdi(skip)]`，否则运行时 panic
4. 可读性差，阅读代码无法直观分辨依赖与状态

**目标**：采用显式注入策略——只有标记了 `#[inject]` / `#[inject(owned)]` 的字段才注入，未标记字段用 `Default::default()`。统一字段标记为 `#[inject(...)]`，移除 `#[rdi(...)]`。严格类型校验，标记与字段类型不匹配则编译错误。

## 设计方案

### 字段标记语法（统一为 `#[inject(...)]`）

| 字段标记 | 允许的字段类型 | 行为 |
|---------|--------------|------|
| 无标记 | 任意（需 `Default`） | `Default::default()` |
| `#[inject]` | `Arc<T>` | 共享注入，未注册 panic `"svc not registered"` |
| `#[inject]` | `Option<Arc<T>>` | 可选共享注入，未注册 `None` |
| `#[inject(key = "k")]` | `Arc<T>` | 键控共享注入，未注册 panic `"keyed not found"` |
| `#[inject(key = "k")]` | `Option<Arc<T>>` | 可选键控共享注入 |
| `#[inject(owned)]` | 裸 `T` | owned 注入，未注册或 Singleton panic |
| `#[inject(owned)]` | `Option<T>` | 可选 owned 注入 |
| `#[inject(owned, key = "k")]` | 裸 `T` / `Option<T>` | 键控 owned 注入 |
| `#[inject(provider)]` | `Arc<dyn IServiceResolver>` | `resolver.clone()` |

### 关键规则

- **可选性由字段类型决定**：`Option<...>` 字段自动可选，无需 `optional` 标记。移除 `#[rdi(optional)]`
- **未标记 = 内部字段**：用 `Default::default()`。移除 `#[rdi(skip)]`（其语义被默认行为取代）
- **严格类型校验**（编译期）：
  - `#[inject]`（非 owned）只允许 `Arc<T>` / `Option<Arc<T>>`，否则 `syn::Error`
  - `#[inject(owned)]` 只允许裸 `T` / `Option<T>`，否则 `syn::Error`
- **命名决策**：保留 `#[derive(Inject)]`（CamelCase，符合 Rust 约定，无 lint），字段属性统一为 `#[inject(...)]`

### 技术可行性（命名冲突）

- `#[proc_macro_derive(Inject, attributes(inject))]` 声明 `inject` 为 derive helper attribute
- `#[proc_macro_attribute] pub fn inject(...)` 是属性宏
- **不冲突**：字段上的 `#[inject]` 在 `#[derive(Inject)]` 上下文中被 derive 消费；在 `#[rust_dicore::inject]` 上下文中被属性宏消费。两者不会同时出现在同一 struct
- 属性宏 `expand_inject_struct` 需剥离字段上的 `#[inject]` 属性（类似当前剥离 `#[rdi]` 的逻辑），避免编译器报"unknown attribute"

## 实现步骤

### 1. 核心宏实现：`crates/rust-dicore-macros/src/lib.rs`

**a. 修改 `IA` 结构体与 `parse_ia` 函数**（当前第 175-206 行）

```rust
#[derive(Default)]
struct IA {
    inject: bool,      // 是否有 #[inject] 标记（非 provider）
    owned: bool,       // owned 注入
    provider: bool,    // 注入 resolver
    key: Option<String>,
}

fn parse_ia(f: &Field) -> IA {
    let mut a = IA::default();
    for attr in &f.attrs {
        if !attr.path().is_ident("inject") {
            continue;
        }
        a.inject = true;
        let Ok(l) = attr.meta.require_list() else {
            continue;  // #[inject] 无 list，仅标记 inject
        };
        l.parse_nested_meta(|m| {
            if m.path.is_ident("owned") {
                a.owned = true;
            } else if m.path.is_ident("provider") {
                a.provider = true;
                a.inject = false;
            } else if m.path.is_ident("key") {
                a.key = Some(m.value()?.parse::<syn::LitStr>()?.value());
            }
            Ok(())
        }).ok();
    }
    a
}
```

**b. 修改 `gen_field_init` 函数**（当前第 102-173 行）

- 返回类型从 `proc_macro2::TokenStream` 改为 `syn::Result<proc_macro2::TokenStream>`（支持编译错误）
- 新逻辑顺序：
  1. 无标记（`!inject && !provider`）→ `Default::default()`
  2. `provider` → `resolver.clone()`
  3. `inject && !owned` → 共享注入；校验 `kind` 必须是 `Arc` 或 `OptionArc`，否则 `Err(syn::Error::new_spanned(field, "#[inject] requires Arc<T> or Option<Arc<T>>"))`
  4. `inject && owned` → owned 注入；校验 `kind` 必须是 `Owned` 或 `OptionOwned`，否则 `Err(syn::Error::new_spanned(field, "#[inject(owned)] requires bare T or Option<T>"))`
- 保留 `classify_field` 函数（仍需识别 Arc/Option/裸 T）
- 保留 `arc_extract` / `owned_extract` 提取逻辑
- 可选性由 `FieldKind::OptionArc` / `OptionOwned` 自动决定，移除 `attrs.optional` 分支

**c. 修改 derive 宏声明**（第 14 行）

```rust
#[proc_macro_derive(Inject, attributes(inject))]  // 原: attributes(rdi)
```

**d. 修改 `expand_inject_derive` 和 `expand_inject_struct`**

- 调用 `gen_field_init(field)?` 处理 `syn::Result`
- `expand_inject_struct` 中剥离字段属性的 `retain` 逻辑（第 536 行）：
  ```rust
  field.attrs.retain(|a| !a.path().is_ident("inject"));  // 原: "rdi"
  ```

### 2. 测试文件更新

**a. `crates/rust-dicore/tests/inject.rs`**（4 处 `#[rdi(...)]`）

```rust
#[derive(Inject)]
struct FullFeatured {
    #[inject]                                            // 原: 无标记
    log: Arc<common::Logger>,
    #[inject]                                            // 原: #[rdi(optional)]
    maybe: Option<Arc<common::MyService>>,
    #[inject(key = "special")]                           // 原: #[rdi(key = "special")]
    special: Arc<common::Logger>,
    label: String,                                       // 原: #[rdi(skip)]，现无标记
}
```

**b. `crates/rust-dicore/tests/inject_attribute.rs`**（2 处 `#[rdi(...)]`）

- `#[rdi(skip)] label: String` → `label: String`（无标记）
- `#[rdi(optional)] maybe: Option<Arc<ExplicitSingleton>>` → `#[inject] maybe: Option<Arc<ExplicitSingleton>>`
- `FieldInjectionService.dep: Arc<DefaultLifetimeService>` → 加 `#[inject]`

**c. `crates/rust-dicore/tests/owned.rs`**（1 处 `#[rdi(...)]` + 多处需加标记）

- `OwnedHandler.core: Arc<ScopedCore>` → `#[inject] core: Arc<ScopedCore>`
- `MixedHandler.svc: common::MyService` → `#[inject(owned)] svc: common::MyService`
- `MixedHandler.logger: Arc<common::Logger>` → `#[inject] logger: Arc<common::Logger>`
- `OptHandler.maybe: Option<common::MyService>` → `#[inject(owned)] maybe: Option<common::MyService>`
- `KeyedOwnedHandler`: `#[rdi(key = "primary")]` → `#[inject(owned, key = "primary")]`
- `BlogHandler.ctx: DbContext` → `#[inject(owned)] ctx: DbContext`
- `BlogHandler.logger: Arc<common::Logger>` → `#[inject] logger: Arc<common::Logger>`
- `CaptiveSingleton.ctx: CaptiveScoped` → `#[inject(owned)] ctx: CaptiveScoped`

**d. 其他测试文件**：检查 `tests/patterns.rs`、`tests/multi_impl.rs`、`tests/module.rs`、`tests/resolution.rs`、`tests/scope.rs`、`tests/validation.rs`、`tests/wrapper.rs`、`tests/named.rs`、`tests/collection.rs`、`tests/bridge.rs`、`tests/coverage_gap.rs` 中是否有 `#[derive(Inject)]` struct 需补充 `#[inject]` 标记

### 3. 新增测试（验证显式化语义）

在 `crates/rust-dicore/tests/inject.rs` 末尾或新文件 `tests/explicit_inject.rs` 添加：

- `unmarked_field_uses_default`：未标记字段用 `Default::default()`，不尝试注入
- `inject_on_option_arc_returns_none_when_absent`：`#[inject]` + `Option<Arc<T>>` 未注册返回 `None`
- `inject_owned_on_option_returns_none_when_absent`：`#[inject(owned)]` + `Option<T>` 未注册返回 `None`
- 编译错误用例（`tests/ui/` 目录或 `trybuild`）：`#[inject]` 标记裸 T → 编译错误；`#[inject(owned)]` 标记 `Arc<T>` → 编译错误。若项目未启用 `trybuild`，则在文档中说明

### 4. 文档更新

**a. `crates/rust-dicore-macros/README.md` 和 `README.zh.md`**
- 字段属性表（第 163-174 行 / 149-161 行）：移除 `#[rdi(...)]` 表，改为 `#[inject(...)]` 表
- 示例代码（第 146 行 / 132 行）：更新字段标记
- 第 261 行 / 241 行对比表："String 字段需 `#[rdi(skip)]`" → "未标记字段用 `Default`"

**b. `crates/rust-dicore/README.md` 和 `README.zh.md`**
- 字段注入示例（第 267-272 行 / 253-258 行）：裸 `T` 字段需 `#[inject(owned)]`
- `#[rdi(provider)]` 示例（README.zh.md 第 318 行）→ `#[inject(provider)]`

**c. `docs/rust-dicore/08-best-practices/common-pitfalls.md`**
- 陷阱 4（第 126-139 行）：`#[rdi(provider)]` → `#[inject(provider)]`
- 陷阱 8（第 241-276 行）：字段注入示例需加 `#[inject]` 标记
- 陷阱 9（第 278-324 行）：`#[derive(Inject)]` 裸 `T` 字段需 `#[inject(owned)]`

**d. `.agents/skills/lrdi-skill/guides/macros.md`**
- §2.2 阶段 2（第 154 行）：`parse_ia` 扫描 `#[inject]` 而非 `#[rdi]`
- §2.4 语法 4（第 375-388 行）：字段属性表全面更新
- §2.6 实战示例（第 500-512 行）：`OrderService` 字段标记更新
- §3.1-3.3：`#[derive(Inject)]` 指南全面更新，`saw()` 改为 `classify_field()`
- §3.3 字段属性完整参考表（第 727-745 行）：移除 `#[rdi(...)]`，改为 `#[inject(...)]`
- §3.4 实战（第 749-820 行）：`OrderService` 示例更新
- 源码映射速查表（第 1411-1434 行）：行号与函数名更新

**e. `.agents/skills/lrdi-skill/SKILL.md` 和 `reference/api.md`**
- 检索其中 `#[rdi(...)]` 引用并更新

**f. `.trae/documents/rust-dicore_owned服务解析实现_plan.md`**
- 历史计划文件，若包含 `#[rdi(...)]` 示例则更新（低优先级，属历史记录）

## 关键文件清单

| 文件 | 修改类型 |
|------|---------|
| `crates/rust-dicore-macros/src/lib.rs` | 核心宏实现重构 |
| `crates/rust-dicore/tests/inject.rs` | 字段标记迁移 |
| `crates/rust-dicore/tests/inject_attribute.rs` | 字段标记迁移 |
| `crates/rust-dicore/tests/owned.rs` | 字段标记迁移 |
| `crates/rust-dicore/tests/{patterns,multi_impl,module,resolution,scope,validation,wrapper,named,collection,bridge,coverage_gap}.rs` | 检查并补充 `#[inject]` 标记 |
| `crates/rust-dicore-macros/README.md` + `README.zh.md` | 文档更新 |
| `crates/rust-dicore/README.md` + `README.zh.md` | 文档更新 |
| `docs/rust-dicore/08-best-practices/common-pitfalls.md` | 文档更新 |
| `.agents/skills/lrdi-skill/guides/macros.md` | 文档更新 |
| `.agents/skills/lrdi-skill/SKILL.md` + `reference/api.md` | 文档更新 |

## 验证方式

1. `cargo build -p rust-dicore-macros` — 宏编译通过
2. `cargo test -p rust-dicore` — 所有现有测试通过（迁移后行为不变）
3. `cargo test -p rust-dicore-macros` — 宏测试通过
4. `cargo build` — 整个 workspace 无警告
5. 手动验证：未标记字段不再尝试注入（写一个 demo struct 含未标记的 `String` 字段，确认不 panic）
6. 新增测试通过：显式注入语义验证

## 破坏性变更说明

此变更是 **0.6.0 破坏性变更**：
- 移除 `#[rdi(...)]` helper attribute，改为 `#[inject(...)]`
- 移除 `#[rdi(skip)]`（未标记字段默认 `Default`）
- 移除 `#[rdi(optional)]`（由 `Option<...>` 字段类型自动决定）
- 所有 `#[derive(Inject)]` / `#[rust_dicore::inject]` struct 的注入字段必须显式加 `#[inject]` 或 `#[inject(owned)]` 标记

迁移指南：
- `Arc<T>` 字段：加 `#[inject]`
- `Option<Arc<T>>` 字段：加 `#[inject]`（移除 `#[rdi(optional)]`）
- 裸 `T` 字段（owned 服务）：加 `#[inject(owned)]`
- `Option<T>` 字段（可选 owned）：加 `#[inject(owned)]`
- `#[rdi(key = "k")]` → `#[inject(key = "k")]` 或 `#[inject(owned, key = "k")]`
- `#[rdi(skip)]` → 移除标记（未标记字段默认 `Default`）
- `#[rdi(provider)]` → `#[inject(provider)]`
