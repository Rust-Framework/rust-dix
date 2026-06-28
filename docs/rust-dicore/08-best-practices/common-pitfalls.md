# 常见陷阱与排查

本文档列举 rust-dicore 开发中的高频错误及其排查方法。

## 陷阱 1：TypeId 不匹配

### 现象

```
panic: service not registered: dyn ILogger
```

### 原因

注册时的类型与解析时的类型不一致：

```rust
// 注册为具体类型
.singleton(|_| Arc::new(FileLogger))

// 解析为 trait
let logger: Arc<dyn ILogger> = provider.get();  // panic
```

### 排查

检查注册代码是否使用泛型参数 `::<dyn Trait>`：

```rust
// ✅ 修正
.singleton::<dyn ILogger>(|_| Arc::new(FileLogger))
```

## 陷阱 2：keyed 服务未找到

### 现象

```
panic: keyed service not registered: dyn IPayGateway:wechat
```

### 原因

- key 名称拼写错误
- keyed 服务未注册
- 注册为其他 key

### 排查

```rust
// 检查 key 是否正确
.keyed::<dyn IPayGateway>("wechat", ...)  // 注册 key = "wechat"
provider.get_keyed::<dyn IPayGateway>("wechat");  // 解析 key = "wechat"

// 安全解析
if let Some(gateway) = provider.try_get_keyed::<dyn IPayGateway>("wechat") {
    // ...
}
```

## 陷阱 3：生命周期依赖异常

### 现象

Scoped 服务在不同 Scope 返回不同实例，但 Singleton 内部状态异常。

### 原因

Singleton 服务依赖了 Scoped 服务：

```rust
#[rust_dicore::inject(singleton)]
struct GlobalService {
    session: Arc<Session>,  // Session 注册为 Scoped
}

// 从根 Provider 解析
let global: Arc<GlobalService> = provider.get();
global.session.id();  // 每次可能不同（等价 Transient）
```

### 排查

检查 Singleton 的字段依赖：

```rust
// ✅ Singleton 只依赖 Singleton
#[rust_dicore::inject(singleton)]
struct GlobalService {
    config: Arc<AppConfig>,  // Singleton
}
```

## 陷阱 4：循环依赖

### 现象

程序运行时栈溢出或死循环。

### 原因

服务 A 依赖 B，B 依赖 A：

```rust
ServiceCollection::new()
    .singleton::<dyn IServiceA>(|r| {
        let b: Arc<dyn IServiceB> = r.get();  // A 依赖 B
        Arc::new(ServiceA { b })
    })
    .singleton::<dyn IServiceB>(|r| {
        let a: Arc<dyn IServiceA> = r.get();  // B 依赖 A → 循环
        Arc::new(ServiceB { a })
    })
    .build()
    .unwrap();
```

### 排查

当前版本无循环依赖检测。解决方案：

1. **重构依赖方向**：消除循环
2. **延迟解析**：用 `#[rdi(provider)]` 按需解析

```rust
#[rust_dicore::inject(singleton)]
struct ServiceA {
    #[rdi(provider)]
    resolver: Arc<dyn IServiceResolver>,  // 按需解析
}

impl ServiceA {
    fn use_b(&self) {
        let b: Arc<dyn IServiceB> = self.resolver.get();  // 延迟解析
    }
}
```

## 陷阱 5：Scope 语义丢失

### 现象

Scoped 服务每次解析返回不同实例。

### 原因

从根 Provider 解析 Scoped 服务：

```rust
ServiceCollection::new()
    .scoped(|_| Arc::new(Session))
    .build()
    .unwrap();

let s1: Arc<Session> = provider.get();  // 根 Provider → 等价 Transient
let s2: Arc<Session> = provider.get();  // s1 != s2
```

### 排查

确保从 Scope 解析 Scoped 服务：

```rust
let scope = provider.create_scope();
let s1: Arc<Session> = scope.get();
let s2: Arc<Session> = scope.get();
// s1 == s2（Scope 内缓存）
```

## 陷阱 6：过程宏编译失败

### 现象

```
error: named struct or unit struct required
error: only structs are supported
```

### 原因

- 使用元组结构体（不支持）
- 使用 enum（不支持）

### 排查

```rust
// ❌ 元组结构体
#[rust_dicore::inject(singleton)]
struct Service(i32, String);  // 不支持

// ✅ 命名结构体
#[rust_dicore::inject(singleton)]
struct Service {
    value: i32,
    name: String,
}

// ✅ 单元结构体
#[rust_dicore::inject(singleton)]
struct Marker;
```

## 陷阱 7：重复 keyed key

### 现象

```
error: rdi-E004: duplicate key `wechat`
```

### 原因

`#[rust_dicore::module]` 中重复定义同名 keyed 服务：

```rust
#[rust_dicore::module]
mod services {
    rust_dicore::register!(keyed "wechat": singleton: dyn IPayGateway => WechatPay);
    rust_dicore::register!(keyed "wechat": singleton: dyn IPayGateway => Alipay);  // 重复
}
```

### 排查

确保每个 key 在 module 内唯一：

```rust
#[rust_dicore::module]
mod services {
    rust_dicore::register!(keyed "wechat": singleton: dyn IPayGateway => WechatPay);
    rust_dicore::register!(keyed "alipay": singleton: dyn IPayGateway => Alipay);  // 不同 key
}
```

## 陷阱 8：字段类型未注册

### 现象

```
panic: svc not registered
```

### 原因

过程宏生成的构造函数解析字段时，字段类型未注册：

```rust
#[rust_dicore::inject(singleton)]
struct Handler {
    user_svc: Arc<dyn IUserService>,  // IUserService 未注册
}
```

### 排查

确保所有依赖字段都已注册：

```rust
// ✅ 先注册依赖
#[rust_dicore::inject(singleton)]
struct UserService;

#[rust_dicore::inject]
impl IUserService for UserService { /* ... */ }

#[rust_dicore::inject(singleton)]
struct Handler {
    user_svc: Arc<dyn IUserService>,  // OK
}
```

## 排查速查表

| 现象 | 可能原因 | 排查方向 |
|------|---------|---------|
| `service not registered` | TypeId 不匹配 | 检查注册是否 `::<dyn Trait>` |
| `keyed service not registered` | key 错误或未注册 | 检查 key 拼写 |
| Scoped 行为异常 | Singleton 依赖 Scoped | 检查生命周期依赖 |
| 栈溢出 | 循环依赖 | 重构或延迟解析 |
| 编译失败 | 元组结构体 | 改命名结构体 |
| `duplicate key` | key 重复 | 确保 key 唯一 |
| `svc not registered` | 字段类型未注册 | 注册所有依赖 |

---

下一节：[反模式警示](anti-patterns.md)