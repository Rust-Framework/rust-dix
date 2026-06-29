# 修复根 ServiceProvider 的 Scoped 生命周期退化为 Transient 的问题

## Summary

当前 `ServiceProvider`（根容器）在解析 `Scoped` 服务时，与 `Transient` 走同一分支 —— 每次解析都直接执行工厂、无任何缓存（见 [provider.rs:378](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L378)）。这违背了 MEDI 的核心设计原则：**根 `IServiceProvider` 本身就是 root scope，从根解析 Scoped 服务时应在根内缓存并复用同一实例**。

本计划按 MEDI 设计理念重构：明确 `ServiceProvider` 作为 root scope 的角色，增加根级 scoped 缓存，拆分 `Scoped` 与 `Transient` 解析路径；同时在 build 阶段实现 captive dependency（Singleton 依赖 Scoped）检测，复用已定义但未启用的 `RdiError::SingletonDependsOnScoped` 错误类型。设计全程利用 Rust 的 `RwLock<HashMap>` 读多写少优势、`Arc` 廉价 clone 语义，避免不必要的写锁与分配。

## Current State Analysis

### 问题代码位置

1. **根 ServiceProvider 结构缺失 scoped 缓存**（[provider.rs:148-163](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L148-L163)）：只有 `singleton_cache`，没有 scoped 缓存字段。
2. **Scoped 退化为 Transient 的铁证**（[provider.rs:378](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L378)）：
   ```rust
   ServiceLifetime::Transient | ServiceLifetime::Scoped => Some((entry.factory)(self)),
   ```
3. **测试固化退化行为**（[tests/validation.rs:54-65](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/tests/validation.rs#L54-L65)）：`singleton_depends_on_scoped_behavior` 断言 `assert!(!Arc::ptr_eq(&a1, &a2))`，把"根上两次解析得到不同实例"当作预期行为。
4. **captive dependency 错误类型已定义但未使用**（[error.rs:10-11](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/error.rs#L10-L11)）：`RdiError::SingletonDependsOnScoped { singleton, scoped }` 从未被构造。

### 正确的对照实现

`Scope` 子作用域对 Scoped 的处理是正确的（[scope.rs:104-119](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/scope.rs#L104-L119)）：先查自身 `scoped_cache`，命中复用，miss 时执行工厂并写入缓存，不回退到根 —— 这正是 MEDI 子 scope 独立缓存的语义。根 ServiceProvider 只需复用同样的模式。

### MEDI 设计原则对照

| 原则 | MEDI 期望 | 当前 LRDI | 修复后 |
|------|-----------|-----------|--------|
| 根 = root scope | 根解析 Scoped 在根内缓存复用 | 退化为 Transient | ✅ root_scoped_cache |
| CreateScope() 子 scope | 子 scope 独立缓存 Scoped | ✅ 已正确 | ✅ 不变 |
| Singleton 全局共享 | 所有 scope 共享 singleton | ✅ 已正确 | ✅ 不变 |
| Transient 每次新建 | 每次解析新建 | ✅ 已正确 | ✅ 不变 |
| Captive dependency 检测 | Singleton 不可依赖 Scoped | ❌ 未检测 | ✅ build 阶段报错 |

## Proposed Changes

### Change 1: ServiceProvider 增加 root scope 的 scoped 缓存字段

**文件**：[crates/rust-dicore/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs)

**What**: 在 `ServiceProvider` 结构体增加 `root_scoped_cache` 字段，并在文档注释中明确 ServiceProvider 即 MEDI root scope 的角色。

**Why**: MEDI 中 `IServiceProvider` 本身就是 root scope，从根解析 Scoped 服务应在根内缓存。当前缺失该字段导致 Scoped 退化为 Transient。字段命名 `root_scoped_cache` 显式体现 root scope 概念。

**How**:
- 在 `provider.rs:148-163` 的结构体定义中，于 `singleton_cache` 之后增加：
  ```rust
  /// Root scope 的 Scoped 实例缓存。
  ///
  /// ServiceProvider 本身即 MEDI 的 root scope：从根直接解析 Scoped 服务时，
  /// 实例在此缓存中复用，语义等同于在根 scope 内单例化。
  /// 子 Scope（通过 `create_scope()` 创建）拥有独立的 scoped_cache，不回退到此缓存。
  root_scoped_cache: RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
  ```
- 在 `provider.rs:195-200` 的 `Self { ... }` 构造中初始化：
  ```rust
  root_scoped_cache: RwLock::new(HashMap::new()),
  ```

### Change 2: 拆分 Scoped 与 Transient 解析路径

**文件**：[crates/rust-dicore/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L355-L380)

**What**: 修改 `get_any_by_entry`，把 `Scoped` 从 `Transient` 分支拆出，走 `root_scoped_cache` 缓存逻辑。

**Why**: 这是修复核心。Scoped 必须在 root scope 内缓存复用，仅 Transient 走纯工厂路径。

**How**: 替换 `provider.rs:355-380` 的 `get_any_by_entry` 实现：
```rust
pub(crate) fn get_any_by_entry(
    &self,
    entry: &ServiceEntry,
) -> Option<Arc<dyn Any + Send + Sync>> {
    match entry.lifetime {
        ServiceLifetime::Singleton => {
            {
                let cache = self.singleton_cache.read().unwrap();
                if let Some(instance) = cache.get(&entry.cache_key) {
                    return Some(instance.clone());
                }
            }
            let instance = (entry.factory)(self);
            self.singleton_cache
                .write()
                .unwrap()
                .insert(entry.cache_key, instance.clone());
            Some(instance)
        }
        ServiceLifetime::Scoped => {
            // Root scope 缓存：ServiceProvider 即 root scope，
            // 从根解析 Scoped 服务在 root_scoped_cache 内复用。
            {
                let cache = self.root_scoped_cache.read().unwrap();
                if let Some(instance) = cache.get(&entry.cache_key) {
                    return Some(instance.clone());
                }
            }
            let instance = (entry.factory)(self);
            self.root_scoped_cache
                .write()
                .unwrap()
                .insert(entry.cache_key, instance.clone());
            Some(instance)
        }
        ServiceLifetime::Transient => Some((entry.factory)(self)),
    }
}
```

**性能说明**: 读锁优先、命中即 clone Arc（廉价），miss 时才升级写锁 —— 与现有 `singleton_cache` 模式一致，读多写少场景下读锁无竞争。

### Change 3: 实现 captive dependency 检测（build 阶段）

**文件**：[crates/rust-dicore/src/provider.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L10-L146)（`ValidationResolver`）、[provider.rs:217-267](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/provider.rs#L217-L267)（`validate_dependencies`）

**What**: 在 build 阶段的依赖验证中，检测 Singleton 的依赖链中是否出现 Scoped 服务，若出现则返回 `RdiError::SingletonDependsOnScoped`。

**Why**: Captive dependency 是 DI 经典反模式 —— Singleton 会捕获并永久持有 Scoped 实例，导致后续 scope 无法获得新的 Scoped 实例。MEDI 实践中应拒绝这种注册。错误类型 `RdiError::SingletonDependsOnScoped` 已定义但未启用，此次一并启用。

**How**:
1. 修改 `ValidationResolver` 结构（provider.rs:10-23），增加解析栈跟踪：
   ```rust
   struct ValidationResolver<'a> {
       store: &'a ServiceStore,
       resolving: RwLock<HashSet<usize>>,
       /// 解析栈：记录当前解析链中每个节点的 (cache_key, lifetime)，
       /// 用于 captive dependency 检测（Singleton 依赖 Scoped）。
       stack: RwLock<Vec<(usize, ServiceLifetime)>>,
       type_names: &'a HashMap<TypeId, &'static str>,
       cache: &'a RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
   }
   ```
2. 在 `ValidationResolver::new` 中初始化 `stack: RwLock::new(Vec::new())`。
3. 在 `get_any`（provider.rs:56-102）和 `get_keyed_any`（provider.rs:104-145）中，解析服务前增加 captive dependency 检查：
   ```rust
   // Captive dependency 检测：如果当前解析链中存在 Singleton 祖先，
   // 而当前服务是 Scoped，则报告 captive dependency。
   if entry.lifetime == ServiceLifetime::Scoped {
       if let Some(&(ancestor_ck, ServiceLifetime::Singleton)) =
           self.stack.read().unwrap().iter().find(|(_, lt)| *lt == ServiceLifetime::Singleton)
       {
           let singleton_name = self.get_type_name(ancestor_ck);
           let scoped_name = self.get_type_name(entry.cache_key);
           return None; // 通过 panic 传递错误，与现有循环依赖检测模式一致
           // 实际用 panic_any 传递 RdiError，由 validate_dependencies 捕获
       }
   }
   ```
   采用与现有循环依赖检测一致的 panic + catch_unwind 模式：用 `std::panic::panic_any(RdiError::SingletonDependsOnScoped { ... })` 传递错误，在 `validate_dependencies` 的 `catch_unwind` 中 downcast 捕获。
4. 在解析前后 push/pop 栈：
   ```rust
   self.stack.write().unwrap().push((entry.cache_key, entry.lifetime));
   let result = (entry.factory)(self as &dyn IServiceResolver);
   self.stack.write().unwrap().pop();
   ```
5. 修改 `validate_dependencies`（provider.rs:240-261）的 catch_unwind 分支，增加对 `RdiError::SingletonDependsOnScoped` 的 downcast 处理：
   ```rust
   Err(panic_info) => {
       if let Some(err) = panic_info.downcast_ref::<RdiError>() {
           match err {
               RdiError::SingletonDependsOnScoped { .. } => return Err(err.clone()),
               _ => {}
           }
       }
       // 保留现有 CIRCULAR_DEPENDENCY 处理逻辑
       // ...
   }
   ```
   注意：`RdiError` 需要派生 `Clone`（目前未派生），或改为返回 `String` 重建错误。由于 `RdiError` 字段都是 `String`/`&'static str`，派生 `Clone` 是安全的。

### Change 4: 为 RdiError 派生 Clone

**文件**：[crates/rust-dicore/src/error.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/error.rs#L2)

**What**: `#[derive(Error, Debug)]` 改为 `#[derive(Error, Debug, Clone)]`。

**Why**: captive dependency 检测通过 panic + catch_unwind 传递错误，捕获后需要返回错误副本。`RdiError` 字段均为 `String`/`&'static str`，Clone 廉价且安全。

### Change 5: 改写固化退化行为的测试

**文件**：[crates/rust-dicore/tests/validation.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/tests/validation.rs#L54-L65)

**What**: 改写 `singleton_depends_on_scoped_behavior` 测试，反映新的 root scope 缓存语义；并新增 captive dependency 拒绝测试。

**Why**: 原测试断言 `!Arc::ptr_eq(&a1, &a2)` 固化了 Scoped 退化为 Transient 的错误行为，修复后会失败。测试名也具误导性（实际未测 singleton 依赖 scoped）。

**How**: 替换为以下测试：
```rust
#[test]
fn scoped_cached_at_root_scope() {
    // ServiceProvider 即 root scope：从根解析 Scoped 服务应在根内缓存复用。
    let provider = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .build()
        .unwrap();

    let a1: Arc<ServiceA> = provider.get();
    let a2: Arc<ServiceA> = provider.get();
    assert!(Arc::ptr_eq(&a1, &a2), "root scope 内 Scoped 应复用同一实例");
}

#[test]
fn scoped_isolated_between_root_and_child_scope() {
    // 根 root scope 与子 scope 的 Scoped 实例相互独立（MEDI 语义）。
    let provider = Arc::new(
        ServiceCollection::new()
            .scoped(|_| Arc::new(ServiceA))
            .build()
            .unwrap(),
    );

    let root_a: Arc<ServiceA> = provider.get();
    let scope = provider.scope();
    let child_a: Arc<ServiceA> = scope.get();
    assert!(!Arc::ptr_eq(&root_a, &child_a), "根与子 scope 的 Scoped 应相互独立");
}

#[test]
fn singleton_depends_on_scoped_rejected() {
    // Captive dependency：Singleton 依赖 Scoped 应在 build 阶段被拒绝。
    let result = ServiceCollection::new()
        .scoped(|_| Arc::new(ServiceA))
        .singleton(|r| {
            let _a: Arc<ServiceA> = r.get();
            Arc::new(ServiceB)
        })
        .build();
    assert!(result.is_err());
    match result {
        Err(RdiError::SingletonDependsOnScoped { singleton, scoped }) => {
            assert!(singleton.contains("ServiceB"));
            assert!(scoped.contains("ServiceA"));
        }
        _ => panic!("Expected SingletonDependsOnScoped error"),
    }
}
```

### Change 6: 更新 lib.rs 文档注释，明确 root scope 概念

**文件**：[crates/rust-dicore/src/lib.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/lib.rs#L140-L143)

**What**: 在 `ServiceProvider` 的 re-export 注释中补充 root scope 语义说明。

**Why**: 让 API 文档明确体现 MEDI 设计理念 —— ServiceProvider 不仅是根容器，本身即 root scope。

**How**: 修改 lib.rs:140-143：
```rust
/// Built service provider — the root DI container (read-only after build).
///
/// 兼具 MEDI 中 `IServiceProvider` 与 root scope 双重身份：
/// - 作为 `IServiceProvider`：提供类型/键控/命名解析入口。
/// - 作为 root scope：从根直接解析 Scoped 服务时，实例在根内缓存复用，
///   语义等同于在根 scope 内单例化。通过 `create_scope()` 创建的子 Scope
///   拥有独立的 scoped 缓存，不回退到根。
///
/// Analogous to `IServiceProvider` in MEDI.
pub use provider::ServiceProvider;
```

## Assumptions & Decisions

1. **RootScope 概念的体现方式**：不引入独立的 `RootScope` 类型（MEDI 本身也没有），而是让 `ServiceProvider` 兼具 root scope 身份，通过 `root_scoped_cache` 字段命名和文档注释显式体现 root scope 概念。这避免了 `ServiceProvider ↔ Scope` 的 `Arc` 循环引用问题，且与 MEDI 中"`IServiceProvider` 本身就是 root scope"的理念完全一致。

2. **子 Scope 的 Scoped 解析不回退到根**：保持 [scope.rs:104-119](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/scope.rs#L104-L119) 现有行为不变。MEDI 语义中，子 scope 内创建的 Scoped 实例独立缓存，不会回退到 root scope 的缓存 —— 这保证不同 scope 间的 Scoped 实例隔离。

3. **Captive dependency 检测严格度**：采用严格版 —— 解析链中任何位置的 Singleton 祖先 + 当前 Scoped 即报错（包括 `Singleton → Transient → Scoped` 间接依赖）。这比仅检测直接依赖更安全，符合 MEDI 最佳实践。

4. **错误传递机制**：复用现有 `ValidationResolver` 的 panic + `catch_unwind` 模式传递 `RdiError::SingletonDependsOnScoped`，与循环依赖检测保持一致的代码风格。为此需要为 `RdiError` 派生 `Clone`（字段均为 `String`/`&'static str`，安全且廉价）。

5. **性能考量**：保持 `std::sync::RwLock<HashMap>` 实现，与现有 `singleton_cache` 一致。读多写少场景下读锁无竞争，命中时仅 clone `Arc`（原子递增，廉价）。未引入 `parking_lot`/`dashmap` 等新依赖，避免扩大改动范围；如需进一步优化可后续独立引入。

6. **wrapper.rs / bridge.rs 无需改动**：`ServiceProviderWrapper`（[wrapper.rs](file:///d:/GitCode/RF/rust-di/crates/rust-dicore/src/wrapper.rs)）通过 `IServiceResolver::get_any` 委托给 child/root `ServiceProvider`，会自动走修复后的 `get_any_by_entry`，自动受益。

## Verification Steps

1. **编译验证**：`cargo build -p rust-dicore` 确保无编译错误。
2. **现有测试**：`cargo test -p rust-dicore` 全部通过（除改写的测试外，其余测试不应受影响）。
3. **root scope 缓存语义**：新增的 `scoped_cached_at_root_scope` 测试通过 —— 根上两次解析 Scoped 得到同一实例。
4. **root 与子 scope 隔离**：新增的 `scoped_isolated_between_root_and_child_scope` 测试通过 —— 根 Scoped 与子 scope Scoped 是不同实例。
5. **captive dependency 检测**：新增的 `singleton_depends_on_scoped_rejected` 测试通过 —— build 返回 `RdiError::SingletonDependsOnScoped`。
6. **间接 captive dependency**：手动验证 `Singleton → Transient → Scoped` 也会被拒绝（可在测试中补充）。
7. **子 scope 行为不回归**：`tests/scope.rs` 的 `scoped_cached_within_same_scope`、`scoped_cached_per_scope` 仍通过。
8. **Singleton 行为不回归**：`tests/validation.rs` 的 `singleton_shared_globally`、`transient_creates_new_instances` 仍通过。
9. **文档构建**：`cargo doc -p rust-dicore` 无警告，root scope 语义说明出现在文档中。
10. **workspace 全量**：`cargo test --workspace` 确保跨 crate 无回归（特别是依赖 rust-dicore 的宏 crate 与示例）。
