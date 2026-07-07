//! Bridge implementations that adapt RDI types to the `sdk::ServiceLocator` trait.
//!
//! The `ServiceLocatorBridge` combines:
//! 1. RDI provider (compile-time registered services, type_name-based)
//! 2. Named service registry (string-based, for cross-DLL access) —
//!    stored directly in `ServiceProvider::named`.

use std::any::Any;
use std::sync::Arc;

use crate::entry::IServiceResolver;
use crate::provider::ServiceProvider;
use crate::scope::Scope;
use crate::service_locator::{INamedRegistrar, IServiceLocator};
use crate::wrapper::ServiceProviderWrapper;

/// Unified provider trait for service resolution and named registry access.
///
/// Replaces the former `RdiProvider` enum — `ServiceProvider`, `Scope`, and
/// `ServiceProviderWrapper` all implement this trait, enabling polymorphic
/// dispatch via `Arc<dyn IProvider>` instead of enum matching.
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

/// Bridges RDI into a single `IServiceLocator` implementation.
///
/// Resolution order for `get_any`:
/// 1. RDI provider (compile-time services)
///
/// Resolution order for `get_any_named`:
/// 1. Named registry in RDI provider
pub struct ServiceLocatorBridge {
    provider: Arc<dyn IProvider>,
}

impl ServiceLocatorBridge {
    pub fn new(provider: Arc<dyn IProvider>) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &Arc<dyn IProvider> {
        &self.provider
    }
}

impl IServiceLocator for ServiceLocatorBridge {
    fn get_any(&self, type_key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.provider.get_any(type_key)
    }

    fn get_any_named(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.provider.get_named_any(name)
    }

    fn register_named_any(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.provider.rdi_register_named(name, service);
    }

    fn remove_named(&self, name: &str) {
        self.provider.rdi_remove_named(name);
    }
}

impl INamedRegistrar for ServiceLocatorBridge {
    fn register_named_any(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.provider.rdi_register_named(name, service);
    }

    fn remove_named(&self, name: &str) {
        self.provider.rdi_remove_named(name);
    }
}

/// Implements `IServiceLocator` for standalone RDI types that don't need
/// the named registry.
macro_rules! impl_service_locator {
    ($ty:ty) => {
        impl IServiceLocator for $ty {
            fn get_any(&self, type_key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
                IServiceResolver::get_any(self, type_key)
            }

            fn get_any_named(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
                self.get_named_any(name)
            }

            fn register_named_any(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
                self.rdi_register_named(name, service);
            }

            fn remove_named(&self, name: &str) {
                self.rdi_remove_named(name);
            }
        }
    };
}

impl_service_locator!(ServiceProvider);
impl_service_locator!(crate::scope::Scope);
impl_service_locator!(ServiceProviderWrapper);