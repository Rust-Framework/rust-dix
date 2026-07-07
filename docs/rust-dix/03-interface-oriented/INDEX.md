# 第三章 面向接口编程范式

本章讲解 rust-dix 推荐的面向接口开发模式——这是实现松耦合、可测试、可扩展架构的核心。

## 本章小节

| 小节 | 内容 |
|------|------|
| [为什么面向接口](why-interface.md) | 接口开发的价值与 Rust trait 对比 |
| [trait 定义与 contracts 层](contracts-layer.md) | 接口与 DTO 的分层组织 |
| [dyn Trait 注册与解析](dyn-trait-registration.md) | 接口注册的核心模式 |
| [多实现与键控服务](multiple-implementations.md) | 策略模式、多态实现 |

## 核心原则

### ✅ 正确做法：依赖 trait，注册为 trait

```rust
// contracts/user.rs — 接口层
pub trait IUserService: Send + Sync {
    fn create(&self, name: &str) -> Result<User, String>;
    fn find(&self, id: u64) -> Result<User, String>;
}

// handlers/user.rs — 实现层
#[rust_dix::inject(singleton)]
pub struct UserService {
    db: Arc<dyn IDbContext>,  // 依赖接口，不依赖具体类型
}

#[rust_dix::inject]
impl IUserService for UserService { ... }

// handlers/user_handler.rs — Handler 注入接口
#[rust_dix::inject(singleton)]
pub struct CreateUserHandler {
    user_svc: Arc<dyn IUserService>,  // 接口依赖
}

#[rust_dix::inject]
impl IRequestHandler<CreateUserRequest, User> for CreateUserHandler { /* ... */ }
```

### ❌ 反模式：依赖具体类型

```rust
// ❌ Handler 依赖具体实现
pub struct CreateUserHandler {
    user_svc: Arc<UserService>,  // 错误：暴露具体类型
}

// ❌ 注册为具体类型
ServiceCollection::new()
    .singleton(|_| Arc::new(UserService))  // 无法按 trait 解析
```

## 下一步

从 [为什么面向接口](why-interface.md) 开始。