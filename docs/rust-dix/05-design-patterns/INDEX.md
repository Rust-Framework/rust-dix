# 第五章 设计模式实践

本章展示如何用 rust-dix 实现常见设计模式。

## 本章小节

| 小节 | 内容 |
|------|------|
| 策略模式：keyed 服务（规划中） | 运行时选择实现 |
| 工厂模式：闭包工厂（规划中） | 动态创建服务 |
| 装饰器模式：链式包装（规划中） | 增强服务功能 |
| 观察者模式：get_all 广播（规划中） | 事件通知 |
| 责任链模式：处理器链（规划中） | 逐级处理请求 |
| 更多模式速查（规划中） | 12 种模式速查表 |

> 具体模式小节页尚未发布；下方"设计模式与 DI / 核心机制"为章节页内置速查。

## 设计模式与 DI

依赖注入天然支持多种设计模式：

| 设计模式 | rust-dix 实现 |
|---------|-----------------|
| 策略模式 | keyed（字符串或类型化 `ServiceKey`）+ get_keyed |
| 工厂模式 | 闭包工厂或 keyed 工厂；任意 key 场景用 `keyed_any_*` 兜底 |
| 装饰器模式 | 工厂闭包链式包装 |
| 观察者模式 | 非 keyed 多实现 + `get_all`（keyed 实现用 `get_all_keyed`） |
| 责任链模式 | keyed + 配置驱动顺序 |
| 适配器模式 | singleton::<dyn Trait> 包装第三方类型 |
| 代理模式 | 闭包 + 持有 Provider 懒加载 |
| 组合模式 | keyed + 运行时组装树 |

## 核心机制

所有行为型模式的基础：非 keyed 多实现用 `get_all` 广播，keyed 实现按 key 单取或用
`get_all_keyed` 批量（不含 `AnyKey` 兜底）。单服务解析遵循"最后注册优先"。

```rust
// 广播目标：同一 trait 的非 keyed 多实现
let all: Vec<Arc<dyn Handler>> = provider.get_all();
// 策略/责任链：按 key 取（字符串或 ServiceKey::new(enum)）
let one: Arc<dyn Handler> = provider.get_keyed("retry").unwrap();
```

> 完整语义（类型化键/AnyKey 缓存/async 枚举）见
> [语义权威页（0.10）](../09-reference/semantics.md) 与
> [扩展契约](../09-reference/extension-points.md)（宏 key 表达式/下游 key/async AnyKey 边界）；
> 模式小节仍规划中。

## 下一步

模式小节页规划中；策略/多实现的现行用法可先阅读
[多实现与键控服务](../03-interface-oriented/multiple-implementations.md)，
设计模式总览见 [用法指南](../08-best-practices/usage-guide.md)。