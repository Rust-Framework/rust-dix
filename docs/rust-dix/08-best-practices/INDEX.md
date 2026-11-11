# 第八章 最佳实践

本章汇总 rust-dix 开发中的最佳实践、常见陷阱和反模式警示。

## 本章小节

| 小节 | 内容 |
|------|------|
| [用法指南与决策矩阵](usage-guide.md) | 导入约定、场景推荐、简洁性技巧（首选阅读） |
| [业务场景实战](business-scenarios.md) | Web 应用、电商、多租户、插件系统、TDD、后台任务完整示例 |
| [面向接口开发清单](interface-checklist.md) | 核心规则速查 |
| [常见陷阱与排查](common-pitfalls.md) | 高频错误 |
| 性能优化建议（规划中） | 性能建议 |
| [反模式警示](anti-patterns.md) | 错误模式与修正 |

## 核心原则速查

### 面向接口开发

| 规则 | 说明 |
|------|------|
| 依赖 `Arc<dyn Trait>` | 不依赖具体类型 |
| 注册为 `dyn Trait` | `singleton::<dyn Trait>(...)` |
| contracts 层放 trait | handlers 层放实现 |
| keyed 服务实现策略 | `get_keyed` 获取指定实现（key 可为字符串或类型化 `ServiceKey`） |
| 任意 key 兜底 | `keyed_any_*`（精确 key 优先、按 (服务注册/类型, 请求 key) 缓存、枚举不含兜底） |

### 生命周期选择

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池 | Singleton |
| 请求上下文、事务 | Scoped |
| 无状态轻量服务 | Transient |
| 禁止 Singleton 依赖 Scoped | build 阶段检测并拒绝（`RdiError::SingletonDependsOnScoped`） |
| 禁止同步 Singleton 依赖 async 服务 | build 阶段 `RdiError::SyncSingletonDependsOnAsyncService`；依赖方改 async 或延迟解析 |

### DI 容器使用

| 规则 | 说明 |
|------|------|
| 构建一次，Arc 持有 | 不要每次重建 |
| 组合根集中配置 | 不把 Provider 当服务定位器到处传递 |
| 后台任务按 tick 建 scope | Singleton 注入 `Arc<dyn ScopeFactory>`（内置、弱引用），不持有 scoped 实例 |
| 测试替换实现 | 改一行注册换 mock（**最后注册优先**，后注册的覆盖先注册） |

### 0.9.x 语义速查

| 规则 | 说明 |
|------|------|
| 单服务解析 | `get` / `get_keyed` / `get_owned` / `get_async` 为**最后注册优先** |
| 多实现收集 | `get_all` / `#[inject] Vec<Arc<T>>` **仅非 keyed**；keyed 用 `get_all_keyed`（不含 `AnyKey` 兜底）；两者含已初始化 async singleton，同步枚举未初始化 async singleton/transient → `AsyncServiceRequiresGetAsync`（async scoped 在 root 被 scope 校验省略） |
| 类型化键 / AnyKey | keyed API 接受 `impl Into<ServiceKey>`；`keyed_any_*` 兜底（精确 key 优先、按 (服务注册/类型, 请求 key) 缓存、单值 `any_key()` 非法）；validation 期消费的 singleton 预填充复用；AnyKey owned 在 root 允许；嵌套不同 key 合法；`ServiceKey::new("x")` ≠ `String` key |
| async 解析/诊断 | 同步解析 async 服务报 `AsyncServiceRequiresGetAsync`；同步 singleton 不能依赖 async 服务；跨任务传 `AsyncFactoryContext`，环返回 `CircularDependency`；`is_circular()` 为启发式 |
| Scope 校验 | 默认严格：root 解析 Scoped 返回 `Err(ScopedFromRootProvider)`；在 `provider.scope()` 内解析（`get_owned`/`try_get_owned` 例外：owned 不缓存，可从 root 取） |
| 旧行为兼容 | `build_with(ServiceProviderOptions::lenient())` 恢复 0.7.x root-scope 语义 |
| 释放 | `Scope::dispose()` / `ServiceProvider::dispose()` 按创建逆序；dispose 后解析返回 `ScopeDisposed` / `ProviderDisposed` |
| 构建顺序 | **同步 singleton 按注册顺序创建；async singleton 按依赖优先拓扑序递归初始化**，两者均确定，不依赖 `HashMap` 顺序 |
| 工程基线 | MSRV **1.95**、`#![forbid(unsafe_code)]`、CI matrix + RustSec `cargo audit`、`SECURITY.md` |

> 逐条差异与迁移见 [与 MEDI 的差异清单](../01-introduction/medi-differences.md)。

## 下一步

从 [用法指南与决策矩阵](usage-guide.md) 开始，建立导入约定与场景选择的整体认知。