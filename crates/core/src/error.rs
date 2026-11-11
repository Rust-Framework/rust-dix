use std::any::Any;
use thiserror::Error;

/// Errors returned by the container.
///
/// The enum is `#[non_exhaustive]`: future releases may add variants without a
/// semver-major break. Match with a wildcard arm (`_ => …`) when handling errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum RdiError {
    #[error("service not registered: {ty}{}", context.as_ref().map(|c| format!(" (while resolving {c})")).unwrap_or_default())]
    ServiceNotFound {
        ty: &'static str,
        context: Option<String>,
    },
    #[error("keyed service not registered: key={key}, type={ty}{}", context.as_ref().map(|c| format!(" (while resolving {c})")).unwrap_or_default())]
    KeyedServiceNotFound {
        key: String,
        ty: &'static str,
        context: Option<String>,
    },
    #[error("circular dependency detected: {0}")]
    CircularDependency(String),
    #[error("singleton '{singleton}' cannot depend on scoped service '{scoped}'")]
    SingletonDependsOnScoped { singleton: String, scoped: String },
    #[error("factory panicked while resolving service '{service}': {payload}")]
    FactoryPanic { service: String, payload: String },
    #[error("cannot resolve owned instance of '{0}': Singleton services cannot be owned")]
    OwnedNotSupported(&'static str),
    #[error(
        "async service '{service}' cannot be built with build(); use ServiceCollection::build_async() instead"
    )]
    AsyncServiceRequiresBuildAsync { service: String },
    #[error(
        "async service '{service}' must be resolved with get_async()/get_keyed_async(); \
         synchronous resolution is not available before its async initialization completes"
    )]
    AsyncServiceRequiresGetAsync { service: String },
    /// 同步 singleton 依赖 async singleton（build 期检测）。
    ///
    /// 同步构造函数无法 `await`，因此该依赖方向在架构上不成立：请把依赖方也改为
    /// async singleton，或在方法内用 `ScopeFactory` / `get_async` 延迟解析。
    #[error(
        "synchronous singleton '{singleton}' depends on async service '{async_service}': \
         a sync constructor cannot await; make '{singleton}' an async singleton or resolve \
         '{async_service}' lazily in a method (ScopeFactory / get_async)"
    )]
    SyncSingletonDependsOnAsyncService {
        singleton: String,
        async_service: String,
    },
    /// 严格 scope 校验下从 root provider 解析 Scoped 服务。
    ///
    /// 等价于 MEDI `ValidateScopes` 的
    /// `InvalidOperationException("Cannot resolve scoped service from root provider.")`。
    #[error(
        "scoped service '{service}' cannot be resolved from the root provider; \
         create a scope with provider.scope() and resolve it there, or build with \
         ServiceProviderOptions{{ validate_scopes: false }} to restore 0.7 root-scope behavior"
    )]
    ScopedFromRootProvider { service: String },
    /// provider 已被显式 [`dispose`](crate::ServiceProvider::dispose) 或已销毁。
    #[error("the service provider has been disposed; resolution is no longer allowed")]
    ProviderDisposed,
    /// scope 已被显式 [`dispose`](crate::Scope::dispose) 或 `Drop` 释放。
    #[error("the scope has been disposed; resolution is no longer allowed")]
    ScopeDisposed,
}

impl RdiError {
    /// Build a [`ServiceNotFound`](Self::ServiceNotFound) with an optional consumer context.
    pub fn not_found(ty: &'static str, context: impl Into<Option<String>>) -> Self {
        Self::ServiceNotFound {
            ty,
            context: context.into(),
        }
    }

    /// Build a [`KeyedServiceNotFound`](Self::KeyedServiceNotFound) with optional context.
    pub fn keyed_not_found(
        ty: &'static str,
        key: impl Into<String>,
        context: impl Into<Option<String>>,
    ) -> Self {
        Self::KeyedServiceNotFound {
            key: key.into(),
            ty,
            context: context.into(),
        }
    }

    /// Build a [`ScopedFromRootProvider`](Self::ScopedFromRootProvider) for `service`.
    pub fn scoped_from_root(service: &'static str) -> Self {
        Self::ScopedFromRootProvider {
            service: service.to_string(),
        }
    }

    /// Whether this error (or a wrapping [`FactoryPanic`](Self::FactoryPanic))
    /// represents a dependency cycle.
    ///
    /// When a user factory `unwrap()`s a nested `CircularDependency`, the
    /// top-level build error is a `FactoryPanic` whose payload contains the
    /// `CircularDependency` rendering. This helper is a **heuristic**: it also
    /// matches a `FactoryPanic` whose (user-provided) payload happens to contain
    /// that text. Prefer matching the typed variants directly when available.
    pub fn is_circular(&self) -> bool {
        match self {
            Self::CircularDependency(_) => true,
            Self::FactoryPanic { payload, .. } => payload.contains("CircularDependency"),
            _ => false,
        }
    }

    /// Convert a `panic::catch_unwind` payload into [`RdiError`].
    ///
    /// Prefers a panicked [`RdiError`] (from inject constructors via `panic_any`),
    /// then string payloads, otherwise wraps as [`FactoryPanic`](Self::FactoryPanic).
    pub fn from_panic_payload(service: &str, payload: Box<dyn Any + Send>) -> Self {
        match payload.downcast::<RdiError>() {
            Ok(err) => *err,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&'static str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                Self::FactoryPanic {
                    service: service.to_string(),
                    payload: msg,
                }
            }
        }
    }
}

/// Convert an inject `Result` into `Arc<T>`, panicking with [`RdiError`] as the
/// payload so runtime `catch_unwind` can recover a typed error.
///
/// Used by generated `__rdi_construct_*` wrappers and inventory factories.
#[doc(hidden)]
pub fn inject_arc<T: Send + Sync + 'static>(
    result: Result<std::sync::Arc<T>, RdiError>,
) -> std::sync::Arc<T> {
    match result {
        Ok(v) => v,
        Err(e) => std::panic::panic_any(e),
    }
}
