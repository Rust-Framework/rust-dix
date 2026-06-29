# rust-dicore 功能增强需求：Owned 服务解析

> **目标**：让 DI 容器能解析 owned `T`（而非 `Arc<T>`），使消费框架（如 rust-ef）能实现 handler 持有 owned `DbContext` + `&mut self` 方法的地道 Rust 模式，无需 interior mutability。
>
> **背景**：rust-ef 框架的 `DbContext` 方法使用 `&mut self`（地道的 Rust 设计）。但 rust-dicore 0.3.2/0.4.1 所有解析方法都返回 `Arc<T>`，导致 `Arc<DbContext>` 只能拿到 `&DbContext`，无法调用 `&mut self` 方法。当前唯一解法是 interior mutability（`RwLock` + `unsafe`），这是沉重妥协。增加 owned 解析能力可从根上解决。

---

## 一、现状分析

### 1.1 当前 API（rust-dicore 0.3.2 / 0.4.1）

**注册**（`collection.rs`）：
```rust
pub fn scoped<T: ?Sized + Send + Sync + 'static>(
    self,
    f: impl Fn(&dyn IServiceResolver) -> Arc<T> + Send + Sync + 'static,
) -> Self
// singleton / transient 同理，工厂都返回 Arc<T>
```

**解析**（`provider.rs` / `scope.rs` / `entry.rs`）：
```rust
// 所有解析方法都返回 Arc<T>
pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T>
pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>>
pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Arc<T>
```

**工厂签名**（`entry.rs:66`）：
```rust
pub type ServiceFactory =
    Arc<dyn Fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync> + Send + Sync>;
```

**内部包装**（`collection.rs:157`）：
```rust
let sf: ServiceFactory = Arc::new(move |r| {
    let val: Arc<T> = (f)(r);
    Arc::new(val) as Arc<dyn Any + Send + Sync>  // Arc<Arc<T>> 擦除为 Arc<dyn Any>
});
```

**Scope 缓存**（`scope.rs:10`）：
```rust
scoped_cache: RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>
```

### 1.2 核心限制

1. **所有解析返回 `Arc<T>`**：无法获得 owned `T` 或 `&mut T`
2. **Trait bound 强制 `Sync`**：`T: ?Sized + Send + Sync + 'static`，owned 单线程访问不需要 `Sync`
3. **无 `get_owned` / `get_mut` 方法**：全仓库零命中
4. **`#[derive(Inject)]` 只支持 `Arc<T>` 字段**：无法注入 owned `T`

### 1.3 消费框架的困境

```rust
// rust-ef 当前困境
#[derive(Inject)]
pub struct BlogHandler {
    ctx: Arc<DbContext>,  // DI 只能注入 Arc<DbContext>
}

async fn handle(&self, req: ...) {
    self.ctx.set::<Blog>().add(blog);   // ❌ set::<T>(&mut self)，但 Arc 只给 &
    self.ctx.save_changes().await?;      // ❌ save_changes(&mut self)
}
```

**当前唯一解法**：interior mutability（`RwLock<DbContextState>` + `Mutex<Vec>` + `unsafe` 生命周期延长）。这是沉重妥协，丧失了 Rust 的编译时安全保证。

---

## 二、需求规格

### 2.1 核心 API：`get_owned::<T>() -> T`

**在 `ServiceProvider` 和 `Scope` 上添加 `get_owned` 方法**：

```rust
impl ServiceProvider {
    /// 解析 owned 服务实例。
    ///
    /// 创建新实例（调用工厂），返回 owned `T`（非 `Arc<T>`）。
    /// 适用于需要 `&mut self` 访问的场景（如 DbContext）。
    ///
    /// - Transient：每次调用创建新实例 ✓
    /// - Scoped：每次调用创建新实例（绕过 scope 缓存）✓
    /// - Singleton：panic（单例是共享的，无法 owned）
    ///
    /// # Panics
    /// - 服务未注册
    /// - 服务是 Singleton 生命周期（共享实例无法 owned）
    pub fn get_owned<T: Send + 'static>(&self) -> T {
        // 1. 找到服务条目
        // 2. 检查生命周期：Singleton → panic
        // 3. 调用工厂（绕过 scope/singleton 缓存）
        // 4. downcast Arc<dyn Any> → Arc<Arc<T>> → Arc<T>
        // 5. Arc::try_unwrap(arc_t) → T（工厂每次创建新 Arc，refcount=1，成功）
    }

    /// try_get_owned：返回 Option，不 panic
    pub fn try_get_owned<T: Send + 'static>(&self) -> Option<T> { ... }
}

impl Scope {
    /// 在 scope 内解析 owned 服务实例。
    ///
    /// - Transient：调用工厂创建新实例 ✓
    /// - Scoped：调用工厂创建新实例（绕过 scope 缓存）✓
    /// - Singleton：使用父 provider 的 singleton 缓存，try_unwrap 失败 → panic
    pub fn get_owned<T: Send + 'static>(&self) -> T { ... }

    pub fn try_get_owned<T: Send + 'static>(&self) -> Option<T> { ... }
}
```

**关键设计点**：

1. **Trait bound 放宽**：`T: Send + 'static`（不需要 `Sync`——owned 实例单线程访问）
2. **绕过缓存**：`get_owned` 对 scoped 服务绕过 `scoped_cache`，每次创建新实例
3. **Singleton 限制**：`get_owned` 对 singleton panic（共享实例无法 owned）
4. **`Arc::try_unwrap` 可行性**：工厂每次调用创建新 `Arc<T>`（refcount=1），`try_unwrap` 成功

### 2.2 Keyed 版本

```rust
impl ServiceProvider {
    pub fn get_keyed_owned<T: Send + 'static>(&self, key: &str) -> T { ... }
    pub fn try_get_keyed_owned<T: Send + 'static>(&self, key: &str) -> Option<T> { ... }
}

impl Scope {
    pub fn get_keyed_owned<T: Send + 'static>(&self, key: &str) -> T { ... }
    pub fn try_get_keyed_owned<T: Send + 'static>(&self, key: &str) -> Option<T> { ... }
}
```

### 2.3 `IServiceResolver` trait 扩展

```rust
pub trait IServiceResolver: Send + Sync {
    // 现有方法保持不变
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;

    // 新增：owned 解析（可选默认实现）
    fn get_owned_any(&self, key: &str) -> Option<Box<dyn Any + Send>> {
        None // 默认不支持，由具体实现覆盖
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Box<dyn Any + Send>> {
        None
    }
}
```

> **注**：`get_owned_any` 返回 `Box<dyn Any + Send>`（非 `Arc`），因为 owned 实例不需要共享。或可返回 `Arc<dyn Any + Send>` 然后 `try_unwrap`——由实现者决定。

### 2.4 `#[derive(Inject)]` 宏增强

**当前**：宏生成的代码对 `Arc<T>` 字段调用 `resolver.get::<T>()`

**需要**：宏自动检测字段类型，区分 `Arc<T>` 和 owned `T`：

```rust
// 输入
#[derive(Inject)]
pub struct BlogHandler {
    ctx: DbContext,           // owned 字段 → get_owned
    logger: Arc<Logger>,      // Arc 字段 → get（现有行为）
    cache: Arc<dyn ICache>,   // trait object → get（现有行为）
}

// 宏生成
impl Inject for BlogHandler {
    fn create(resolver: &dyn IServiceResolver) -> Self {
        Self {
            ctx: resolver.get_owned::<DbContext>(),      // ← 新增
            logger: resolver.get::<Logger>(),             // ← 现有
            cache: resolver.get::<dyn ICache>(),          // ← 现有
        }
    }
}
```

**宏检测逻辑**：
- 字段类型是 `Arc<T>` 或 `Arc<dyn Trait>` → 调用 `get::<T>()`
- 字段类型是裸 `T`（非 `Arc`） → 调用 `get_owned::<T>()`
- 字段类型是 `Option<Arc<T>>` → 调用 `try_get::<T>().map(Some)`
- 字段类型是 `Option<T>` → 调用 `try_get_owned::<T>().map(Some)`

### 2.5 新注册方法（可选，推荐）

虽然 `get_owned` 可对现有 scoped/transient 注册工作，但为了语义清晰，推荐增加 owned 专用注册方法：

```rust
impl ServiceCollection {
    /// 注册 owned scoped 服务。
    /// 
    /// 工厂返回 owned `T`（非 `Arc<T>`）。
    /// 适用于需要 `&mut self` 访问的服务（如 DbContext）。
    /// 
    /// 通过 `get_owned::<T>()` 解析，每次创建新实例。
    /// 通过 `get::<T>()` 解析会 panic（owned 注册不支持 Arc 解析）。
    pub fn scoped_owned<T: Send + 'static>(
        self,
        f: impl Fn(&dyn IServiceResolver) -> T + Send + Sync + 'static,
    ) -> Self { ... }

    /// 注册 owned transient 服务。
    pub fn transient_owned<T: Send + 'static>(
        self,
        f: impl Fn(&dyn IServiceResolver) -> T + Send + Sync + 'static,
    ) -> Self { ... }

    /// 注册 keyed owned scoped 服务。
    pub fn keyed_scoped_owned<T: Send + 'static>(
        self,
        key: impl Into<String>,
        f: impl Fn(&dyn IServiceResolver) -> T + Send + Sync + 'static,
    ) -> Self { ... }
}
```

**owned 工厂类型**：
```rust
/// owned 工厂：返回 Box<dyn Any + Send>（owned，非 Arc）
pub type OwnedServiceFactory =
    Arc<dyn Fn(&dyn IServiceResolver) -> Box<dyn Any + Send> + Send + Sync>;
```

**ServiceEntry 扩展**：
```rust
pub struct ServiceEntry {
    pub cache_key: usize,
    pub key: Option<String>,
    pub type_name: &'static str,
    pub factory: ServiceFactory,           // Arc 工厂（现有）
    pub owned_factory: Option<OwnedServiceFactory>,  // owned 工厂（新增）
    pub lifetime: ServiceLifetime,
}
```

> **设计决策**：`owned_factory` 是 `Option`，向后兼容。如果使用 `scoped_owned` 注册，填充 `owned_factory`；`get_owned` 优先使用 `owned_factory`，回退到 `factory` + `try_unwrap`。

---

## 三、使用示例

### 3.1 rust-ef DbContext 注入（核心用例）

```rust
// 注册（rust-ef 的 add_dbcontext 改用 scoped_owned）
impl DbContextServiceCollectionExt for ServiceCollection {
    fn add_dbcontext(self, configure: ...) -> Self {
        let options = Arc::new(builder.build());
        self.scoped_owned(move |_| {
            DbContext::from_options(&options).expect("Failed to create DbContext")
        })
    }
}

// Handler 持有 owned DbContext
#[derive(Inject)]
pub struct BlogHandler {
    ctx: DbContext,  // owned，非 Arc
}

// Handler 方法用 &mut self —— 地道 Rust，无锁无 unsafe
#[inject]
#[async_trait]
impl IRequestHandler<CreateBlogRequest, BlogModel> for BlogHandler {
    async fn handle(&mut self, req: CreateBlogRequest) -> Result<BlogModel> {
        self.ctx.set::<Blog>().add(blog);      // ✓ &mut self
        self.ctx.save_changes().await?;         // ✓ &mut self
        Ok(...)
    }
}
```

### 3.2 混合注入

```rust
#[derive(Inject)]
pub struct OrderHandler {
    ctx: DbContext,           // owned → get_owned
    logger: Arc<Logger>,      // shared → get
    cache: Arc<dyn ICache>,   // trait object → get
}
```

### 3.3 非 web 场景

```rust
let provider = ServiceCollection::new()
    .scoped_owned(|_| DbContext::from_options(&options).unwrap())
    .build()?;

let scope = provider.create_scope();
let mut ctx: DbContext = scope.get_owned();  // owned
ctx.set::<Blog>().add(blog);
ctx.save_changes().await?;
```

---

## 四、实现方案（建议）

### 4.1 方案 A：最小改动（仅 `get_owned` 方法）

**改动文件**：`provider.rs`、`scope.rs`、`entry.rs`

**实现**：
```rust
// entry.rs — IServiceResolver 新增方法
pub trait IServiceResolver: Send + Sync {
    // ... 现有方法 ...

    /// 创建 owned 服务实例（绕过缓存）。
    /// 返回 Arc<dyn Any>，由调用者 try_unwrap。
    fn create_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        None // 默认实现，由 ServiceProvider/Scope 覆盖
    }
}

// provider.rs
impl ServiceProvider {
    pub fn get_owned<T: Send + 'static>(&self) -> T {
        let arc = self.create_fresh::<T>();  // 绕过缓存，调用工厂
        Arc::try_unwrap(arc).unwrap_or_else(|_| {
            panic!("get_owned: service {} is shared, cannot own", std::any::type_name::<T>())
        })
    }
}

// scope.rs
impl Scope {
    pub fn get_owned<T: Send + 'static>(&self) -> T {
        // 对 transient/scoped：绕过 scoped_cache，直接调用工厂
        // 对 singleton：尝试 try_unwrap（可能失败）
    }
}
```

**优点**：改动最小，不破坏现有 API
**缺点**：scoped 的 `get_owned` 绕过缓存（每次新实例），语义略有歧义

### 4.2 方案 B：完整 owned 注册（推荐）

**改动文件**：`collection.rs`、`entry.rs`、`provider.rs`、`scope.rs`、`descriptor.rs`、`store.rs`

**实现**：
1. 新增 `OwnedServiceFactory` 类型
2. `ServiceEntry` 增加 `owned_factory: Option<OwnedServiceFactory>` 字段
3. `ServiceCollection` 增加 `scoped_owned` / `transient_owned` / `keyed_scoped_owned` 方法
4. `ServiceProvider` / `Scope` 增加 `get_owned` / `try_get_owned` 方法
5. `get_owned` 优先使用 `owned_factory`，回退到 `factory` + `try_unwrap`
6. `#[derive(Inject)]` 宏检测字段类型，自动选择 `get` / `get_owned`

**优点**：语义清晰，owned 注册与 Arc 注册分离
**缺点**：改动较大，需同步改 `#[derive(Inject)]` 宏

### 4.3 推荐路径

**先实现方案 A**（最小改动，快速验证），再迭代到方案 B（完整语义）。

方案 A 的 `get_owned` 对 scoped 绕过缓存是可接受的——DbContext 每请求只需要一个实例，`get_owned` 每次创建新实例恰好满足需求。

---

## 五、对消费框架的影响

### 5.1 rust-ef（本仓库）

| 改动 | 说明 |
|------|------|
| `di.rs` `add_dbcontext` | `self.scoped(...)` → `self.scoped_owned(...)`（工厂返回 owned `DbContext`） |
| `DbContext` 方法签名 | **不变**——保持 `&mut self`（无需 interior mutability） |
| `DbSet` 方法签名 | **不变**——保持 `&mut self` |
| `ErasedSetOps` trait | **不变**——保持 `&mut dyn Any` |
| Handler 模板 | `ctx: Arc<DbContext>` → `ctx: DbContext`；`handle(&self)` → `handle(&mut self)` |
| 文档 | 删除 interior mutability 相关说明；强调 owned 注入 |

### 5.2 rust-webapp（下游）

| 改动 | 说明 |
|------|------|
| Handler 调度 | `get::<Handler>()` → `get_owned::<Handler>()`（每请求创建 owned handler） |
| `IRequestHandler` trait | `handle(&self, ...)` → `handle(&mut self, ...)` |
| Handler 生命周期 | 确认每请求创建（transient/owned），非 singleton |

### 5.3 `#[derive(Inject)]` 宏（rust-dicore-macros）

| 改动 | 说明 |
|------|------|
| 字段类型检测 | `Arc<T>` → `get::<T>()`；裸 `T` → `get_owned::<T>()` |
| `Option` 支持 | `Option<Arc<T>>` → `try_get`；`Option<T>` → `try_get_owned` |

---

## 六、验收标准

### 6.1 功能验收

```rust
// 1. scoped_owned 注册 + get_owned 解析
let p = ServiceCollection::new()
    .scoped_owned(|_| MyService::new())
    .build()?;
let s: MyService = p.get_owned();  // ✓ owned

// 2. 混合注入
#[derive(Inject)]
struct Handler {
    owned_svc: MyService,       // get_owned
    shared_svc: Arc<Logger>,    // get
}
let h: Handler = scope.get_owned();  // ✓ 自动混合

// 3. DbContext 用例（核心目标）
let mut ctx: DbContext = scope.get_owned();
ctx.set::<Blog>().add(blog);       // ✓ &mut self
ctx.save_changes().await?;         // ✓ &mut self

// 4. Singleton 拒绝 owned
let p = ServiceCollection::new()
    .singleton(|_| Arc::new(MyService::new()))
    .build()?;
// p.get_owned::<MyService>() → panic("singleton cannot be owned")

// 5. Keyed owned
let p = ServiceCollection::new()
    .keyed_scoped_owned("primary", |_| DbContext::new())
    .build()?;
let ctx: DbContext = p.get_keyed_owned("primary");
```

### 6.2 非功能验收

- **性能**：`get_owned` 与 `get` 相比，仅少一次 `Arc` 包装，开销可忽略
- **线程安全**：`get_owned` 返回 `T: Send`，可跨线程转移
- **向后兼容**：现有 `get::<T>()` / `Arc<T>` 注册和行为不变
- **编译时安全**：无 `unsafe`（`Arc::try_unwrap` 是安全 API）

---

## 七、开放问题（供 rust-dicore 维护者决策）

1. **Scoped owned 语义**：`get_owned` 对 scoped 服务绕过缓存（每次新实例）是否可接受？
   - rust-ef 视角：可接受（DbContext 每请求一个实例）
   - 替代方案：`get_owned` 对 scoped 只允许调用一次，第二次 panic（"one-shot"）

2. **Singleton owned**：`get_owned` 对 singleton 是 panic 还是返回错误？
   - 推荐：panic（与 `get()` 未注册时 panic 的行为一致）
   - 替代方案：返回 `Result<T, Error>`

3. **Owned 工厂 vs Arc 工厂 + try_unwrap**：
   - 方案 A：复用现有 `Arc` 工厂 + `try_unwrap`（最小改动）
   - 方案 B：新增 `OwnedServiceFactory`（语义清晰，需改 `ServiceEntry`）
   - 推荐：先 A 后 B

4. **`#[derive(Inject)]` 的 `&mut self`**：
   - rust-dicore 的 `Inject` trait `create()` 返回 `Self`（owned），handler 本身是 owned
   - 下游框架（rust-webapp）的 `IRequestHandler::handle` 需要从 `&self` 改为 `&mut self`
   - 这是否属于 rust-dicore 的职责范围？还是仅由下游框架处理？

5. **`T: Send` vs `T: Send + Sync`**：
   - owned 解析不需要 `Sync`（单线程访问）
   - 是否放宽到 `T: Send + 'static`？还是保持 `Send + Sync` 以统一？
   - 推荐：owned 方法用 `T: Send + 'static`，放宽限制

---

## 八、优先级

| 需求 | 优先级 | 理由 |
|------|--------|------|
| `ServiceProvider::get_owned::<T>() -> T` | P0 | 核心能力 |
| `Scope::get_owned::<T>() -> T` | P0 | web 场景必需 |
| `scoped_owned` / `transient_owned` 注册 | P1 | 语义清晰（方案 B） |
| `#[derive(Inject)]` 自动检测 owned 字段 | P1 | 开发体验 |
| Keyed owned (`get_keyed_owned`) | P2 | 多 DB 场景 |
| `IServiceResolver::get_owned_any` | P2 | trait 对象场景 |

---

## 九、总结

本需求要求 rust-dicore 增加 **owned 服务解析**能力（`get_owned::<T>() -> T`），使消费框架能注入 owned 实例而非 `Arc<T>`。

**核心价值**：
- 消除 interior mutability 妥协（`RwLock` + `unsafe`）
- 支持地道的 Rust `&mut self` 方法模式
- 保持 `Arc<T>` 共享注入的现有能力（向后兼容）
- 开发者体验：`ctx: DbContext`（owned）比 `ctx: Arc<DbContext>`（共享）更自然

**最小可行实现**：在 `ServiceProvider` 和 `Scope` 上添加 `get_owned::<T>() -> T` 方法，复用现有 `Arc` 工厂 + `Arc::try_unwrap`。改动量小，不破坏现有 API。
