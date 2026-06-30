use crate::lifetime::ServiceLifetime;
use crate::provider::ServiceProvider;
use std::any::Any;
use std::sync::Arc;

/// Service resolver trait — the core DI resolution interface.
///
/// Provides both type-erased and generic resolution methods.
pub trait IServiceResolver: Send + Sync {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;

    /// Type-erased polymorphic resolution: return all registered instances
    /// (default + keyed) of the given type.
    ///
    /// Used by `Vec<Arc<T>>` field injection (`#[inject]` on `Vec<Arc<T>>`)
    /// to collect all implementations — equivalent to C# MEDI
    /// `IEnumerable<IFoo>` constructor injection.
    ///
    /// Default empty `Vec` keeps external implementors backward-compatible.
    fn get_all_any(&self, _key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        Vec::new()
    }

    /// Return an `Arc<ServiceProvider>` for on-demand service resolution.
    ///
    /// Used by `#[inject(provider)]` to give a service access to the DI container
    /// so it can resolve scoped/owned instances on-demand (e.g., per-request
    /// `DbContext` in a web handler).
    ///
    /// Returns `None` if the resolver is not backed by a `ServiceProvider`
    /// (e.g., `ValidationResolver` during build-time validation) or if the
    /// provider was not created via [`ServiceProvider::into_arc`].
    ///
    /// Default `None` keeps external implementors backward-compatible.
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        None
    }

    /// Resolve a service by type (concrete or `dyn Trait`).
    /// Panics if not registered.
    fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Arc<T>
    where
        Self: Sized,
    {
        self.get_any(std::any::type_name::<T>())
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
            .unwrap_or_else(|| panic!("service not registered: {}", std::any::type_name::<T>()))
    }

    /// Resolve a service by type, returning `None` if not registered.
    fn try_get<T: ?Sized + Sync + Send + 'static>(&self) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        self.get_any(std::any::type_name::<T>())
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
    }

    /// Resolve a keyed service by type and key. Panics if not found.
    fn get_keyed<T: ?Sized + Sync + Send + 'static>(&self, variant: &str) -> Arc<T>
    where
        Self: Sized,
    {
        self.get_keyed_any(std::any::type_name::<T>(), variant)
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
            .unwrap_or_else(|| {
                panic!(
                    "keyed service not registered: {}:{}",
                    std::any::type_name::<T>(),
                    variant
                )
            })
    }

    /// Resolve a keyed service by type and key, returning `None` if not found.
    fn try_get_keyed<T: ?Sized + Sync + Send + 'static>(&self, variant: &str) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        self.get_keyed_any(std::any::type_name::<T>(), variant)
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
    }

    /// Resolve all registered instances of `T` (default + keyed).
    ///
    /// Equivalent to C# MEDI `GetServices<T>()`. Types implementing `IServiceResolver`
    /// get this for free via `get_all_any`; types with an inherent `get_all<T>`
    /// method (e.g. `ServiceProvider`, `Scope`) use that instead — Rust's method
    /// resolution gives inherent methods priority over trait methods.
    fn get_all<T: ?Sized + Sync + Send + 'static>(&self) -> Vec<Arc<T>>
    where
        Self: Sized,
    {
        self.get_all_any(std::any::type_name::<T>())
            .into_iter()
            .filter_map(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
            .collect()
    }

    /// Type-erased owned resolution: bypass cache, call factory fresh, return
    /// `Arc<dyn Any + Send + Sync>` (concrete `Arc<Arc<T>>`).
    ///
    /// - Singleton → `None` (shared singleton cannot be owned).
    /// - Scoped/Transient → factory invoked directly (no cache read/write),
    ///   caller downcasts + `Arc::try_unwrap` to obtain owned `T`.
    ///
    /// Default `None` keeps external implementors backward-compatible.
    fn get_owned_any(&self, _key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }
    fn get_keyed_owned_any(
        &self,
        _key: &str,
        _variant: &str,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

    /// Resolve owned `T` (bypass cache, fresh each call). Panics if not
    /// registered or the service is Singleton (shared instance cannot own).
    fn get_owned<T: Send + Sync + 'static>(&self) -> T
    where
        Self: Sized,
    {
        let arc_any = self
            .get_owned_any(std::any::type_name::<T>())
            .unwrap_or_else(|| {
                panic!(
                    "get_owned: not registered or Singleton: {}",
                    std::any::type_name::<T>()
                )
            });
        let arc_t: Arc<T> = arc_any
            .downcast::<Arc<T>>()
            .ok()
            .map(|d| Arc::clone(&*d))
            .unwrap_or_else(|| panic!("get_owned: type mismatch: {}", std::any::type_name::<T>()));
        Arc::try_unwrap(arc_t).unwrap_or_else(|_| {
            panic!(
                "get_owned: {} is shared (refcount > 1)",
                std::any::type_name::<T>()
            )
        })
    }

    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T>
    where
        Self: Sized,
    {
        let arc_any = self.get_owned_any(std::any::type_name::<T>())?;
        let arc_t: Arc<T> = arc_any.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d))?;
        Arc::try_unwrap(arc_t).ok()
    }

    /// Resolve owned keyed `T`. Panics if not found or Singleton.
    fn get_keyed_owned<T: Send + Sync + 'static>(&self, variant: &str) -> T
    where
        Self: Sized,
    {
        let arc_any = self
            .get_keyed_owned_any(std::any::type_name::<T>(), variant)
            .unwrap_or_else(|| {
                panic!(
                    "get_keyed_owned: not registered or Singleton: {}:{}",
                    std::any::type_name::<T>(),
                    variant
                )
            });
        let arc_t: Arc<T> = arc_any
            .downcast::<Arc<T>>()
            .ok()
            .map(|d| Arc::clone(&*d))
            .unwrap_or_else(|| {
                panic!(
                    "get_keyed_owned: type mismatch: {}:{}",
                    std::any::type_name::<T>(),
                    variant
                )
            });
        Arc::try_unwrap(arc_t).unwrap_or_else(|_| {
            panic!(
                "get_keyed_owned: {}:{} is shared (refcount > 1)",
                std::any::type_name::<T>(),
                variant
            )
        })
    }

    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, variant: &str) -> Option<T>
    where
        Self: Sized,
    {
        let arc_any = self.get_keyed_owned_any(std::any::type_name::<T>(), variant)?;
        let arc_t: Arc<T> = arc_any.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d))?;
        Arc::try_unwrap(arc_t).ok()
    }
}

pub struct ServiceEntry {
    pub cache_key: usize,
    pub key: Option<String>,
    pub type_name: &'static str, // kept for IServiceLocator string-based resolution
    pub factory: ServiceFactory,
    pub lifetime: ServiceLifetime,
}

pub type ServiceFactory =
    Arc<dyn Fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync> + Send + Sync>;
