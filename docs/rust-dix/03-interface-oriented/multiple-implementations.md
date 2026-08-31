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

### 批量获取：get_all

```rust
// 获取所有实现
let all: Vec<Arc<dyn IPaymentGateway>> = provider.get_all();
assert_eq!(all.len(), 3);  // wechat + alipay + credit

// 遍历所有实现（如检查所有网关可用性）
for gateway in &all {
    println!("可用网关: {}", gateway.name());
}
```

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
    .keyed::<dyn ILogOutput>("file", |_| Arc::new(FileOutput { path: "/var/log/app.log" }))
    .keyed::<dyn ILogOutput>("console", |_| Arc::new(ConsoleOutput))
    .keyed::<dyn ILogOutput>("remote", |_| Arc::new(RemoteOutput { endpoint: "log-server" }))
    .build()
    .unwrap();

// 组合多个输出
let outputs: Vec<Arc<dyn ILogOutput>> = provider.get_all();
for output in &outputs {
    output.write(LogLevel::Info, "服务启动");
}
```

### 场景：多租户存储后端

```rust
trait IStorage: Send + Sync {
    fn save(&self, tenant: &str, data: &[u8]) -> Result<(), String>;
}

let provider = ServiceCollection::new()
    .keyed::<dyn IStorage>("tenant_a", |_| Arc::new(PostgresStorage))
    .keyed::<dyn IStorage>("tenant_b", |_| Arc::new(MysqlStorage))
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

## 最佳实践

| ✅ 推荐 | ❌ 避免 |
|--------|--------|
| 默认 + keyed 组合 | 只 keyed 无默认导致 get() panic |
| key 名称语义明确（如 "wechat"） | key 名称模糊（如 "impl1"） |
| 用 get_all 批量处理观察者 | 手动逐个 get_keyed |
| 策略选择逻辑集中封装 | 分散在各处的 if/else |

## 下一步

下一节：[第五章 设计模式实践](05-design-patterns/INDEX.md) — 看策略模式如何与 DI 容器配合。