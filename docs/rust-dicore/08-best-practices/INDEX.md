# 第八章 最佳实践

本章汇总 rust-dicore 开发中的最佳实践、常见陷阱和反模式警示。

## 本章小节

| 小节 | 内容 |
|------|------|
| [用法指南与决策矩阵](usage-guide.md) | 导入约定、场景推荐、简洁性技巧（首选阅读） |
| [业务场景实战](business-scenarios.md) | Web 应用、电商、多租户、插件系统、TDD 完整示例 |
| [面向接口开发清单](interface-checklist.md) | 核心规则速查 |
| [常见陷阱与排查](common-pitfalls.md) | 高频错误 |
| [性能优化建议](performance-tips.md) | 性能建议 |
| [反模式警示](anti-patterns.md) | 错误模式与修正 |

## 核心原则速查

### 面向接口开发

| 规则 | 说明 |
|------|------|
| 依赖 `Arc<dyn Trait>` | 不依赖具体类型 |
| 注册为 `dyn Trait` | `singleton::<dyn Trait>(...)` |
| contracts 层放 trait | handlers 层放实现 |
| keyed 服务实现策略 | `get_keyed` 获取指定实现 |

### 生命周期选择

| 场景 | 生命周期 |
|------|---------|
| 全局配置、连接池 | Singleton |
| 请求上下文、事务 | Scoped |
| 无状态轻量服务 | Transient |
| 禁止 Singleton 依赖 Scoped | build 阶段检测并拒绝（`RdiError::SingletonDependsOnScoped`） |

### DI 容器使用

| 规则 | 说明 |
|------|------|
| 构建一次，Arc 持有 | 不要每次重建 |
| 组合根集中配置 | 不传递 Provider 到处使用 |
| 测试替换实现 | 改一行注册换 mock |

## 下一步

从 [用法指南与决策矩阵](usage-guide.md) 开始，建立导入约定与场景选择的整体认知。