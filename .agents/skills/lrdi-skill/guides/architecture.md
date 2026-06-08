# LRDI 架构原理深度指南

## 一、设计哲学

LRDI 受 Microsoft.Extensions.DependencyInjection (MEDI) 启发，核心设计原则：

1. **构建时不可变**：ServiceProvider 构建后只读，所有注册在 ServiceCollection.build() 时完成
2. **类型驱动解析**：以 Rust 的 TypeId 作为服务标识，同一个 TypeId 对应一组 ServiceEntry
3. **两阶段 Singleton 初始化**：先收集所有 singleton，再统一执行工厂，支持交叉引用
4. **Arc 共享所有权**：所有服务通过 Arc<T> 共享，天然线程安全
5. **类型擦除存储**：内部以 Arc<dyn Any + Send + Sync> 存储，解析时 downcast 回具体类型

## 二、核心类型详解

### ServiceCollection
- 注册阶段的数据结构，持有 Vec<ServiceDescriptor>
- ServiceDescriptor 包含：type_id, type_name, key, factory, lifetime
- 工厂闭包签名为 Fn(&dyn IServiceResolver) -> Arc<T>，闭包接收 resolver 参数以支持依赖解析
- 实现 builder pattern，所有方法返回 Self
- .build() 将 descriptors 转换为 ServiceStore 并构建 ServiceProvider

### ServiceProvider
- 根容器，持有 ServiceStore、type_map（type_name→TypeId）、singleton_cache（RwLock<HashMap>）、named（RwLock<HashMap>）
- 构建时执行两阶段 singleton 初始化
- Phase 1: 收集所有 singleton entries（按 cache_key 索引）
- Phase 2: 逐个执行工厂，若工厂引用尚未初始化的 singleton，通过 RwLock 的 lazy fallback 按需执行
- 构建完成后不可变（注册不再支持）

### ServiceStore
- 内部类型：HashMap<TypeId, Vec<ServiceEntry>>
- ServiceEntry 包含：cache_key, key (Option<String>), type_name, factory (ServiceFactory), lifetime
- ServiceFactory = Arc<dyn Fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync> + Send + Sync>

### Scope
- 持有 Arc<ServiceProvider>（父容器引用）和 scoped_cache（RwLock<HashMap>）
- Singleton 解析：委托父容器
- Transient 解析：每次执行工厂，不缓存
- Scoped 解析：在 scoped_cache 中缓存，首次执行工厂
- Scope drop 时，scoped_cache 自然释放（Rust 无析构回调）

### ServiceProviderWrapper
- 持有 child: Arc<ServiceProvider> 和 root: Arc<ServiceProvider>
- 解析策略：child-first，root-fallback
- get_all 合并两者结果（child 优先）
- IServiceResolver 实现：child.get_any() 或 root.get_any()
- 子容器对根容器不可见（插件隔离）

### RdiProvider 与 ServiceLocatorBridge
- RdiProvider 是 enum { Root(Arc<ServiceProvider>), Wrapped(Arc<ServiceProviderWrapper>) }
- 统一两种容器类型，都实现 IServiceResolver
- ServiceLocatorBridge 适配 RdiProvider 为 IServiceLocator

## 三、生命周期决策树

详细展开每种生命周期的选择逻辑，包含决策流程：

当你需要注册一个服务时，按以下决策树选择生命周期：

1. 这个服务是否包含可变状态？
   - 是 → 2
   - 否 → 3

2. 这个状态的生存范围是什么？
   - 整个应用生命周期 → Singleton（如：连接池、全局配置）
   - 单个请求/操作 → Scoped（如：请求上下文、事务对象）
   - 不需要共享状态 → Transient（如：值对象、DTO）

3. 这个服务是否有昂贵的初始化成本？
   - 是 → Singleton（避免重复初始化）
   - 否 → Transient（默认选择）

4. 特殊情况：
   - 需要在运行时动态选择实现 → 键控服务 + 策略模式
   - 需要在测试中替换 → 注册为 dyn Trait
   - 需要跨请求共享但又不能全局 → Scoped

## 四、注册与解析流程图

绘制完整的注册→解析流程图，展示数据流：

```
注册阶段:
  ServiceCollection::new()
    .singleton(|_| Arc::new(Foo))
    .transient(|p| Arc::new(Bar(p.get())))
    .build()
      │
      ▼
  ServiceCollection.push()
    → 创建 ServiceDescriptor { type_id=TypeId::of::<T>(), key, factory, lifetime }
    → 工厂闭包 type-erased: Arc<dyn Any+Send+Sync>
      │
      ▼
  ServiceCollection.build()
    → 遍历 descriptors，按 TypeId 分组为 HashMap<TypeId, Vec<ServiceEntry>>
    → 构建 type_map（type_name → TypeId）
    → 构建 ServiceProvider
    → 两阶段 Singleton 初始化
    → 返回 Result<ServiceProvider, RdiError>

解析阶段:
  provider.get::<Foo>()
    → try_get::<Foo>()
      → TypeId::of::<Foo>()
      → store.get(&tid)
      → 找 key.is_none() 的 entry
      → get_any_by_entry(entry)
        ├── Singleton → 查 singleton_cache / lazy fallback
        ├── Scoped → 委托给 Scope 处理
        └── Transient → 执行工厂
      → extract(): 从 Arc<dyn Any> 中取出 Arc<T>（双层 Arc 解包）
    → 返回 Arc<Foo>
```

## 五、双重 Arc 包装机制

详细解释为什么需要双层 Arc：

工厂闭包返回 Arc<T>（内层）→ push() 中包装为 Arc<dyn Any+Send+Sync>（外层）
原因：Rust 的类型系统要求 Arc<dyn Any> 的内部类型必须是 Sized
Arc<T> 是 Sized → Arc<Arc<T>> as Arc<dyn Any> → 可行
T（可能是 dyn Trait）是 !Sized → 不能直接 Arc<T> as Arc<dyn Any>

extract() 方法：Arc<dyn Any> → downcast::<Arc<T>>() → Arc::clone(&*double) → Arc<T>

## 六、线程安全设计

- ServiceProvider: 持有 RwLock<HashMap>（singleton_cache 和 named），读多写少
- Scope: 持有 RwLock<HashMap>（scoped_cache），仅在首次解析时写入
- 所有 Arc 共享使容器可跨线程 Clone/传递
- IServiceResolver trait 要求 Send + Sync

## 七、与 MEDI 的对应关系

| MEDI (.NET) | LRDI (Rust) | 差异 |
|-------------|-------------|------|
| IServiceCollection | ServiceCollection | LRDI 用 builder pattern |
| IServiceProvider | ServiceProvider | LRDI 构建后不可变 |
| IServiceScope | Scope | 功能等价 |
| AddSingleton<T> | .singleton(f) | LRDI 必须提供工厂闭包 |
| AddScoped<T> | .scoped(f) | 同上 |
| AddTransient<T> | .transient(f) | 同上 |
| GetService<T> | .get_service::<T>() | 返回 Option<Arc<T>> |
| GetRequiredService<T> | .get::<T>() | panic 而非抛异常 |
| CreateScope() | .create_scope() | 完全等价 |
| TryAdd | .try_add(f) | 完全等价 |
| KeyedService | .keyed(k, f) | LRDI 原生支持 |
| - | ServiceProviderWrapper | LRDI 特有，MEDI 无直接对应 |
