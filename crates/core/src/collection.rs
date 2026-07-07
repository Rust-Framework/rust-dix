use crate::entry::{AsyncServiceFactory, ServiceDescriptor, ServiceFactory, ServiceLifetime};
use crate::registration::ServiceRegistration;
use std::any::{Any, TypeId};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub struct ServiceCollection {
    descriptors: Vec<ServiceDescriptor>,
}

impl ServiceCollection {
    pub fn new() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    pub fn singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Singleton, None, f);
        self
    }

    pub fn transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Transient, None, f);
        self
    }

    pub fn scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Scoped, None, f);
        self
    }

    /// Register a keyed singleton service.
    ///
    /// The key distinguishes multiple implementations of the same trait.
    /// Use `get_keyed::<T>(key)` or `try_get_keyed::<T>(key)` to resolve.
    pub fn keyed_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Singleton, Some(k.into()), f);
        self
    }

    /// Register a keyed transient service (new instance each resolution).
    pub fn keyed_transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Transient, Some(k.into()), f);
        self
    }

    /// Register a keyed scoped service (shared within a scope).
    pub fn keyed_scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(ServiceLifetime::Scoped, Some(k.into()), f);
        self
    }

    /// Register an async singleton service (initialized once during `build_async`).
    pub fn async_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Singleton, None, f);
        self
    }

    /// Register an async transient service (new async instance each resolution).
    pub fn async_transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Transient, None, f);
        self
    }

    /// Register an async scoped service (shared within a scope).
    pub fn async_scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Scoped, None, f);
        self
    }

    /// Register an async keyed singleton service.
    pub fn async_keyed_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Singleton, Some(k.into()), f);
        self
    }

    /// Register an async keyed transient service.
    pub fn async_keyed_transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Transient, Some(k.into()), f);
        self
    }

    /// Register an async keyed scoped service.
    pub fn async_keyed_scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<String>,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Scoped, Some(k.into()), f);
        self
    }

    pub fn try_add<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        let tid = TypeId::of::<T>();
        if self
            .descriptors
            .iter()
            .any(|d| d.type_id == tid && d.key.is_none())
        {
            return self;
        }
        self.push(ServiceLifetime::Singleton, None, f);
        self
    }

    pub fn add<T: ?Sized + Send + Sync + 'static>(
        mut self,
        lt: ServiceLifetime,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push(lt, None, f);
        self
    }

    pub fn instance<T: Send + Sync + 'static>(mut self, v: Arc<T>) -> Self {
        let ff: ServiceFactory = Arc::new(move |_| Arc::new(v.clone()));
        self.descriptors.push(ServiceDescriptor {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key: None,
            factory: ff,
            async_factory: None,
            lifetime: ServiceLifetime::Singleton,
        });
        self
    }

    pub fn singleton_value<T: Send + Sync + 'static>(self, v: T) -> Self {
        self.instance(Arc::new(v))
    }

    pub fn build(self) -> Result<crate::provider::ServiceProvider, crate::error::RdiError> {
        let mut s = crate::entry::ServiceStore::new();
        for (n, d) in self.descriptors.into_iter().enumerate() {
            let e = crate::entry::ServiceEntry {
                cache_key: n,
                key: d.key,
                type_name: d.type_name,
                factory: d.factory,
                async_factory: d.async_factory,
                lifetime: d.lifetime,
            };
            s.entry(d.type_id).or_default().push(e);
        }
        crate::provider::ServiceProvider::new(s)
    }

    /// Build the provider, executing async factories for singleton services.
    ///
    /// Async singleton factories are run during build and their results are
    /// cached as singleton instances. Async transient/scoped factories are
    /// stored for later resolution.
    ///
    /// Returns `Arc<ServiceProvider>` so that `provider_arc()` works for
    /// async resolution via `get_async` / `get_keyed_async`.
    ///
    /// Use this instead of [`build`](Self::build) when any service requires
    /// async initialization (e.g., database connections, remote config).
    pub async fn build_async(
        self,
    ) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
        crate::provider::ServiceProvider::new_async(self.descriptors).await
    }

    /// Build a `ServiceCollection` from all `#[rust_dicore::inject]` annotations
    /// in the current binary (across all crates).
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    pub fn from_injected() -> Self {
        let mut descriptors = Vec::new();
        for reg in inventory::iter::<ServiceRegistration> {
            let factory: ServiceFactory = Arc::new(move |r| (reg.factory)(r));
            descriptors.push(ServiceDescriptor {
                type_id: reg.type_id,
                type_name: (reg.type_name_fn)(),
                key: None,
                factory,
                async_factory: None,
                lifetime: reg.lifetime,
            });
        }
        Self { descriptors }
    }

    /// Register a new singleton service.
    fn push<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        key: Option<String>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) {
        let sf: ServiceFactory = Arc::new(move |r| {
            let val: Arc<T> = (f)(r);
            Arc::new(val) as Arc<dyn Any + Send + Sync>
        });
        self.descriptors.push(ServiceDescriptor {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key,
            factory: sf,
            async_factory: None,
            lifetime: lt,
        });
    }

    /// Register an async service.
    fn push_async<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        key: Option<String>,
        f: impl Fn(Arc<crate::provider::ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        let af: AsyncServiceFactory = Arc::new(move |sp: Arc<crate::provider::ServiceProvider>| {
            let fut = (f)(sp);
            Box::pin(async move {
                let val: Arc<T> = fut.await;
                Arc::new(val) as Arc<dyn Any + Send + Sync>
            }) as Pin<Box<dyn Future<Output = Arc<dyn Any + Send + Sync>> + Send>>
        });
        // Sync factory: panics if called directly (user must use build_async).
        let type_name = std::any::type_name::<T>();
        let sf: ServiceFactory = Arc::new(move |_| {
            panic!(
                "async service '{}' resolved via sync path; use build_async() instead",
                type_name
            )
        });
        self.descriptors.push(ServiceDescriptor {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key,
            factory: sf,
            async_factory: Some(af),
            lifetime: lt,
        });
    }
}
impl Default for ServiceCollection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, PartialEq)]
    struct G {
        n: String,
    }
    #[derive(Debug, PartialEq)]
    struct C {
        v: i32,
    }
    #[test]
    fn empty() {
        let p = ServiceCollection::new().build().unwrap();
        assert!(p.get_optional::<G>().is_none());
    }
    #[test]
    fn singleton() {
        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(G { n: "Hi".into() }))
            .build()
            .unwrap();
        assert_eq!(p.get::<G>().unwrap().n, "Hi");
    }
    #[test]
    fn singleton_caches() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CNT: AtomicUsize = AtomicUsize::new(0);
        let p = ServiceCollection::new()
            .singleton(|_| {
                CNT.fetch_add(1, Ordering::SeqCst);
                Arc::new(C { v: 42 })
            })
            .build()
            .unwrap();
        let _ = p.get::<C>();
        let _ = p.get::<C>();
        assert_eq!(CNT.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn transient_not_cached() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CNT: AtomicUsize = AtomicUsize::new(0);
        let p = ServiceCollection::new()
            .transient(|_| {
                CNT.fetch_add(1, Ordering::SeqCst);
                Arc::new(C { v: 1 })
            })
            .build()
            .unwrap();
        let _ = p.get::<C>();
        let _ = p.get::<C>();
        assert_eq!(CNT.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn instance() {
        let g = Arc::new(G { n: "Inst".into() });
        let p = ServiceCollection::new()
            .instance(g.clone())
            .build()
            .unwrap();
        assert!(Arc::ptr_eq(&g, &p.get::<G>().unwrap()));
    }
    #[test]
    fn keyed_svc() {
        let p = ServiceCollection::new()
            .keyed_singleton("a", |_| Arc::new(G { n: "A".into() }))
            .keyed_singleton("b", |_| Arc::new(G { n: "B".into() }))
            .build()
            .unwrap();
        assert_eq!(p.get_keyed::<G>("a").unwrap().n, "A");
        assert_eq!(p.get_keyed::<G>("b").unwrap().n, "B");
    }
    #[test]
    fn get_all() {
        let p = ServiceCollection::new()
            .keyed_singleton("x", |_| Arc::new(C { v: 1 }))
            .keyed_singleton("y", |_| Arc::new(C { v: 2 }))
            .build()
            .unwrap();
        assert_eq!(p.get_all::<C>().len(), 2);
    }
    #[test]
    fn try_add_skips() {
        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(G { n: "First".into() }))
            .try_add(|_| Arc::new(G { n: "Second".into() }))
            .build()
            .unwrap();
        assert_eq!(p.get::<G>().unwrap().n, "First");
    }
}
