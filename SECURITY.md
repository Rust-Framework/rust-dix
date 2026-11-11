# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.9.x   | ✅        |
| 0.8.x   | ✅ (security fixes only, until 0.9 reaches 0.9.1) |
| < 0.8   | ❌        |

## Reporting a vulnerability

Please **do not** open a public issue for security-sensitive reports.
Use GitHub's private vulnerability reporting:

<https://github.com/Rust-Framework/rust-dix/security/advisories/new>

Include a description, the affected version(s), a proof of concept when
possible, and any suggested mitigation. We aim to acknowledge reports within
**72 hours** and to ship a fix or documented mitigation within **30 days** for
confirmed issues. We will credit reporters in the advisory unless asked not to.

## Security design notes

- **No `unsafe` code.** Both crates opt into `#![forbid(unsafe_code)]`.
  All synchronization uses the standard library's `Mutex` / `RwLock` /
  `OnceLock` primitives.
- **No network or filesystem access.** The container only manages in-memory
  service registration, resolution and disposal.
- **Panic containment.** Factory panics during resolution are converted into
  typed [`RdiError`] values; panics from user `Drop`/cleanup closures are
  collected so disposal completes and cannot double-panic into an abort.
- **Disposal contract.** A provider/scope must not be resolved from
  concurrently with `dispose()`; after disposal, resolution returns typed
  `ProviderDisposed` / `ScopeDisposed` errors. `get_owned` instances are
  caller-owned and are never tracked or disposed by the container.
- **Cycle safety.** Sync dependency cycles are detected at build time and at
  runtime. Async self/cross cycles are detected through a per-resolution chain
  (including across tasks when an `AsyncFactoryContext` is propagated), so a
  misconfigured graph returns `CircularDependency` instead of dead-locking.
- **Supply chain.** CI runs `cargo test`, `cargo clippy -D warnings`,
  `rustdoc -D warnings`, and a RustSec `cargo audit` on every push. Releases
  are published by GitHub Actions on `v*` tag pushes using the
  `CARGO_REGISTRY_TOKEN` secret (tag pushes are not signature-verified by the
  workflow; repository write access is required to create the tag).
- **MSRV.** The minimum supported Rust version is declared in `Cargo.toml`
  (`rust-version`) and verified in CI.

## Out of scope

- Vulnerabilities in user-provided factories/services.
- Denial of service caused by calling the container from `Drop`/cleanup
  closures after disposal, or by resolving concurrently with disposal
  (explicitly outside the documented contract).
- `get_owned` instances: the caller owns and must dispose those.
