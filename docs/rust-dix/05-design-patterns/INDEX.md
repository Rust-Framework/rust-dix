# 第五章 设计模式实践

本章展示如何用 rust-dix 实现常见设计模式。

## 本章小节

| 小节 | 内容 |
|------|------|
| [策略模式：keyed 服务](strategy-pattern.md) | 运行时选择实现 |
| [工厂模式：闭包工厂](factory-pattern.md) | 动态创建服务 |
| [装饰器模式：链式包装](decorator-pattern.md) | 增强服务功能 |
| [观察者模式：get_all 广播](observer-pattern.md) | 事件通知 |
| [责任链模式：处理器链](chain-pattern.md) | 逐级处理请求 |
| [更多模式速查](pattern-reference.md) | 12 种模式速查表 |

## 设计模式与 DI

依赖注入天然支持多种设计模式：

| 设计模式 | rust-dix 实现 |
|---------|-----------------|
| 策略模式 | keyed + get_keyed |
| 工厂模式 | 闭包工厂或 keyed 工厂 |
| 装饰器模式 | 工厂闭包链式包装 |
| 观察者模式 | keyed + get_all |
| 责任链模式 | keyed + 配置驱动顺序 |
| 适配器模式 | singleton::<dyn Trait> 包装第三方类型 |
| 代理模式 | 闭包 + 持有 Provider 懒加载 |
| 组合模式 | keyed + 运行时组装树 |

## 核心机制

所有行为型模式的基础：

```rust
// 多实现注册
ServiceCollection::new()
    .keyed_singleton::<dyn Handler>("h1", |_| Arc::new(Handler1))
    .keyed_singleton::<dyn Handler>("h2", |_| Arc::new(Handler2))
    .keyed_singleton::<dyn Handler>("h3", |_| Arc::new(Handler3))
    .build()
    .unwrap();

// 按名称获取
let h1: Arc<dyn Handler> = provider.get_keyed("h1");

// 批量获取
let all: Vec<Arc<dyn Handler>> = provider.get_all();
```

## 下一步

从 [策略模式：keyed 服务](strategy-pattern.md) 开始。