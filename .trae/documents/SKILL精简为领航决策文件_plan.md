# SKILL.md 精简为领航决策文件

## Context

当前 SKILL.md 有 545 行，包含大量代码示例、4 个完整模板、错误排查表等参考内容。按 Agent Skills 标准规范，SKILL.md 主职责是**领航决策**——路由到支持文件、强制规则声明、门禁检查，不应承载可按需加载的参考内容。冗余内容分散 LLM 注意力，且与 guide 文件重复维护成本高。

目标：SKILL.md 从 545 行精简到 ~200 行，只保留导航/决策/门禁，详细内容下沉到 guide 文件按需加载。

## 改动方案

### 1. SKILL.md 保留内容（精简后 ~200 行）

| 章节 | 处理 | 说明 |
|------|------|------|
| YAML frontmatter | 保留 | 不变 |
| 强制约束声明 | 保留 | 硬性约束入口，不改 |
| 何时激活此技能 | 保留 | 激活条件，不改 |
| 支持文件索引 | 保留+增强 | 新增 troubleshooting.md 条目 |
| 核心工作流程 | **精简** | 去掉代码示例和详细步骤，只保留路由指令（"查决策矩阵→加载X文件→选方式"） |
| 注册语法参考表 | **删除** | 与 usage-guide.md §2 路由表重复 |
| 场景用法决策矩阵 | 保留 | 这是决策路由核心，不改 |
| 必须遵守的规则 0-9 | **精简** | 每条规则缩为一行声明+引导文件引用，删除所有代码块和决策表 |
| 输出代码前必检清单 | 保留 | 门禁是决策核心，不改 |
| 常见任务模板（4个） | **移出** | 移到 patterns/recipes.md |
| 常见错误排查 | **移出** | 移到新建 guides/troubleshooting.md |
| 处理流程总结 | 保留 | 精简流程图引用 |

### 2. 规则精简示例

**改前（规则 0，27 行含代码）：**
```markdown
### 规则 0：面向接口开发（最重要）

```rust
// ✅ 正确：依赖 trait，注册为 trait
trait IUserService: Send + Sync { ... }
#[derive(Inject)]
struct UserService { #[inject] db: Arc<dyn IDbContext> }
#[inject] impl IUserService for UserService { ... }
// Handler 依赖接口
#[inject(transient)]
struct CreateUserHandler { #[inject] user_svc: Arc<dyn IUserService> }
// ❌ 错误：依赖具体类型
struct CreateUserHandler { user_svc: Arc<UserService> }
```

**contracts 层**：放 trait 定义 + DTO
**handlers 层**：放 trait 实现 + #[inject]
```

**改后（规则 0，3 行）：**
```markdown
### 规则 0：面向接口开发（最重要）
依赖 `dyn Trait` 而非具体类型；struct 用 `#[derive(Inject)]` + impl 用 `#[inject]` 注册为 trait。详见 `guides/usage-guide.md` §放置位置决策、`guides/macros.md` §2.1。
```

每条规则统一为：**一行规则声明 + 详见 `guide文件` §章节**。

### 3. 内容移出目标

| 移出内容 | 目标文件 | 说明 |
|---------|---------|------|
| 4 个常见任务模板 | `patterns/recipes.md` | recipes.md 当前仅 61 行骨架，模板作为"快速起步模板"章节填充，与端到端示例互补 |
| 8 条常见错误排查 | 新建 `guides/troubleshooting.md` | 独立排错指南，SKILL.md 索引指向它 |

### 4. 支持文件索引增强

索引表新增一行：
```
| `guides/troubleshooting.md` | 用户遇到错误/panic/编译失败 | 8 条常见错误排查顺序、现象/原因/修正 |
| `patterns/recipes.md` | 用户要完整示例或快速起步模板 | 4 个起步模板（最小项目/策略模式/Web请求/Mock测试）+ 端到端示例骨架 |
```

### 5. 核心工作流程精简

**改前**（5 个子章节，~65 行，含详细步骤和代码）：
```
### 当用户要求"添加 LRDI 到我的项目"
1. 检查 Cargo.toml...
2. 在每个使用 DI 的文件顶部添加 use rust_dicore::*...
3. 构建 ServiceProvider：...
4. 在需要服务的地方通过 provider.get::<T>()...

### 当用户要注册服务
1. 确认服务类型...
2. **首选方案**：使用 #[inject]...
3. **备选方案**：手动 ServiceCollection...
4. **关键规则**：注册时的 T 类型...
```

**改后**（6 行路由指令）：
```
### 核心工作流程（路由）
- "添加 LRDI 到项目" → `Cargo.toml` 加 `rust-dicore = "0.4"` + 文件顶部 `use rust_dicore::*;` + 构建一次 provider
- "注册服务" → 查下方决策矩阵 → 选方式 → 参考 `guides/usage-guide.md`
- "解析服务" → `provider.get::<T>()` → 参考 `reference/api.md` 解析 API
- "设计架构" → 加载 `guides/architecture.md`
- "使用过程宏" → 加载 `guides/macros.md`（注意规则 9 放置二选一）
- "遇到错误" → 加载 `guides/troubleshooting.md`
```

## 涉及文件

| 文件 | 操作 |
|------|------|
| `.agents/skills/lrdi-skill/SKILL.md` | 精简：545→~200 行 |
| `.agents/skills/lrdi-skill/guides/troubleshooting.md` | **新建**：移入 8 条错误排查 |
| `.agents/skills/lrdi-skill/patterns/recipes.md` | 编辑：移入 4 个起步模板 |

## 验证

1. 检查 SKILL.md 行数 ≤ 220
2. 检查 SKILL.md 中无完整代码块（门禁表和决策矩阵中的表格/片段除外）
3. 检查每条规则有 `详见` 引用指向 guide 文件
4. 检查 `guides/troubleshooting.md` 和 `patterns/recipes.md` 内容完整
5. 检查支持文件索引表含 troubleshooting.md 条目
6. `cargo build --workspace` 通过（不涉及代码改动，仅文档）
