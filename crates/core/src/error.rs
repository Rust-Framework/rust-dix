use std::any::Any;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
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
