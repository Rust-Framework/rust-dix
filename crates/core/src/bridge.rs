//! Unified provider trait for service resolution and named registry access.

use std::any::Any;
use std::sync::Arc;

use crate::entry::IServiceResolver;
use crate::provider::ServiceProvider;
use crate::scope::Scope;
use crate::wrapper::ServiceProviderWrapper;

/// Unified provider trait for service resolution and named registry access.
///
/// `ServiceProvider`, `Scope`, and `ServiceProviderWrapper` all implement this
/// trait, enabling polymorphic dispatch via `Arc<dyn IProvider>` instead of
/// enum matching.
pub trait IProvider: IServiceResolver {
    fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>);
    fn rdi_remove_named(&self, name: &str);
}

impl IProvider for ServiceProvider {
    fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        ServiceProvider::get_named_any(self, name)
    }
    fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        ServiceProvider::rdi_register_named(self, name, service);
    }
    fn rdi_remove_named(&self, name: &str) {
        ServiceProvider::rdi_remove_named(self, name);
    }
}

impl IProvider for ServiceProviderWrapper {
    fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        ServiceProviderWrapper::get_named_any(self, name)
    }
    fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        ServiceProviderWrapper::rdi_register_named(self, name, service);
    }
    fn rdi_remove_named(&self, name: &str) {
        ServiceProviderWrapper::rdi_remove_named(self, name);
    }
}

impl IProvider for Scope {
    fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        Scope::get_named_any(self, name)
    }
    fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        Scope::rdi_register_named(self, name, service);
    }
    fn rdi_remove_named(&self, name: &str) {
        Scope::rdi_remove_named(self, name);
    }
}
