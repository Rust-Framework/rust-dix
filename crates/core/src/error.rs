use thiserror::Error;
#[derive(Error, Debug, Clone)]
pub enum RdiError {
    #[error("service not registered: {ty}{}", context.as_ref().map(|c| format!(" (while resolving {c})")).unwrap_or_default())]
    ServiceNotFound {
        ty: &'static str,
        context: Option<String>,
    },
    #[error("keyed service not registered: key={key}, type={ty}")]
    KeyedServiceNotFound { key: String, ty: &'static str },
    #[error("circular dependency detected: {0}")]
    CircularDependency(String),
    #[error("singleton '{singleton}' cannot depend on scoped service '{scoped}'")]
    SingletonDependsOnScoped { singleton: String, scoped: String },
    #[error("factory panicked while resolving service '{service}': {payload}")]
    FactoryPanic { service: String, payload: String },
    #[error("cannot resolve owned instance of '{0}': Singleton services cannot be owned")]
    OwnedNotSupported(&'static str),
}
