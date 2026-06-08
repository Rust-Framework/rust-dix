<!-- markdownlint-disable MD033 MD041 -->

<div align="center">

# LRDI

**Rust Dependency Injection Framework** · **Rust 依赖注入框架**

[![Crates.io][crates-badge]][crates-url]
[![MIT License][license-badge]][license-url]

[Documentation][docs-url] · [Crates.io][crates-url] · [Repository][repo-url]

</div>

---

**LRDI** is a Rust dependency injection framework inspired by
**Microsoft.Extensions.DependencyInjection (MEDI)**.

**LRDI** 是一个受 **Microsoft.Extensions.DependencyInjection** 启发的 Rust 依赖注入框架。

This repository is a Cargo workspace that contains:

此仓库是一个 Cargo workspace，包含以下 crate：

| Crate | Description | 说明 |
|-------|-------------|------|
| [`lrdi`](crates/lrdi) | Main library — service collection, provider, scopes, resolver | 主库——服务集合、容器、作用域、解析器 |
| [`lrdi-macros`](crates/lrdi-macros) | Procedural macros — `#[derive(Inject)]`, `#[lrdi::module]`, `lrdi::inject!` | 过程宏——构造器注入、编译期模块扫描 |

## Quick Start · 快速开始

```rust
use lrdi::*;
use std::sync::Arc;

struct Logger { prefix: String }
struct Worker { logger: Arc<Logger> }

let provider = ServiceCollection::new()
    .singleton(|_| Arc::new(Logger { prefix: "INFO".into() }))
    .transient(|p| Arc::new(Worker { logger: p.get() }))
    .build()
    .unwrap();

let logger: Arc<Logger> = provider.get();
```

```toml
[dependencies]
lrdi = "0.1"
```

---

## Crate Layout · 项目结构

```
lrdi/
├── Cargo.toml                         # Workspace manifest
├── crates/
│   ├── lrdi/                          # Main crate
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/
│   │   └── tests/
│   └── lrdi-macros/                   # Proc-macro crate
│       ├── Cargo.toml
│       └── src/
├── publish.sh
├── rustfmt.toml
└── README.md                          # This file
```

For detailed documentation, see the [crates/lrdi/README.md](crates/lrdi/README.md).

详细文档请参阅 [crates/lrdi/README.md](crates/lrdi/README.md)。

---

## License · 许可

Licensed under the MIT License.

MIT 协议授权。

[crates-badge]: https://img.shields.io/crates/v/lrdi.svg
[crates-url]: https://crates.io/crates/lrdi
[license-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[license-url]: https://opensource.org/licenses/MIT
[docs-url]: https://docs.rs/lrdi
[repo-url]: https://gitcode.com/rf2026/RDI
