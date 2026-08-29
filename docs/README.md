# rust-dix 文档

本目录为 **rust-dix 开发者手册：面向接口开发的 Rust DI 框架** 的 canonical 源，覆盖 DI 容器的三种生命周期、键控服务、过程宏与跨 DLL 插件等主题。

## 结构

```
docs/
└── rust-dix/            # 文档根，对应 /api/docs/rust-dix/*
    ├── FOREWORD.md       # 前言
    ├── INDEX.md          # 全书目录
    ├── INDEX.json        # 文档网站左侧菜单
    └── 01-introduction/  # 章节（共 8 章）
        ├── 02-quickstart/         # 快速上手
        ├── 03-interface-oriented/ # 面向接口编程范式
        ├── 04-di-patterns/        # 服务注册与解析
        ├── 05-design-patterns/    # 设计模式实践
        ├── 06-testing/            # 测试策略
        ├── 07-advanced/           # 高级特性
        └── 08-best-practices/     # 最佳实践
```

## 阅读

- [前言](rust-dix/FOREWORD.md) — 了解本书定位、读者画像与阅读路径
- [全书目录](rust-dix/INDEX.md)

## 阅读建议

- **快速入门**：从[第 2 章 快速上手](rust-dix/02-quickstart/)开始，掌握第一个 DI 容器与生命周期选择
- **设计模式**：重点阅读[第 5 章 设计模式实践](rust-dix/05-design-patterns/)，涵盖策略 / 工厂 / 装饰器等模式落地
- **最佳实践**：参考[第 8 章 最佳实践](rust-dix/08-best-practices/)，含面向接口开发清单与常见陷阱排查

## 维护

编辑 `docs/rust-dix/` 下的 Markdown 即可；Docbit 启动时会自动确保 `INDEX.json` 存在。