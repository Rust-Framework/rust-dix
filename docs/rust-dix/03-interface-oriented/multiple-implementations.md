# 多实现与键控服务

## 问题场景

一个 trait 有多个实现，需要在运行时根据条件选择：
- 支付网关：微信支付、支付宝、信用卡
- 日志输出：文件日志、控制台日志、远程日志
- 数据存储：PostgreSQL、MySQL、SQLite

## rust-dix 解决方案：keyed 服务

### 注册多个实现

```rust
use rust_dix::*;
use std::sync::Arc;

// 接口
trait IPaymentGateway: Send + Sync {
    fn pay(&self, amount: f64) -> Result<String, String>;
    fn name(&self) -> &'static str;
}

// 实现一：微信支付
struct WechatPay;
impl IPaymentGateway for WechatPay {
    fn pay(&self, amount: f64) -> Result<String, String> {
        Ok(format!("微信支付：¥{}", amount))
    }
    fn name(&self) -> &'static str { "wechat" }
}

// 实现二：支付宝
struct Alipay;
impl IPaymentGateway for Alipay {
    fn pay(&self, amount: f64) -> Result<String, String> {
        Ok(format!("支付宝：¥{}", amount))
    }
    fn name(&self) -> &'static str { "alipay" }
}

// 注册：默认 + keyed
let provider = ServiceCollection::new()
    // 默认策略（singleton 注册，无 key）
    .singleton::<dyn IPaymentGateway>(|_| Arc::new(WechatPay))
    // 替代策略（keyed 注册）
    .keyed_singleton::<dyn IPaymentGateway>("wechat", |_| Arc::new(WechatPay))
    .keyed_singleton::<dyn IPaymentGateway>("alipay", |_| Arc::new(Alipay))
    .keyed_singleton::<dyn IPaymentGateway>("credit", |_| Arc::new(CreditCardPay))
    .build()
    .unwrap();
```

### 解析：按 key 选择实现

```rust
// 默认解析
let default: Arc<dyn IPaymentGateway> = provider.get().unwrap();
assert_eq!(default.name(), "wechat");

// 按名称选择
let alipay: Arc<dyn IPaymentGateway> = provider.get_keyed("alipay").unwrap();
assert_eq!(alipay.name(), "alipay");

// 安全解析（可选）
if let Some(gateway) = provider.try_get_keyed::<dyn IPaymentGateway>("credit") {
    gateway.pay(100.0);
}
```

### 批量获取：get_all / get_all_keyed

`get_all` **只返回非 keyed 注册**（对齐 MEDI `GetServices`）；keyed 实现要用
`get_all_keyed`（0.9.0 起只列**显式 key**，不含 `AnyKey` 兜底）。两者都含**已初始化**的
async singleton；同步枚举**未初始化** async singleton/transient → `AsyncServiceRequiresGetAsync`
（`get_all` typed panic），async scoped 在 root 被 scope 校验省略（Scope 内枚举才 panic），
`Vec` 注入对任何未初始化 async 返回 `Err`。

```rust
// 非 keyed 实现：上面的例子只注册了 1 个默认实现（WechatPay）
let all: Vec<Arc<dyn IPaymentGateway>> = provider.get_all();
assert_eq!(all.len(), 1);

// keyed 实现：一次取该类型的显式 keyed 注册（按注册顺序；不含 AnyKey 兜底）
let keyed: Vec<Arc<dyn IPaymentGateway>> = provider.get_all_keyed();
assert_eq!(keyed.len(), 3);  // wechat + alipay + credit

// 需要“全部实现”时按业务语义合并
for gateway in all.iter().chain(keyed.iter()) {
    println!("可用网关: {}", gateway.name());
}
```

> 单服务解析（`get` / `get_keyed`）为"最后注册优先"：默认实现（非 keyed）多次注册时
> 只有最后一个会被 `get` 返回，但 `get_all` 仍按注册顺序返回全部非 keyed 注册。

## 多态实现模式

### 模式 A：默认 + keyed（推荐）

```rust
ServiceCollection::new()
    .singleton::<dyn Trait>(|_| Arc::new(DefaultImpl))  // 默认
    .keyed_singleton::<dyn Trait>("alt1", |_| Arc::new(AltImpl1))  // 替代
    .keyed_singleton::<dyn Trait>("alt2", |_| Arc::new(AltImpl2))
    .build()
```

- `provider.get()` 返回默认实现
- `provider.get_keyed("alt1")` 返回指定实现

### 模式 B：全 keyed（无默认）

```rust
ServiceCollection::new()
    .keyed_singleton::<dyn Trait>("impl1", |_| Arc::new(Impl1))
    .keyed_singleton::<dyn Trait>("impl2", |_| Arc::new(Impl2))
    .keyed_singleton::<dyn Trait>("impl3", |_| Arc::new(Impl3))
    .build()
```

- 无 `provider.get()`，必须用 `get_keyed`
- 适合强制指定策略的场景

### 模式 C：运行时路由（策略工厂）

```rust
struct PaymentService {
    provider: Arc<ServiceProvider>,
    default_gateway: &'static str,
}

impl PaymentService {
    fn pay(&self, method: &str, amount: f64) -> Result<String, String> {
        // 尝试按指定方法获取，失败则回退默认
        let gateway: Arc<dyn IPaymentGateway> = 
            self.provider.try_get_keyed(method)
                .unwrap_or_else(|| self.provider.get().unwrap());
        gateway.pay(amount)
    }
}
```

## 实际应用示例

### 场景：多日志输出

```rust
trait ILogOutput: Send + Sync {
    fn write(&self, level: LogLevel, msg: &str);
}

struct FileOutput { path: String }
struct ConsoleOutput;
struct RemoteOutput { endpoint: String }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn ILogOutput>("file", |_| Arc::new(FileOutput { path: "/var/log/app.log" }))
    .keyed_singleton::<dyn ILogOutput>("console", |_| Arc::new(ConsoleOutput))
    .keyed_singleton::<dyn ILogOutput>("remote", |_| Arc::new(RemoteOutput { endpoint: "log-server" }))
    .build()
    .unwrap();

// 组合多个输出：全是 keyed 注册，所以用 get_all_keyed（get_all 只会返回空 Vec）
let outputs: Vec<Arc<dyn ILogOutput>> = provider.get_all_keyed();
for output in &outputs {
    output.write(LogLevel::Info, "服务启动");
}

// 注意（0.9.0）：AnyKey 兜底注册不计入 get_all_keyed（只列显式 key）
```

### 场景：多租户存储后端

```rust
trait IStorage: Send + Sync {
    fn save(&self, tenant: &str, data: &[u8]) -> Result<(), String>;
}

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IStorage>("tenant_a", |_| Arc::new(PostgresStorage))
    .keyed_singleton::<dyn IStorage>("tenant_b", |_| Arc::new(MysqlStorage))
    .build()
    .unwrap();

// 按租户选择存储
let storage: Arc<dyn IStorage> = provider.get_keyed(tenant_id).unwrap();
storage.save(tenant_id, data);
```

## key 与生命周期的组合

| 方法 | 生命周期 | 用途 |
|-----|---------|------|
| `keyed_singleton::<T>(k, f)` | Singleton | 全局共享的策略 |
| `keyed_transient::<T>(k, f)` | Transient | 每次创建的策略 |
| `keyed_scoped::<T>(k, f)` | Scoped | Scope 内共享的策略 |

```rust
// 策略实例每次新建
.keyed_transient::<dyn IAlgorithm>("sort", |_| Arc::new(QuickSort))

// 策略实例 Scope 内共享
.keyed_scoped::<dyn ICache>("session", |_| Arc::new(SessionCache))
```

## 类型化键与 AnyKey 兜底（0.9.0）

所有 keyed API 接受 `impl Into<ServiceKey>`：字符串调用保持兼容，也可用 enum/整数等
**类型化键**（键相等包含值的类型，`ServiceKey::new(1u32) != "1"`）。

> **字符串键易错点**：`ServiceKey::new("x")` 是 `&str` typed key，与
> `keyed_singleton("x", …)` 存储的 `String` key **不相等**。字符串键请用 `"x".into()` /
> `ServiceKey::from("x")`；`ServiceKey::new` 只用于有意的 typed 键（enum/整数/`&'static str`）。

```rust
#[derive(Debug, PartialEq, Eq, Hash)]
enum StorageKey { TenantA, TenantB }

let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IStorage>(ServiceKey::new(StorageKey::TenantA), |_| Arc::new(PostgresStorage))
    .keyed_singleton::<dyn IStorage>(ServiceKey::new(StorageKey::TenantB), |_| Arc::new(MysqlStorage))
    .build()
    .unwrap();

let storage: Arc<dyn IStorage> =
    provider.get_keyed(ServiceKey::new(StorageKey::TenantA)).unwrap();
```

**AnyKey 兜底**（对齐 MEDI `KeyedService.AnyKey`）：`keyed_any_singleton` /
`keyed_any_scoped` / `keyed_any_transient` 注册一个"没有精确 key 时使用"的实现，工厂接收
被请求的 key，singleton/scoped 实例**按 (服务注册/类型, 请求 key) 分别缓存**——同一注册 +
同一 key 复用，不同 key 或不同 AnyKey 注册各自独立（不会串缓存）：

```rust
let provider = ServiceCollection::new()
    .keyed_singleton::<dyn IStorage>("admin", |_| Arc::new(AdminStorage)) // 精确 key 优先
    .keyed_any_singleton::<dyn IStorage>(|_, key| {
        // key: &ServiceKey —— 按请求 key 选择租户
        Arc::new(TenantStorage::new(&format!("{key:?}")))
    })
    .build()
    .unwrap();

let admin: Arc<dyn IStorage> = provider.get_keyed("admin").unwrap();     // 精确
let tenant: Arc<dyn IStorage> = provider.get_keyed("tenant-42").unwrap(); // 兜底，按注册+key 缓存
```

> 注意（完整语义见 [0.9 语义权威页](../09-reference/semantics-0.9.md#2-anykey-兜底)）：
> - `get_all_keyed` 不含 AnyKey 兜底；`ServiceKey::any_key()` 不能做单值解析
>   （`is_registered_keyed(any_key())` → `false`）。枚举 async 的边界见参考页 §4。
> - AnyKey 仅同步注册、不做 eager 初始化/单独校验；`keyed_any_scoped` 被同步 singleton
>   依赖仍按 captive dependency 拒绝。
> - validation 预填充：build 期消费的 singleton 写入 **(注册, key) 缓存**，运行时同注册同 key
>   复用同一实例（单次构造）；owned 的 scoped/transient 在 strict root 允许（owned 例外）。
> - 工厂内嵌套解析**不同请求 key** 合法；只有同注册 + 同 key 才是环；`&ServiceKey` 无 typed
>   downcast 访问器（用 `Debug`/`Display`/`Eq`/`Hash`）。

## 最佳实践

| ✅ 推荐 | ❌ 避免 |
|--------|--------|
| 默认 + keyed 组合 | 只 keyed 无默认导致 `get()` 返回 `Err(ServiceNotFound)` |
| key 名称语义明确（如 "wechat"） | key 名称模糊（如 "impl1"） |
| 语义稳定的 key 用类型化 `ServiceKey::new(enum)` | 用字符串近似替代类型（`1u32` 与 `"1"` 不相等） |
| 用 `get_all` / `get_all_keyed` 批量处理观察者 | 手动逐个 `get_keyed` 或误以为 `get_all` 含 keyed |
| 需要"任意 key 兜底"时用 `keyed_any_*` | 用默认实现 + 手写回退（重复且易漏） |
| 策略选择逻辑集中封装 | 分散在各处的 if/else |

## 下一步

下一节：[第五章 设计模式实践](../05-design-patterns/INDEX.md) — 看策略模式如何与 DI 容器配合（具体模式小节规划中）。