# 字段注入显式化重构 · 文档收尾计划

## Summary

字段注入显式化重构的核心代码（宏实现 + 测试迁移 + 新增显式注入测试）已完成并通过全部 154 项测试。
本计划聚焦**剩余的文档收尾工作**：将所有文档中残留的旧 `#[rdi(...)]` 语法更新为新 `#[inject(...)]` 语义，
并为文档示例中**未标记的可注入字段**补上 `#[inject]` / `#[inject(owned)]` 标记（新语义下未标记字段走 `Default::default()`，
旧示例若不补标记将行为错误）。最后做整库验证。

## Current State Analysis

### 已完成（确认无误）

- **`crates/rust-dicore-macros/src/lib.rs`**：`#[proc_macro_derive(Inject, attributes(inject))]`，`IA` 结构体使用 `inject`/`owned`/`provider`/`key`，`gen_field_init` 返回 `syn::Result` 并做严格类型校验，剥离 `#[inject]` 属性。✅
- **测试文件**（`inject.rs`、`inject_attribute.rs`、`owned.rs`、新增 `explicit_inject.rs`）：已迁移到 `#[inject]` / `#[inject(owned)]` 标记。✅
- **`crates/rust-dicore-macros/README.md` / `README.zh.md`**：字段属性表、示例已更新为 `#[inject(...)]`。✅（但 `README.zh.md:249` 对比表 cons 列仍残留 `#[rdi(skip)]`，需修）

### 残留工作（grep `#[rdi(` 确认）

旧 `#[rdi(...)]` 语法仍残留在以下文档（排除 `target/` 构建产物与 `.trae/documents/` 历史计划文件）：

| 文件 | 残留位置 | 性质 |
|------|---------|------|
| `crates/rust-dicore-macros/README.zh.md:249` | 对比表 cons 列 `String 字段需 #[rdi(skip)]` | 漏改 |
| `crates/rust-dicore/README.zh.md:253-266, 318` | `Handler` 裸 T 示例无标记；`OrderService` `#[rdi(provider)]` | 语法 + 行为 |
| `crates/rust-dicore/README.md:267-280` | `Handler` 裸 T/Arc 示例无标记 | 行为（无 `#[rdi(`，但缺标记） |
| `docs/rust-dicore/03-interface-oriented/dyn-trait-registration.md:185-220` | `FileLogger`/`Handler` 多处 `#[rdi(...)]` + 未标记 `Arc<dyn ...>` 字段 | 语法 + 行为 |
| `docs/rust-dicore/08-best-practices/common-pitfalls.md:125,130,254-275,297-317` | 陷阱 4/8/9 | 语法 + 行为 |
| `docs/rust-dicore/08-best-practices/interface-checklist.md:163-168` | `CreateUserHandler` `#[rdi(optional)]` + 未标记字段 | 语法 + 行为 |
| `.agents/skills/lrdi-skill/SKILL.md:73,113,297` | 概述与排查 | 语法 |
| `.agents/skills/lrdi-skill/reference/api.md:533-591` | 字段属性表 + 示例 | 语法 + 行为 |
| `.agents/skills/lrdi-skill/guides/macros.md` | 大量（§2.2/2.4/2.6/3.1-3.4/速查表） | 语法 + 行为 + 描述 |

**关键点**：除单纯的 `#[rdi(...)]` → `#[inject(...)]` 改名外，许多示例中 `Arc<T>` / 裸 `T` 字段**当前无任何标记**（依赖旧的"按类型自动注入"语义）。
新语义下这些字段会走 `Default::default()` 而非注入，导致示例行为错误。必须为每个应注入的字段补 `#[inject]`（`Arc<T>`/`Option<Arc<T>>`）或 `#[inject(owned)]`（裸 `T`/`Option<T>`）。

## Proposed Changes

### 1. `crates/rust-dicore-macros/README.zh.md`（1 处）

**第 249 行**对比表 cons 列：
- `需要 inventory；String 字段需 #[rdi(skip)]` → `需要 inventory；未标记字段需 Default`

### 2. `crates/rust-dicore/README.md`（English，行为修正）

**第 267-280 行** `Handler` 示例：
- 描述句：`a bare T field is auto-resolved as owned` → `mark bare T fields with #[inject(owned)]; Arc<T> fields with #[inject]`
- 代码：
  ```rust
  #[derive(Inject)]
  struct Handler {
      #[inject(owned)]
      ctx: DbContext,           // owned  → get_owned
      #[inject]
      logger: Arc<Logger>,      // shared → get
  }
  ```

### 3. `crates/rust-dicore/README.zh.md`（语法 + 行为）

**第 253-266 行** `Handler` 示例：同上，描述句改为"裸 `T` 字段用 `#[inject(owned)]` 标记按 owned 解析；`Arc<T>` 字段用 `#[inject]` 保持共享"，代码补标记。

**第 316-320 行** `OrderService`：
- `#[rdi(provider)]` → `#[inject(provider)]`
- （注：`#[inject(provider)]` 存在预已知类型问题，属既有问题不在本次范围，仅做语法迁移）

### 4. `docs/rust-dicore/03-interface-oriented/dyn-trait-registration.md`

**第 185-193 行** `FileLogger`：
- `config: Arc<dyn IConfig>` → 加 `#[inject]`
- `#[rdi(optional)] fallback: Option<Arc<dyn ILogger>>` → `#[inject] fallback: Option<Arc<dyn ILogger>>`
- `#[rdi(skip)] name: String` → `name: String`（移除标记）

**第 199-206 行**字段属性表：整表替换为 `#[inject(...)]` 新表（参照 `rust-dicore-macros/README.md` 的字段属性表）

**第 208-220 行** `Handler`：
- `user_svc: Arc<dyn IUserService>` → `#[inject] user_svc: Arc<dyn IUserService>`
- `#[rdi(optional)] logger` → `#[inject] logger`
- `#[rdi(key = "wechat")] pay` → `#[inject(key = "wechat")] pay`
- `#[rdi(provider)] resolver` → `#[inject(provider)] resolver`
- `#[rdi(skip)] name` → `name`（移除标记）

### 5. `docs/rust-dicore/08-best-practices/common-pitfalls.md`

**陷阱 4（第 125、128-132 行）**：
- 第 125 行：`#[rdi(provider)]` → `#[inject(provider)]`
- 第 130 行：`#[rdi(provider)]` → `#[inject(provider)]`

**陷阱 8（第 253-258、272-275 行）**：
- 第 256 行 `Handler.user_svc: Arc<dyn IUserService>` → 加 `#[inject]`
- 第 274 行同字段 → 加 `#[inject]`
- 排查说明补充"注入字段必须显式标 `#[inject]`，否则走 `Default::default()`"

**陷阱 9（第 297-298、317 行）**：
- 第 297-298 行"宏裸 T 字段"描述：改为"`#[derive(Inject)]` 的 `#[inject(owned)]` 裸 T 字段走 owned 路径"
- 第 317 行：`#[derive(Inject)]` 裸 `T` 字段 → `#[inject(owned)]` 裸 `T` 字段

### 6. `docs/rust-dicore/08-best-practices/interface-checklist.md`

**第 163-168 行** `CreateUserHandler`：
- `user_svc: Arc<dyn IUserService>` → `#[inject] user_svc: Arc<dyn IUserService>`
- `#[rdi(optional)] logger` → `#[inject] logger`

### 7. `.agents/skills/lrdi-skill/SKILL.md`

- 第 73 行：`通过 #[rdi(...)] 字段属性控制注入行为` → `通过 #[inject(...)] 字段属性显式标记注入字段`
- 第 113 行：同上改 `#[inject(...)]`
- 第 297 行：`#[rdi(provider)]` → `#[inject(provider)]`

### 8. `.agents/skills/lrdi-skill/reference/api.md`

**第 533-591 行**字段属性段：
- 第 533 行标题：`字段属性（#[rdi(...)]）` → `字段属性（#[inject(...)]）`
- 第 539-542 行属性表：整表替换为新 `#[inject(...)]` 表（参照 macros README）
- 第 566-568 行示例：`#[rdi(optional)]` → `#[inject]`，`#[rdi(skip)]` → 移除标记，并为 `Arc<T>` 字段补 `#[inject]`
- 第 591 行：`#[rdi(...)]` → `#[inject(...)]`

### 9. `.agents/skills/lrdi-skill/guides/macros.md`（重点更新）

本文件描述最详尽，需按新语义重写相关段落。逐段处理：

- **第 5 行**：`#[rdi(...)]` 字段属性 → `#[inject(...)]`
- **第 29 行**：迁移说明整行更新为"字段属性 `#[inject(...)]`，派生宏声明 `#[proc_macro_derive(Inject, attributes(inject))]`"
- **§2.2（第 154 行）**：`parse_ia` 扫描 `#[rdi]` → 扫描 `#[inject]`；`IA` 字段改为 `inject`/`owned`/`provider`/`key`；移除 `skip`/`optional`
- **§2.4（第 375-388 行）语法 4 字段属性表**：整表替换为新表（无标记=Default、`#[inject]`、`#[inject(owned)]`、`#[inject(key=)]`、`#[inject(provider)]`）；补充"严格类型校验"说明
- **§2.4 示例（第 453、468、504-510 行）**：`#[rdi(skip)]` → 移除标记；`#[rdi(optional)]` → `#[inject]`；`#[rdi(key=)]` → `#[inject(key=)]`；`#[rdi(provider)]` → `#[inject(provider)]`；未标记 `Arc<T>` 字段补 `#[inject]`
- **§2.6 排查表（第 556-557 行）**：`#[rdi]` 属性解析失败 → `#[inject]`；支持的键名改为 `owned`/`provider`/`key`；`Default` 缺失条件改为"未标记字段"
- **§3.1-3.3**：`saw()` → `classify_field()`；`FieldKind` 枚举（Arc/OptionArc/Owned/OptionOwned）；`gen_field_init` 返回 `syn::Result` 及严格类型校验逻辑
- **§3.3 字段属性完整参考表（第 727-738 行）**：整表替换
- **§3.4 实战示例（第 749-820 行）**：`OrderService` 字段标记全部更新；未标记 `Arc<T>` 补 `#[inject]`
- **§3.x 排查表（第 830-831 行）**：同 §2.6 排查表更新
- **第 1289 行**：`#[rdi(provider)]` → `#[inject(provider)]`
- **第 1354 行**：`#[rdi(optional)]` → `#[inject]`
- **源码映射速查表**：行号与函数名按当前 `lib.rs` 校正

> **说明**：`macros.md` 篇幅长，采用逐段 Edit 而非整文件重写，保留未受影响的内容。

### 10. 历史计划文件（低优先级，不改）

`.trae/documents/rust-dicore_owned服务解析实现_plan.md`、`字段注入显式化重构_plan.md` 等属历史记录，保留原样。

## Assumptions & Decisions

1. **核心实现 + 测试已完成**：依据 `lib.rs:14` 确认 `attributes(inject)`，依据 grep 确认测试文件已无 `#[rdi(`。本计划不动源码与测试。
2. **`#[inject(provider)]` 既有类型问题不在范围**：`resolver.clone()` 产生 `&dyn IServiceResolver` 而非 `Arc<dyn IServiceResolver>`，是预存在问题。文档仅做语法迁移，不修复该 bug。
3. **未标记可注入字段必须补标记**：新语义下未标记 = `Default::default()`，旧示例中靠"按类型自动注入"的字段若不补标记将行为错误。这是文档正确性要求，非可选。
4. **`Option<...>` 可选性由类型决定**：`#[inject]` + `Option<Arc<T>>` = 可选共享；`#[inject(owned)]` + `Option<T>` = 可选 owned。无需 `optional` 标记。
5. **`#[rdi(skip)]` 语义被默认行为取代**：移除标记即可，未标记字段自动 `Default::default()`。
6. **历史计划文件不改**：`.trae/documents/` 下的计划文件属历史记录，保留原 `#[rdi(...)]` 用法。
7. **`target/` 构建产物不改**：`target/package/` 下的旧版 README 是打包快照，不维护。

## Verification

1. **编译**：`cargo build`（workspace 无错误无新警告）
2. **测试**：`cargo test -p rust-dicore`（154 项全过）+ `cargo test -p rust-dicore-macros`
3. **残留检查**：`grep -rn "#\[rdi("` 排除 `target/` 与 `.trae/documents/`，确认源码与文档零残留
4. **示例正确性抽查**：人工核对 `crates/rust-dicore/README.md` 与 `README.zh.md` 的 `Handler` 示例字段标记齐全

## 实施顺序

1. `crates/rust-dicore-macros/README.zh.md`（1 处，最快）
2. `crates/rust-dicore/README.md` + `README.zh.md`
3. `docs/rust-dicore/03-interface-oriented/dyn-trait-registration.md`
4. `docs/rust-dicore/08-best-practices/common-pitfalls.md`
5. `docs/rust-dicore/08-best-practices/interface-checklist.md`
6. `.agents/skills/lrdi-skill/SKILL.md` + `reference/api.md`
7. `.agents/skills/lrdi-skill/guides/macros.md`（最重，放最后）
8. 验证：`cargo build` + `cargo test` + grep 残留检查
