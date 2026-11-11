# 参考 / 附录

> 本章是 rust-dix 的**参考与附录**：语义权威页、扩展契约、错误目录、生产部署检查清单。
> 版本：**0.10.0**。

## 本页内容

| 页面 | 用途 |
|------|------|
| [当前版本语义（0.10）](semantics.md) | **唯一权威**的 0.10 容器语义：类型化键/宏 key 表达式/`AnyKey`、async 与环检测、枚举、诊断、MSRV/安全 |
| [扩展契约与边界](extension-points.md) | 官方扩展路径（`From` 本地 key + 宏）、自定义 `IServiceResolver`、显式注册 + 自定义 resolver、**async AnyKey 边界** |
| [RdiError 错误目录](error-catalog.md) | 全部错误变体 + 触发条件 + 处理建议；`is_circular()` 启发式说明 |
| [生产环境部署检查清单](production-checklist.md) | 生产部署逐项检查（原 `docs/production-deployment-checklist.md`，已迁入本章） |

## 使用建议

1. **遇到语义分歧**：以 [当前版本语义（0.10）](semantics.md) 为准；其他章节只保留情境摘要并链接回本页。
2. **需要扩展/自定义 key 或 resolver**：先读 [扩展契约](extension-points.md)；优先 `From` + 现有宏，不要复制注入宏。
3. **排查错误**：先查 [错误目录](error-catalog.md)，再按 service/context/path 定位注册与作用域。
4. **上线前**：逐项执行 [生产部署检查清单](production-checklist.md)。

## 相关章节

- [第一章 · 与 MEDI 的差异清单](../01-introduction/medi-differences.md)（逐条语义对照与迁移速查）
- [第三章 · 多实现与键控服务](../03-interface-oriented/multiple-implementations.md)（typed key / AnyKey 用法）
- [第八章 · 最佳实践](../08-best-practices/INDEX.md)（清单、陷阱与反模式）
- [crates/core README](https://github.com/Rust-Framework/rust-dix/blob/main/crates/core/README.md)（API 参考）
