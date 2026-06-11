use crate::entry::IServiceResolver;
use crate::lifetime::ServiceLifetime;
use std::any::{Any, TypeId};
use std::sync::Arc;

/// A runtime service registration collected via `inventory::submit!`.
/// Each `#[lrdi::inject(...)]` on a struct generates one submission.
pub struct ServiceRegistration {
    pub lifetime: ServiceLifetime,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub factory: fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync>,
}

// Safety: fn pointer + enum + TypeId + &str are all Send + Sync + 'static
inventory::collect!(ServiceRegistration);
