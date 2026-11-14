# rust-dix 文档

本目录为 **rust-dix 开发者手册：面向接口开发的 Rust 依赖注入（DI）框架**（当前对应 **1.0.0**）的 canonical 源，覆盖 DI 容器的三种生命周期、类型化键控服务与 `AnyKey`、宏 key 表达式、异步环检测、过程宏与参考/附录。

## 结构

```
docs/
├── README.md                    # 本文件
└── rust-dix/                    # 文档根，对应 /api/docs/rust-dix/*
    ├── FOREWORD.md              # 前言
    ├── INDEX.md                 # 全书目录
    ├── INDEX.json               # 文档网站左侧菜单
    ├── 01-introduction/         # 第一章 认识 rust-dix（已发布 medi-differences）
    ├── 02-quickstart/           # 第二章 快速上手（章节页已发布，小节规划中）
    ├── 03-interface-oriented/   # 第三章 面向接口编程范式（已发布 2 页）
    ├── 05-design-patterns/      # 第五章 设计模式实践（章节页已发布，小节规划中）
    ├── 08-best-practices/       # 第八章 最佳实践（已发布 6 页）
    └── 09-reference/            # 附录 · 参考（语义权威页（0.10） / 扩展契约 / 错误目录 / 生产清单）
        ├── INDEX.md
        ├── semantics.md
        ├── extension-points.md
        ├── error-catalog.md
        └── production-checklist.md
```

> **发布状态**：全书规划 9 章，当前**已发布 6 章**（01、02、03、05、08、09）。
> 第 4、6、7 章（服务注册与解析 / 测试策略 / 高级特性）**规划中**，目录页不提供链接；
> 已发布章节中的部分小节同样仍在规划中。`docs/rust-dix/INDEX.md` 只链接已发布页面，
> `INDEX.json` 也只登记已发布页面。

## 附加文档

- [参考章 · 当前版本语义（0.10）](rust-dix/09-reference/semantics.md) — 语义冲突时以本页为准
- [扩展契约与边界](rust-dix/09-reference/extension-points.md) — 本地 key `From` + 宏、自定义 resolver、async AnyKey 边界
- [RdiError 错误目录](rust-dix/09-reference/error-catalog.md) — 错误变体与处理建议
- [生产环境部署检查清单](rust-dix/09-reference/production-checklist.md) — 上线逐项检查（原 `docs/production-deployment-checklist.md`，已迁入参考章）
- [与 MEDI 的差异清单](rust-dix/01-introduction/medi-differences.md) — .NET 迁移必读
- [开发环境与磁盘占用治理（target/）](DEVELOPMENT.md) — profile 调优、清理脚本与实测数据
- [变更日志](../CHANGELOG.md) / [安全策略](../SECURITY.md) — 版本历史与漏洞上报

## 阅读

- [前言](rust-dix/FOREWORD.md) — 本书定位、读者画像与阅读路径
- [全书目录](rust-dix/INDEX.md)
- [与 MEDI 的差异清单](rust-dix/01-introduction/medi-differences.md) — scope 校验、dispose、keyed、get_all、async 枚举逐条对照

## 阅读建议

- **快速入门**：从[第二章 快速上手](rust-dix/02-quickstart/INDEX.md)开始，掌握第一个 DI 容器与生命周期选择
- **设计模式**：阅读[第五章 设计模式实践](rust-dix/05-design-patterns/INDEX.md)，了解策略/工厂/装饰器等模式落地
- **最佳实践**：参考[第八章 最佳实践](rust-dix/08-best-practices/INDEX.md)（清单、陷阱、反模式）
- **排障/上线**：先查[参考章](rust-dix/09-reference/INDEX.md)的错误目录与生产检查清单

## 维护

编辑 `docs/rust-dix/` 下的 Markdown 即可；Docbit 启动时会自动确保 `INDEX.json` 存在。
新增页面需同时更新 `docs/rust-dix/INDEX.md`、`INDEX.json`（仅登记已发布页）与本文件的目录树。
