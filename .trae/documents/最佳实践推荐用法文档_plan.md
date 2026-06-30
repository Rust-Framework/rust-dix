# 最佳实践推荐用法文档 · 实施计划

## Context

当前 docs 与 skill 文档存在两个缺口：
1. **路径简化缺位**：框架宏全限定路径 `#[rust_dicore::inject]`、`rust_dicore::register!()` 冗长，但文档从未引导用户用 `use rust_dicore::*` 简化。现有 55 处（docs）+ 144 处（skill）示例几乎全用全限定路径，与"简洁"诉求相悖。
2. **场景决策分散**：框架功能点完备（4 类宏 + 全套注册 API + 10 种设计模式），但"什么场景用什么方式"散落在各处，无统一决策矩阵。用户不清楚何时手写构造函数、何时用 `#[inject]` 字段标记、何时用闭包工厂。

**目标**：新建决策指南文档明确推荐用法，引导路径简化，挖掘代码简洁性技巧；同步统一最佳实践章节示例为简短风格，避免自相矛盾。

**已确认决策**（用户）：
- 路径简化主推 `use rust_dicore::*;`（大型项目可选 `use rust_dicore as di;` 别名）
- 功能点场景以新建 `usage-guide.md` 决策指南组织
- 充分挖掘简化编码、提高可读性的方法沉淀到文档

**技术依据**：`crates/rust-dicore/src/lib.rs:206` `pub use rust_dicore_macros::{inject, module, register, Inject};` + L139/150/160/168 等 `pub use` 类型，`use rust_dicore::*` 后 `#[inject]`、`#[derive(Inject)]`、`#[module]`、`register!()`、`ServiceCollection`、`ServiceProvider`、`Scope`、`IServiceResolver` 均可直接使用。

## Proposed Changes

### 1. 新建 `docs/rust-dicore/08-best-practices/usage-guide.md`（核心交付）

面向用户的决策指南，含 7 节：

**§1 导入约定（路径简化）**
- 推荐写法：文件顶部 `use rust_dicore::*;` + `use std::sync::Arc;`
- 简化对照表：
  | 旧写法 | 推荐写法 |
  |--------|---------|
  | `#[rust_dicore::inject]` | `#[inject]` |
  | `#[rust_dicore::inject(scoped)]` | `#[inject(scoped)]` |
  | `#[derive(rust_dicore::Inject)]` | `#[derive(Inject)]` |
  | `#[rust_dicore::module]` | `#[module]` |
  | `rust_dicore::register!(...)` | `register!(...)` |
  | `rust_dicore::ServiceCollection::new()` | `ServiceCollection::new()` |
- 大型多 crate 项目可选 `use rust_dicore as di;` → `#[di::inject]`
- 约定：每个使用 DI 的文件顶部加这两行 use

**§2 注册方式决策矩阵**
| 场景 | 推荐方式 | 一句话理由 |
|------|---------|-----------|
| 标准服务自动注册 | `#[inject]` 属性宏 | 零样板，注册+构造一步到位 |
| 面向接口 trait 注册 | `#[inject]` 在 impl 块 | trait 自动推断，无需 `as =` |
| 多实现运行时路由 | keyed + 属性宏/闭包 | 策略模式 |
| 集中声明式配置 | `#[module]` + `register!()` | 注册与服务模块同处 |
| 已有实例/配置值 | `singleton_value(T)` / `instance(Arc<T>)` | 免工厂闭包 |
| 条件可选注册 | `try_add` | 不覆盖已注册 |
| 复杂工厂/运行时参数 | 手动闭包 `|p| Arc::new(...)` | 需要计算逻辑 |

**§3 构造函数策略（手写 vs 宏标记 vs 闭包）**
- 三种方式对比表（代码量、结构体干净度、适用场景）
- **手写构造函数**：结构体字段零属性，最干净；闭包内 `MyService::new(p.get(), p.get())`。适用：初始化逻辑复杂、字段需计算、不适合标记。
- **`#[inject]` 字段标记**：`#[derive(Inject)]` 或 `#[inject]` 属性宏自动生成构造函数。适用：依赖都是可直接解析的服务。未标记字段（内部状态）自动 `Default`，无需 `#[rdi(skip)]`。
- **闭包工厂**：手动 `|p| Arc::new(...)`。适用：已有实例、条件构造、运行时参数。
- 决策树：依赖能否直接解析？→ 能：`#[inject]` 标记；不能（需计算/已有实例）：手写构造 or 闭包

**§4 生命周期选择**（引用 INDEX 既有表，不重复）

**§5 字段注入标记速查**
- `#[inject]` → `Arc<T>` / `Option<Arc<T>>`（共享）
- `#[inject(owned)]` → 裸 `T` / `Option<T>`（owned）
- 未标记 → `Default::default()`（内部状态）
- 严格类型校验：标记与类型不匹配触发编译错误

**§6 代码简洁性技巧（核心挖掘）**
1. `use rust_dicore::*` 统一引入，消除长路径
2. `#[inject]` 属性宏零样板（vs 手动 `ServiceCollection` 逐个注册）
3. impl 块 trait 自动推断（无需 `as = dyn Trait`）
4. `singleton_value(T)` 自动 Arc 包装（vs `instance(Arc::new(T))`）
5. `from_injected()` 一行收集所有属性宏注册（vs 逐个 `.singleton()`）
6. `Option<Arc<T>>` 可选依赖由类型决定，无额外标记
7. 单元结构体 `struct Config;` 直接支持
8. `try_add` 条件注册避免覆盖
9. 未标记字段自动 `Default`，结构体保持干净
10. keyed 服务实现策略模式，一接口多实现

**§7 完整示例：冗长 → 简洁演进**
同一 `OrderService` 三种写法代码量对比（全限定手动闭包 → `use *` + 闭包 → `#[inject]` 属性宏），直观展示简洁性提升。

### 2. 更新 `docs/rust-dicore/08-best-practices/INDEX.md`

- 小节表格追加条目：`[用法指南与决策矩阵](usage-guide.md) | 导入约定、场景推荐、简洁性技巧`（置顶，作为首选阅读）
- "下一步"指向 usage-guide.md

### 3. 统一 best-practices 章节示例为简短路径（一致性保证）

将以下 3 文件中 `#[rust_dicore::inject...]` → `#[inject...]`、`rust_dicore::register!` → `register!`、`rust_dicore::module` → `#[module]`，并在代码块顶部加 `use rust_dicore::*;`：
- `docs/rust-dicore/08-best-practices/anti-patterns.md`（9 处）
- `docs/rust-dicore/08-best-practices/interface-checklist.md`（11 处）
- `docs/rust-dicore/08-best-practices/common-pitfalls.md`（17 处）

> best-practices 章节应以身作则，与新 usage-guide.md 推荐风格一致，避免"文档说简化、示例用长路径"的自相矛盾。

### 4. 统一 `docs/rust-dicore/03-interface-oriented/dyn-trait-registration.md` 示例（11 处）

核心示例文档，同样统一为 `use rust_dicore::*` + `#[inject]` 风格。

### 5. 更新 `docs/rust-dicore/02-quickstart/INDEX.md`

在快速开始中加入"导入约定"提示，引导新用户从一开始就用 `use rust_dicore::*`。

### 6. skill `SKILL.md` 更新

- **§"当用户要求添加 LRDI 到我的项目"**（L56-64）：步骤 2 强化导入约定，列出 `use rust_dicore::*` 后可直接用的宏与类型清单
- **§"注册语法参考"表**（L84-92）：示例列统一为简短 `#[inject]` 风格
- **§"当用户使用过程宏"**（L108-116）：示例统一简短风格
- **新增§"场景推荐用法决策矩阵"**（置于"必须遵守的规则"前）：精简版决策路由表，指向 `guides/usage-guide.md`
- **§"常见错误排查"**：补充"路径冗长"条目，引导用 `use rust_dicore::*`

### 7. 新建 `.agents/skills/lrdi-skill/guides/usage-guide.md`（AI agent 决策参考）

紧凑版决策指南，面向 AI agent 生成代码时参考：
- 导入约定模板（固定 use 两行）
- 场景 → 推荐方式路由表（精简，对应 SKILL.md 决策矩阵的展开）
- 构造函数策略决策树
- 简洁性技巧清单（10 条，对应 docs usage-guide §6）
- 完整示例片段

### 8. skill `guides/macros.md` / `reference/api.md` 顶部加导入约定说明

- 两文件顶部加一句："本文示例为明确宏来源使用全限定路径；实际编码推荐 `use rust_dicore::*;` 后用 `#[inject]` 等简短形式，详见 `guides/usage-guide.md`。"
- 存量教学示例保留全限定（教学目的），不强制全量替换（86+20 处工作量过大且教学价值在于明确路径）

## Assumptions & Decisions

1. **路径简化主推 `use rust_dicore::*`**：技术可行（lib.rs:206 导出所有宏），符合 Rust DI 框架惯例。
2. **best-practices 章节示例必须统一**：该章节是推荐用法的权威来源，示例与文字须自洽。03 dyn-trait 作为核心示例一并统一。
3. **macros.md/api.md 保留全限定示例**：参考文档教学价值在于明确宏来源，仅加导入约定提示，不全量替换。
4. **不新建 performance-tips.md**：INDEX 中引用但文件缺失，属历史遗留，本次不修复（超出范围）。
5. **构造函数三种策略并存**：不淘汰任何方式，文档明确各自适用场景，由用户按需选择。
6. **skill guides/usage-guide.md 与 docs usage-guide.md 内容呼应但格式不同**：skill 版紧凑面向 AI agent 路由，docs 版详尽面向人阅读。

## 实施顺序

1. 新建 `docs/rust-dicore/08-best-practices/usage-guide.md`（核心）
2. 更新 `docs/rust-dicore/08-best-practices/INDEX.md` + `docs/rust-dicore/INDEX.md` 导航
3. 统一 best-practices 3 文档示例（anti-patterns、interface-checklist、common-pitfalls）
4. 统一 `03-interface-oriented/dyn-trait-registration.md` 示例
5. 更新 `02-quickstart/INDEX.md` 导入约定
6. 新建 `.agents/skills/lrdi-skill/guides/usage-guide.md`
7. 更新 skill `SKILL.md`（导入约定 + 决策矩阵 + 示例统一）
8. skill `macros.md`/`api.md` 顶部加导入约定提示
9. 验证：grep 确认 best-practices 章节零 `#[rust_dicore::inject` 残留；docs 链接可达

## Verification

1. **路径一致性**：`grep -rn "#\[rust_dicore::inject" docs/rust-dicore/08-best-practices/` → 期望 0 残留（示例已统一为 `#[inject]`）
2. **导航可达**：人工核对 `08-best-practices/INDEX.md`、`docs/rust-dicore/INDEX.md` 中 usage-guide.md 链接
3. **内容完整性**：usage-guide.md 含 7 节，覆盖导入约定、决策矩阵、构造函数策略、简洁性技巧
4. **skill 一致性**：SKILL.md 注册语法参考表与 guides/usage-guide.md 风格一致
5. **编译验证**（可选，因纯文档变更无代码影响）：`cargo build --workspace` 确认无破坏
