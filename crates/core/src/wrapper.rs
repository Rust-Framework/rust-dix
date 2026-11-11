use crate::entry::IServiceResolver;
use crate::error::RdiError;
use crate::provider::ServiceProvider;
use std::any::{Any, TypeId};
use std::sync::Arc;

pub struct ServiceProviderWrapper {
    child: Arc<ServiceProvider>,
    root: Arc<ServiceProvider>,
}

impl ServiceProviderWrapper {
    pub fn new(child: Arc<ServiceProvider>, root: Arc<ServiceProvider>) -> Arc<Self> {
        Arc::new(Self { child, root })
    }
    pub fn child(&self) -> &Arc<ServiceProvider> {
        &self.child
    }
    pub fn root(&self) -> &Arc<ServiceProvider> {
        &self.root
    }

    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        // Child-first, root-fallback. Errors other than "not registered" (e.g.
        // root scope validation) must not be masked as ServiceNotFound.
        match self.child.get::<T>() {
            Ok(v) => Ok(v),
            Err(RdiError::ServiceNotFound { .. }) => self.root.get::<T>(),
            Err(err) => Err(err),
        }
    }
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.try_get::<T>()
    }
    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<crate::key::ServiceKey>,
    ) -> Result<Arc<T>, RdiError> {
        let key = key.into();
        match self.child.get_keyed::<T>(&key) {
            Ok(v) => Ok(v),
            Err(RdiError::KeyedServiceNotFound { .. }) => self.root.get_keyed::<T>(&key),
            Err(err) => Err(err),
        }
    }
    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        let mut r = self.child.get_all::<T>();
        r.extend(self.root.get_all::<T>());
        r
    }
    /// Unkeyed + keyed registrations are kept separate (MEDI parity): this
    /// returns child-then-root **keyed** instances only.
    pub fn get_all_keyed<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        let mut r = self.child.get_all_keyed::<T>();
        r.extend(self.root.get_all_keyed::<T>());
        r
    }
    /// Whether `T` is registered in the child or the root (MEDI
    /// `IServiceProviderIsService` parity).
    pub fn is_registered<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.child.is_registered::<T>() || self.root.is_registered::<T>()
    }
    /// Whether `T` is registered under `key` in the child or the root.
    pub fn is_registered_keyed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<crate::key::ServiceKey>,
    ) -> bool {
        let key = key.into();
        self.child.is_registered_keyed::<T>(&key) || self.root.is_registered_keyed::<T>(&key)
    }
    pub fn get_named<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.child
            .get_named::<T>(name)
            .or_else(|| self.root.get_named::<T>(name))
    }
    pub fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.child
            .get_named_any(name)
            .or_else(|| self.root.get_named_any(name))
    }

    /// Resolve owned `T` (bypass cache, fresh each call). Child-first, root-fallback.
    /// Returns `Err` if not registered or the service is Singleton.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        self.try_get_owned::<T>()
            .ok_or_else(|| RdiError::not_found(type_name, None))
    }
    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        let arc_any =
            IServiceResolver::get_owned_any(self.child.as_ref(), std::any::type_name::<T>())
                .or_else(|| {
                    IServiceResolver::get_owned_any(self.root.as_ref(), std::any::type_name::<T>())
                })?;
        let arc_t = crate::provider::ServiceProvider::extract::<T>(arc_any)?;
        Arc::try_unwrap(arc_t).ok()
    }
    /// Resolve owned keyed `T`. Child-first, root-fallback.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(
        &self,
        key: impl Into<crate::key::ServiceKey>,
    ) -> Result<T, RdiError> {
        let key = key.into();
        match self.child.get_keyed_owned::<T>(&key) {
            Ok(value) => Ok(value),
            Err(RdiError::KeyedServiceNotFound { .. }) => self.root.get_keyed_owned::<T>(&key),
            Err(err) => Err(err),
        }
    }
    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(
        &self,
        key: impl Into<crate::key::ServiceKey>,
    ) -> Option<T> {
        self.get_keyed_owned(key).ok()
    }

    /// Resolve an async service by type. Child-first, root-fallback.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        match self.child.get_async::<T>().await {
            Ok(svc) => Ok(svc),
            Err(_) => self.root.get_async::<T>().await,
        }
    }
    /// Resolve an async keyed service. Child-first, root-fallback.
    pub async fn get_keyed_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<crate::key::ServiceKey> + Send,
    ) -> Result<Arc<T>, RdiError> {
        let key = key.into();
        match self.child.get_keyed_async::<T>(&key).await {
            Ok(svc) => Ok(svc),
            Err(_) => self.root.get_keyed_async::<T>(key).await,
        }
    }

    fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.child
            .get_optional::<T>()
            .or_else(|| self.root.get_optional::<T>())
    }
}

impl IServiceResolver for ServiceProviderWrapper {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        IServiceResolver::get_any(self.child.as_ref(), key)
            .or_else(|| IServiceResolver::get_any(self.root.as_ref(), key))
    }
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        IServiceResolver::get_keyed_any(self.child.as_ref(), key, variant)
            .or_else(|| IServiceResolver::get_keyed_any(self.root.as_ref(), key, variant))
    }
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.child
            .get_by_type_id(tid)
            .or_else(|| self.root.get_by_type_id(tid))
    }
    fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.child
            .get_keyed_by_type_id(tid, key)
            .or_else(|| self.root.get_keyed_by_type_id(tid, key))
    }
    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        // 合并 child + root（非 keyed）：与 inherent `get_all` 的 child-then-root 顺序一致。
        let mut result = IServiceResolver::get_all_any(self.child.as_ref(), key);
        result.extend(IServiceResolver::get_all_any(self.root.as_ref(), key));
        result
    }
    fn get_all_keyed_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        let mut result = IServiceResolver::get_all_keyed_any(self.child.as_ref(), key);
        result.extend(IServiceResolver::get_all_keyed_any(self.root.as_ref(), key));
        result
    }
    fn get_all_any_result(&self, key: &str) -> Result<Vec<Arc<dyn Any + Send + Sync>>, RdiError> {
        let mut result = IServiceResolver::get_all_any_result(self.child.as_ref(), key)?;
        result.extend(IServiceResolver::get_all_any_result(
            self.root.as_ref(),
            key,
        )?);
        Ok(result)
    }
    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        IServiceResolver::get_owned_any(self.child.as_ref(), key)
            .or_else(|| IServiceResolver::get_owned_any(self.root.as_ref(), key))
    }
    fn try_get_any_result(
        &self,
        key: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        match self.child.try_get_any_result(key) {
            Ok(Some(v)) => Ok(Some(v)),
            _ => self.root.try_get_any_result(key),
        }
    }
    fn try_get_keyed_any_result(
        &self,
        key: &str,
        variant: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        match self.child.try_get_keyed_any_result(key, variant) {
            Ok(Some(v)) => Ok(Some(v)),
            _ => self.root.try_get_keyed_any_result(key, variant),
        }
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        IServiceResolver::get_keyed_owned_any(self.child.as_ref(), key, variant)
            .or_else(|| IServiceResolver::get_keyed_owned_any(self.root.as_ref(), key, variant))
    }
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        Some(Arc::clone(&self.child))
    }
}

impl ServiceProviderWrapper {
    /// Register a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.child.rdi_register_named(name, service);
    }

    /// Remove a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_remove_named(&self, name: &str) {
        self.child.rdi_remove_named(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::ServiceCollection;
    #[derive(Debug, PartialEq)]
    struct PO;
    #[derive(Debug, PartialEq)]
    struct RO {
        v: i32,
    }
    #[derive(Debug, PartialEq)]
    struct B {
        s: String,
    }
    #[test]
    fn child_prio() {
        let r = ServiceCollection::new()
            .singleton(|_| Arc::new(B { s: "root".into() }))
            .build()
            .unwrap();
        let c = ServiceCollection::new()
            .singleton(|_| Arc::new(B { s: "child".into() }))
            .build()
            .unwrap();
        let w = ServiceProviderWrapper::new(c, r);
        assert_eq!(w.get::<B>().unwrap().s, "child");
    }
    #[test]
    fn root_fallback() {
        let r = ServiceCollection::new()
            .singleton(|_| Arc::new(RO { v: 42 }))
            .build()
            .unwrap();
        let c = ServiceCollection::new().build().unwrap();
        let w = ServiceProviderWrapper::new(c, r);
        assert_eq!(w.get::<RO>().unwrap().v, 42);
    }
    #[test]
    fn child_invisible() {
        let r = ServiceCollection::new().build().unwrap();
        let c = ServiceCollection::new()
            .singleton(|_| Arc::new(PO))
            .build()
            .unwrap();
        let _w = ServiceProviderWrapper::new(c, r.clone());
        assert!(r.get_optional::<PO>().is_none());
    }
}
