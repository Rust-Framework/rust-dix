use thiserror::Error;
#[derive(Error, Debug, Clone)]
pub enum RdiError {
    #[error("service not registered: {0}")]
    ServiceNotFound(&'static str),
    #[error("keyed service not registered: key={key}, type={ty}")]
    KeyedServiceNotFound { key: String, ty: &'static str },
    #[error("circular dependency detected: {0}")]
    CircularDependency(String),
    #[error("singleton '{singleton}' cannot depend on scoped service '{scoped}'")]
    SingletonDependsOnScoped { singleton: String, scoped: String },
}
