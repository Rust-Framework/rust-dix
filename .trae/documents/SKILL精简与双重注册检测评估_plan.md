# SKILL.md 精简为领航决策文件 + 双重注册编译时检测可行性评估

## Summary

两个关联请求：
1. **Request F（执行项）**：遵循 Agent Skills 标准规范，将 SKILL.md 从 545 行精简至 ~200 行，仅保留领航/决策/门禁内容。冗余内容（模板、错误排查、规则中的代码块）已迁移至支持文件（recipes.md / troubleshooting.md 均已就位），本次只需重写 SKILL.md 本体。
2. **Request G（结论+文档化）**：评估宏能否编译时检测双重注册。**结论：不可行**，需在文档中记录原因，让未来开发者理解为何依赖门禁 A9/B6 而非编译期检查。

---

## Current State Analysis

### Request F — SKILL.md 当前状态（545 行）

已读取 `d:\GitCode\RF\rust-di\.agents\skills\lrdi-skill\SKILL.md` 全文。冗余分布：

| 行范围 | 内容 | 处置 |
|--------|------|------|
| 1-30 | frontmatter + 强制约束声明 | **保留** |
| 33-47 | 何时激活 | **保留** |
| 51-66 | 支持文件索引 | **保留+增强**（补 troubleshooting.md 条目） |
| 70-132 | 核心工作流程（5 小节） | **精简**为 6 条路由要点 |
| 98-107 | 注册语法参考表 | **删除**（与决策矩阵重复） |
| 136-163 | 场景用法决策矩阵 + 构造函数决策树 + 简洁性规则 | **保留** |
| 166-363 | 必须遵守的规则 0-9（含大量代码块） | **精简**：每条规则变一行声明 + `详见` 引用 |
| 367-401 | 输出代码前必检清单（门禁 A/B/C） | **保留** |
| 405-495 | 常见任务模板（4 个） | **删除**（已迁至 recipes.md） |
| 499-510 | 常见错误排查（8 条） | **删除**（已迁至 troubleshooting.md） |
| 514-545 | 处理流程总结 | **保留**（微调：门禁 B 项数 5→6） |

**支持文件就位情况（已确认）**：
- `patterns/recipes.md`（190 行）：已含"快速起步模板"4 个 + 端到端示例 6 个 ✅
- `guides/troubleshooting.md`（98 行）：已含 8 条错误排查（现象→原因→修正） ✅

### Request G — 宏架构与双重注册可行性分析

已读取 `crates/macros/src/lib.rs` 全文 + `crates/core/src/registration.rs` + `crates/core/src/collection.rs:120-168` + `crates/core/tests/inject_attribute.rs:122-131`。

**三条不可行理由**：

1. **技术层面——过程宏跨调用无状态**
   - `#[proc_macro_attribute] pub fn inject(...)` 每次调用独立（lib.rs:452-460）
   - struct 上的 `#[inject]` 与 impl 上的 `#[inject]` 是两次独立调用，彼此看不见对方
   - 过程宏无共享静态状态、无 build script 介入点，Rust 过程宏按设计即无状态

2. **类型层面——TypeId 不冲突**
   - struct 放置：`type_id: TypeId::of::<#name>()`（lib.rs:533）→ 具体类型 TypeId
   - impl 放置：`type_id: TypeId::of::<dyn #trait_path>()`（lib.rs:629）→ trait 对象 TypeId
   - 两者 TypeId 不同 → 在 `ServiceStore = HashMap<TypeId, Vec<ServiceEntry>>` 中落入不同桶 → 无运行时冲突可检测
   - `from_injected()`（collection.rs:135-148）遍历 `inventory::iter` 全部 push，**无去重、无冲突检测**

3. **设计层面——双重注册是被支持的特性**
   - 测试 `inject_combined_concrete_and_trait`（inject_attribute.rs:122-131）明确验证：同一 struct 同时注册为具体类型 + `dyn IThingB`，两者均可解析
   - 注释 `// Concrete is also usable as the trait` 表明这是**有意设计**，非 bug
   - 加检测会破坏向后兼容，且与"罕见但确需同时暴露"的合法用例冲突（见 SKILL.md 规则 9 放置决策表第三行）

**结论**：编译时检测不可行。双重注册的防范依赖**文档约束 + 门禁 A9/B6**（已在 SKILL.md 中实现）。

---

## Proposed Changes

### Change 1: 重写 SKILL.md 为精简领航文件（Request F 主体）

**文件**：`d:\GitCode\RF\rust-di\.agents\skills\lrdi-skill\SKILL.md`

**目标**：545 行 → ≤220 行，仅含领航/决策/门禁。

**保留段落（原样或微调）**：
- YAML frontmatter（1-9 行）
- 强制约束声明（16-29 行）—— 微调：门禁 B 项数 5→6
- 何时激活（33-47 行）
- 支持文件索引（51-66 行）—— 增强：补 `guides/troubleshooting.md` 条目，更新 `patterns/recipes.md` 描述
- 场景用法决策矩阵（136-163 行）
- 输出代码前必检清单·门禁 A/B/C（367-401 行）
- 处理流程总结（514-545 行）—— 微调：门禁 B 项数 5→6

**精简段落**：

1. **核心工作流程**（原 70-132 行，5 小节）→ 6 条路由要点：
   ```
   - 添加 LRDI 到项目 → Cargo.toml 加 `rust-dicore="0.4"` + 文件顶部 `use rust_dicore::*;`（详见 guides/usage-guide.md §导入约定）
   - 注册服务 → 查"场景用法决策矩阵"选方式（详见 guides/usage-guide.md §决策矩阵）
   - 解析服务 → `provider.get::<T>()` / `get_optional` / `get_keyed` / `get_all`（详见 reference/api.md）
   - 设计架构 → 加载 guides/architecture.md，按层选生命周期
   - 使用过程宏 → 加载 guides/macros.md，按构造函数决策树选标注方式
   - 设计模式/插件 → 加载 patterns/design-patterns.md 或 guides/plugin-system.md
   ```

2. **必须遵守的规则 0-9**（原 166-363 行，含大量代码块）→ 每条一行声明 + `详见` 引用：
   - 规则 0：面向接口开发，依赖 `dyn Trait` 而非具体类型；struct 用 `#[derive(Inject)]` + impl 用 `#[inject]` 注册为 trait。详见 `guides/usage-guide.md` §面向接口、`guides/macros.md` §2.1。
   - 规则 1：注册类型 = 解析类型（`TypeId` 必须一致）。详见 `guides/troubleshooting.md` §1。
   - 规则 2：ServiceProvider 构建一次，`Arc` 全局持有。详见 `guides/architecture.md` §生命周期。
   - 规则 3：不传递 ServiceProvider，入口处解析依赖注入构造函数。详见 `patterns/anti-patterns.md` §反模式 5。
   - 规则 4：生命周期选择——全局共享→Singleton、请求级→Scoped、无状态→Transient；禁止 Singleton 依赖 Scoped（build 阶段拒绝）。详见 `guides/architecture.md` §生命周期决策树。
   - 规则 5：测试中用 mock 实现替换真实服务。详见 `guides/testing.md`。
   - 规则 6：禁止全限定路径，文件顶部 `use rust_dicore::*;`。详见 `guides/usage-guide.md` §导入约定。
   - 规则 7：注册方式按决策矩阵路由。详见 `guides/usage-guide.md` §决策矩阵。
   - 规则 8：字段注入必须显式标记（`#[inject]`/`#[inject(owned)]`/`#[inject(provider)]`）；未标记字段走 `Default`。详见 `guides/macros.md` §字段属性、`guides/usage-guide.md` §字段注入标记速查。
   - 规则 9：`#[inject]` 放置二选一——struct 与 impl 不可同时用；面向接口时 struct 用 `#[derive(Inject)]`，impl 用 `#[inject]`；Handler 推荐放 impl 上。宏**无法**编译时检测双重注册（见下文 §双重注册检测限制），依赖门禁 A9/B6 把关。详见 `guides/usage-guide.md` §放置位置决策、`patterns/anti-patterns.md` §反模式 10。

**删除段落**：
- 注册语法参考表（98-107 行）—— 与决策矩阵重复
- 规则 0-9 中的所有代码块 —— 迁至引用的 guide 文件
- 常见任务模板（405-495 行）—— 已迁至 `patterns/recipes.md`
- 常见错误排查（499-510 行）—— 已迁至 `guides/troubleshooting.md`

**新增段落（小型）**：
- 在规则 9 后追加一段简短说明"双重注册检测限制"（3-4 行），解释宏无法编译时检测的三条理由（技术无状态/TypeId 不冲突/被支持特性），指向 `patterns/anti-patterns.md` §反模式 10 的详细说明。

### Change 2: 在 anti-patterns.md 反模式 10 补充"为何宏无法检测"小节（Request G 文档化）

**文件**：`d:\GitCode\RF\rust-di\docs\rust-dicore\08-best-practices\anti-patterns.md`

**位置**：反模式 10 末尾（当前第 400 行 `> 详见 ...` 之前）

**新增内容**（约 15 行）：
```markdown
### 为什么宏无法编译时检测双重注册？

双重注册只能靠门禁 A9/B6 在代码生成阶段拦截，过程宏本身**无法**编译时检测，原因有三：

1. **过程宏跨调用无状态**：`#[inject]` 标在 struct 上和标在 impl 上是两次独立的宏调用，彼此看不见对方，Rust 过程宏按设计即无共享状态。
2. **TypeId 不冲突**：struct 放置注册为 `TypeId::of::<T>()`（具体类型），impl 放置注册为 `TypeId::of::<dyn Trait>()`（trait 对象）—— 两者 TypeId 不同，在 `ServiceStore` 中落入不同桶，无运行时冲突可检测。
3. **双重注册是被支持的特性**：测试 `inject_combined_concrete_and_trait` 明确验证同一 struct 同时注册为具体类型 + `dyn Trait` 两者均可解析（"Concrete is also usable as the trait"）。加检测会破坏此合法用例（罕见但确需同时暴露）。

因此，双重注册的防范完全依赖**文档约束 + 门禁 A9/B6**，而非编译期检查。
```

### Change 3: 在 common-pitfalls.md 陷阱 10 补充同样说明（交叉引用）

**文件**：`d:\GitCode\RF\rust-di\docs\rust-dicore\08-best-practices\common-pitfalls.md`

**位置**：陷阱 10 末尾

**新增内容**（约 3 行，精简版 + 交叉引用）：
```markdown
> **为何宏无法编译时检测？** 过程宏跨调用无状态、struct 与 impl 的 TypeId 不冲突、且双重注册是被支持的特性（见 `inject_combined_concrete_and_trait` 测试）。防范完全依赖门禁 A9/B6。详见 [反模式 10](anti-patterns.md#为什么宏无法编译时检测双重注册)。
```

---

## Assumptions & Decisions

1. **假设**：recipes.md 和 troubleshooting.md 内容已就位且无需改动（Phase 1 已确认：190 行 / 98 行）。
2. **决策**：规则 0-9 保留为"一行声明 + 详见引用"形式，不删除规则本身——门禁 A/B 引用了规则编号，删除会断链。
3. **决策**：双重注册检测限制的详细说明放在 anti-patterns.md 反模式 10（用户面向），SKILL.md 规则 9 仅放 3-4 行精简说明 + 指向反模式 10。common-pitfalls.md 陷阱 10 放精简版 + 交叉引用。
4. **决策**：不修改宏源码（lib.rs）—— Request G 是评估问题，结论是"不可行"，无需代码变更。
5. **假设**：用户认可"双重注册是被支持特性"这一现状，不要求改造宏去主动拒绝双重注册（如要求，需另开任务且会破坏向后兼容）。

---

## Verification Steps

1. **行数校验**：SKILL.md ≤ 220 行（`Select-String` 统计行数）
2. **无完整代码块校验**：SKILL.md 规则 0-9 中无 ` ```rust ` 代码块（仅决策表/门禁表保留）
3. **规则引用完整性**：每条规则 0-9 都有 `详见` 引用指向存在的 guide 文件
4. **索引完整性**：支持文件索引含 `guides/troubleshooting.md` 条目
5. **门禁项数一致**：强制约束声明、门禁 B、处理流程总结三处的门禁 B 项数统一为 6
6. **双重注册说明存在**：anti-patterns.md 反模式 10 含"为什么宏无法编译时检测"小节；common-pitfalls.md 陷阱 10 含交叉引用；SKILL.md 规则 9 含精简说明
7. **编译验证**：`cargo build --workspace` 通过（文档变更不影响编译，但确认无副作用）
8. **测试验证**：`cargo test --workspace` 0 失败（确认 `inject_combined_concrete_and_trait` 仍通过——它是"被支持特性"的证据）
