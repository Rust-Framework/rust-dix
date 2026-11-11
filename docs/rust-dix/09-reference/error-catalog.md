# 参考 · RdiError 错误目录

> 集中列出 `RdiError` 变体、触发条件与处理建议。判错/日志/告警以此为速查；
> 语义背景见 [语义权威页（0.10）](semantics.md)。
>
> `RdiError` 为 `#[non_exhaustive]`：新增变体不破坏下游 `match`，但匹配处需保留 `_ =>` 分支。

## 变体速查

| 变体 | 触发条件 | 处理建议 |
|------|----------|----------|
| `ServiceNotFound { ty, context }` | 非 keyed 服务未注册（`get` / `get_owned` 等 required API） | 检查注册类型是否与解析类型一致（`singleton::<dyn Trait>` vs 具体类型）；`context` 含解析链时用于定位上游依赖 |
| `KeyedServiceNotFound { key, ty, .. }` | keyed 查询无精确注册且无 `AnyKey` 兜底 | 检查 key 类型/值：`ServiceKey::new("x")`（`&str`）≠ 字符串注册的 `String` key，字符串键用 `ServiceKey::from("x")` / `"x".into()`；确认是否需要 `keyed_any_*` 兜底。0.10 宏 key 可传 typed key/`From` 下游 key，注意与注册侧 key 类型一致 |
| `CircularDependency(path)` | 同步解析栈或 async 解析链检测到自引用/交叉递归 | 重构依赖，或改用 `Arc<dyn ScopeFactory>` / 延迟解析；不要 `.unwrap()` 后忽略 |
| `SingletonDependsOnScoped { singleton, scoped }` | build 期 captive dependency 检测：Singleton 直接/间接依赖 Scoped | Singleton 只依赖 Singleton/Transient；Scoped 依赖改为每 scope 解析 |
| `FactoryPanic { service, payload }` | 工厂 panic 被捕获并转为 typed 错误（用户工厂 panic/`unwrap`/`expect`） | 修复工厂；payload 含原始信息。环被 `unwrap` 时 payload 含 `CircularDependency`，可用 `is_circular()` 判定 |
| `OwnedNotSupported(type_name)` | 对 Singleton 调 `get_owned`；或对 Singleton 调 `get_keyed_owned`/keyed owned | 用 `try_get_owned`（返回 `None`），或改为 Transient/Scoped；共享实例用 `get` |
| `AsyncServiceRequiresBuildAsync { service }` | 集合含 `async_*` 注册却调用 `build()`（**仅此场景**） | 改用 `build_async()` / `build_async_with(options)` |
| `AsyncServiceRequiresGetAsync { service }` | 同步解析**未初始化**的 async 服务：`get`（`get_optional` 返回 `None`）；同步枚举未初始化 async singleton/transient（`get_all` typed payload panic、`get_all_any_result`/`Vec` 注入返回 `Err`） | 改用 `get_async` / `get_keyed_async`；async scoped/transient 逐项解析 |
| `SyncSingletonDependsOnAsyncService { singleton, async_service }` | build 期同步 singleton 依赖 async 服务 | 把依赖方也改为 async singleton，或在方法内用 `Arc<dyn ScopeFactory>` / `get_async` 延迟解析 |
| `ScopedFromRootProvider { service }` | strict scope 校验（默认）：从 root provider 解析 Scoped（直接/经 Transient 链）；`get_optional`/`try_*` 视为 `None`，`get_all*` 省略 | 在 `provider.scope()` 内解析；确需 0.7.x 行为用 `build_with(ServiceProviderOptions::lenient())`。**owned 例外**：`get_owned`/`try_get_owned`（含 keyed，含 AnyKey scoped/transient）允许从 root 取，实例不缓存/不跟踪，但传递依赖仍受校验 |
| `ProviderDisposed` | provider 已 `dispose()` 后继续解析 | 应用关闭时先停 worker/请求，再 dispose；不要把已释放容器继续传递 |
| `ScopeDisposed` | scope 已 `dispose()`（或 Drop）后继续解析 | 每个 scope 生命周期内使用；不要缓存 scope 或其服务跨请求复用 |

## `is_circular()`（启发式）

```rust
if err.is_circular() {
    // 直接 CircularDependency，或 FactoryPanic payload 含 "CircularDependency"
}
```

- **启发式**：`FactoryPanic { payload }` 只要 payload 含 `CircularDependency` 文本即命中，
  用户自定义 payload 可能误判；需要精确形态时优先匹配 typed 变体
  （`matches!(err, RdiError::CircularDependency(_))`）。
- 常见场景：嵌套 `get_async` 得到 typed `CircularDependency`；上层工厂 `.unwrap()` 后
  顶层 `build_async` 看到的是 `FactoryPanic`，此时 `is_circular()` 可同时识别。

## 错误映射与日志建议

| 场景 | 建议 |
|------|------|
| required 解析失败 | 记录 `service`、`context`（解析链）与 root scope 状态 |
| 循环依赖 | 记录 path（`CircularDependency` 的字符串路径）；`FactoryPanic` 再记录 payload |
| 生命周期/scope | 记录调用位置（root vs scope）与注册生命周期 |
| 启动期 `SyncSingletonDependsOnAsyncService` / `AsyncServiceRequiresBuildAsync` | 属于配置错误，启动即失败（fail-fast），不要重试 |
| `AsyncServiceRequiresGetAsync` | 调用方改用 `get_async`；枚举场景改为逐项异步解析 |
| dispose 后错误 | 说明关闭顺序问题：先停生产任务，再 dispose provider/scope |

## 相关

- [语义权威页（0.10）](semantics.md)
- [生产环境部署检查清单](production-checklist.md)
- [常见陷阱与排查](../08-best-practices/common-pitfalls.md)
