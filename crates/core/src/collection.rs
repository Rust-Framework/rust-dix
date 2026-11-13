use crate::entry::{
    AsyncServiceFactory, KeyedServiceFactory, ServiceDescriptor, ServiceFactory, ServiceLifetime,
};
use crate::key::ServiceKey;
use crate::registration::ServiceRegistration;
use std::any::Any;
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
    /// String keys (`keyed_singleton("name", …)`) and typed keys
    /// (`keyed_singleton(ServiceKey::new(MyKey::A), …)`) are both accepted.
    pub fn keyed_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<ServiceKey>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_keyed(ServiceLifetime::Singleton, k.into(), f);
        self
    }

    /// Register a keyed transient service (new instance each resolution).
    pub fn keyed_transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<ServiceKey>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_keyed(ServiceLifetime::Transient, k.into(), f);
        self
    }

    /// Register a keyed scoped service (shared within a scope).
    pub fn keyed_scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        k: impl Into<ServiceKey>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_keyed(ServiceLifetime::Scoped, k.into(), f);
        self
    }

    /// Register an `AnyKey` fallback singleton (MEDI `KeyedService.AnyKey`).
    ///
    /// The factory receives the **requested** key and one instance is created
    /// and cached **per requested key**. Exact-key registrations take precedence.
    pub fn keyed_any_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver, &ServiceKey) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_any_keyed(ServiceLifetime::Singleton, f);
        self
    }

    /// Register an `AnyKey` fallback scoped service (one instance per requested
    /// key per scope).
    pub fn keyed_any_scoped<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver, &ServiceKey) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_any_keyed(ServiceLifetime::Scoped, f);
        self
    }

    /// Register an `AnyKey` fallback transient service (fresh instance per
    /// resolution, receives the requested key).
    pub fn keyed_any_transient<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(&dyn crate::entry::IServiceResolver, &ServiceKey) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.push_any_keyed(ServiceLifetime::Transient, f);
        self
    }

    /// Register an async singleton service (initialized once during `build_async`).
    ///
    /// The factory receives `Arc<ServiceProvider>`. To resolve services from
    /// within the factory, use `get_async`; to propagate the resolution context
    /// to spawned tasks (enabling cross-task cycle detection), call
    /// [`ServiceProvider::async_context`](crate::ServiceProvider::async_context).
    pub fn async_singleton<T: ?Sized + Send + Sync + 'static>(
        mut self,
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        k: impl Into<ServiceKey>,
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        k: impl Into<ServiceKey>,
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        k: impl Into<ServiceKey>,
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.push_async(ServiceLifetime::Scoped, Some(k.into()), f);
        self
    }

    pub fn try_add<T: ?Sized + Send + Sync + 'static>(
        self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.try_add_with_lifetime(ServiceLifetime::Singleton, f)
    }

    /// Register a singleton only if no unkeyed registration for `T` exists yet.
    pub fn try_add_singleton<T: ?Sized + Send + Sync + 'static>(
        self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.try_add_with_lifetime(ServiceLifetime::Singleton, f)
    }

    /// Register a scoped service only if no unkeyed registration for `T` exists yet.
    pub fn try_add_scoped<T: ?Sized + Send + Sync + 'static>(
        self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.try_add_with_lifetime(ServiceLifetime::Scoped, f)
    }

    /// Register a transient service only if no unkeyed registration for `T` exists yet.
    pub fn try_add_transient<T: ?Sized + Send + Sync + 'static>(
        self,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        self.try_add_with_lifetime(ServiceLifetime::Transient, f)
    }

    fn try_add_with_lifetime<T: ?Sized + Send + Sync + 'static>(
        mut self,
        lt: ServiceLifetime,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) -> Self {
        let tid = std::any::TypeId::of::<T>();
        if self
            .descriptors
            .iter()
            .any(|d| d.type_id == tid && d.key.is_none())
        {
            return self;
        }
        self.push(lt, None, f);
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

    /// Register a singleton instance of an already-created service.
    ///
    /// `T` may be unsized, so an existing trait-object `Arc<dyn Trait>` can be
    /// registered directly (`instance::<dyn IClock>(Arc::new(FakeClock))`) and
    /// resolved with `get::<dyn IClock>()`.
    pub fn instance<T: ?Sized + Send + Sync + 'static>(mut self, v: Arc<T>) -> Self {
        let ff: ServiceFactory = Arc::new(move |_| Arc::new(v.clone()));
        self.descriptors.push(ServiceDescriptor {
            type_id: std::any::TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key: None,
            factory: ff,
            keyed_factory: None,
            async_factory: None,
            lifetime: ServiceLifetime::Singleton,
        });
        self
    }

    pub fn singleton_value<T: Send + Sync + 'static>(self, v: T) -> Self {
        self.instance(Arc::new(v))
    }

    pub fn build(self) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
        self.build_with(crate::provider::ServiceProviderOptions::default())
    }

    /// Build with explicit [`ServiceProviderOptions`](crate::ServiceProviderOptions).
    ///
    /// `build()` equals `build_with(ServiceProviderOptions::default())`, i.e.
    /// strict scope validation is enabled by default (MEDI `ValidateScopes`).
    /// Use [`crate::ServiceProviderOptions::lenient`] to restore the ≤ 0.7
    /// root-scope behavior where the root provider caches scoped services.
    pub fn build_with(
        self,
        options: crate::provider::ServiceProviderOptions,
    ) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
        crate::provider::ServiceProvider::new_with(self.into_store(), options)
    }

    /// Build the provider, executing async factories for singleton services.
    ///
    /// Async singleton factories are run during build and their results are
    /// cached as singleton instances. Async transient/scoped factories are
    /// stored for later resolution.
    ///
    /// Use this instead of [`build`](Self::build) when any service requires
    /// async initialization (e.g., database connections, remote config).
    pub async fn build_async(
        self,
    ) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
        self.build_async_with(crate::provider::ServiceProviderOptions::default())
            .await
    }

    /// Async variant of [`build_with`](Self::build_with).
    pub async fn build_async_with(
        self,
        options: crate::provider::ServiceProviderOptions,
    ) -> Result<Arc<crate::provider::ServiceProvider>, crate::error::RdiError> {
        crate::provider::ServiceProvider::new_async_with(self.into_store(), options).await
    }

    /// Materialize descriptors into the by-TypeId `ServiceStore`.
    ///
    /// `cache_key` is the descriptor index (registration order); it drives
    /// deterministic eager initialization and reverse-order disposal.
    fn into_store(self) -> crate::entry::ServiceStore {
        let mut s = crate::entry::ServiceStore::new();
        for (n, d) in self.descriptors.into_iter().enumerate() {
            let e = crate::entry::ServiceEntry {
                cache_key: n,
                key: d.key,
                type_name: d.type_name,
                factory: d.factory,
                keyed_factory: d.keyed_factory,
                async_factory: d.async_factory,
                lifetime: d.lifetime,
            };
            s.entry(d.type_id).or_default().push(e);
        }
        s
    }

    /// Build a `ServiceCollection` from all `#[rust_dix::inject]` annotations
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
                keyed_factory: None,
                async_factory: None,
                lifetime: reg.lifetime,
            });
        }
        Self { descriptors }
    }

    /// Register a sync service (unkeyed).
    fn push<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        key: Option<ServiceKey>,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) {
        let sf: ServiceFactory = Arc::new(move |r| {
            let val: Arc<T> = (f)(r);
            Arc::new(val) as Arc<dyn Any + Send + Sync>
        });
        self.descriptors.push(ServiceDescriptor {
            type_id: std::any::TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key,
            factory: sf,
            keyed_factory: None,
            async_factory: None,
            lifetime: lt,
        });
    }

    /// Register a sync keyed service.
    fn push_keyed<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        key: ServiceKey,
        f: impl Fn(&dyn crate::entry::IServiceResolver) -> Arc<T> + Send + Sync + 'static,
    ) {
        self.push(lt, Some(key), f);
    }

    /// Register an `AnyKey` fallback sync service (key-aware factory).
    fn push_any_keyed<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        f: impl Fn(&dyn crate::entry::IServiceResolver, &ServiceKey) -> Arc<T> + Send + Sync + 'static,
    ) {
        let keyed: KeyedServiceFactory = Arc::new(move |r, key| {
            let val: Arc<T> = (f)(r, key);
            Arc::new(val) as Arc<dyn Any + Send + Sync>
        });
        // The plain factory must never run for an `AnyKey` registration; it only
        // exists because every `ServiceEntry` carries one. Running it would be a
        // framework bug, so fail loudly with a typed error.
        let stub: ServiceFactory = Arc::new(|_| {
            std::panic::panic_any(crate::error::RdiError::FactoryPanic {
                service: "AnyKey registration".into(),
                payload: "AnyKey registrations require a requested key and cannot be \
                          resolved without one"
                    .into(),
            })
        });
        self.descriptors.push(ServiceDescriptor {
            type_id: std::any::TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            key: Some(ServiceKey::any_key()),
            factory: stub,
            keyed_factory: Some(keyed),
            async_factory: None,
            lifetime: lt,
        });
    }

    /// Register an async service (legacy `Arc<ServiceProvider>` factory).
    fn push_async<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        lt: ServiceLifetime,
        key: Option<ServiceKey>,
        f: impl Fn(
                Arc<crate::provider::ServiceProvider>,
            ) -> Pin<Box<dyn Future<Output = Arc<T>> + Send>>
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
        // Sync factory: typed error if invoked before async initialization.
        let type_name = std::any::type_name::<T>();
        let sf: ServiceFactory = Arc::new(move |_| {
            std::panic::panic_any(crate::error::RdiError::AsyncServiceRequiresGetAsync {
                service: type_name.to_string(),
            })
        });
        self.descriptors.push(ServiceDescriptor {
            type_id: std::any::TypeId::of::<T>(),
            type_name,
            key,
            factory: sf,
            keyed_factory: None,
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
