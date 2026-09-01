use crate::error::RdiError;
use crate::provider::ServiceProvider;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Service lifetime enum — defines how instances are cached.
///
/// - `Transient`: new instance every resolution (never cached)
/// - `Scoped`: one instance per scope (cached within the scope)
/// - `Singleton`: one instance shared across all scopes (cached globally)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceLifetime {
    Transient,
    Scoped,
    Singleton,
}

/// Service descriptor holds registration metadata for a service.
///
/// Created during `ServiceCollection` building and passed to `ServiceStore`
/// to create `ServiceEntry`.
#[derive(Clone)]
pub struct ServiceDescriptor {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub key: Option<String>,
    pub factory: ServiceFactory,
    pub async_factory: Option<AsyncServiceFactory>,
    pub lifetime: ServiceLifetime,
}

/// ServiceStore is the root registry — maps `TypeId` to all `ServiceEntry` entries.
///
/// Multiple entries per type exist when keyed variants are registered.
pub type ServiceStore = HashMap<TypeId, Vec<ServiceEntry>>;

/// Service resolver trait — the core DI resolution interface.
///
/// Provides both type-erased and generic resolution methods.
pub trait IServiceResolver: Send + Sync {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;

    /// TypeId 直查（默认 `None`，`ServiceProvider` 等覆盖为 O(1) 直查）。
    ///
    /// 外部实现者若需优化可覆盖；不覆盖则 [`Self::get`] / [`Self::try_get`]
    /// 等默认实现 fallback 到 [`Self::get_any`] 字符串路径。
    fn get_by_type_id(&self, _tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }
    /// TypeId 直查 keyed 版本（默认 `None`）。
    fn get_keyed_by_type_id(
        &self,
        _tid: TypeId,
        _key: &str,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

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
    /// (e.g., `ValidationResolver` during build-time validation).
    ///
    /// Default `None` keeps external implementors backward-compatible.
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        None
    }

    /// Resolve a service by type (concrete or `dyn Trait`).
    /// Returns `Err(RdiError::ServiceNotFound)` if not registered.
    ///
    /// 优先走 `get_by_type_id`（O(1) TypeId 直查），未覆盖时 fallback
    /// 到 `get_any` 字符串路径。
    fn get<T: ?Sized + Sync + Send + 'static>(&self) -> Result<Arc<T>, RdiError>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        self.get_by_type_id(tid)
            .or_else(|| self.get_any(type_name))
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
            .ok_or_else(|| RdiError::not_found(type_name, None))
    }

    /// Resolve a service by type, returning `None` if not registered.
    fn try_get<T: ?Sized + Sync + Send + 'static>(&self) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        self.get_by_type_id(tid)
            .or_else(|| self.get_any(type_name))
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
    }

    /// Resolve a keyed service by type and key. Returns `Err` if not found.
    fn get_keyed<T: ?Sized + Sync + Send + 'static>(&self, variant: &str) -> Result<Arc<T>, RdiError>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        self.get_keyed_by_type_id(tid, variant)
            .or_else(|| self.get_keyed_any(type_name, variant))
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
            .ok_or_else(|| RdiError::keyed_not_found(type_name, variant, None))
    }

    /// Resolve a keyed service by type and key, returning `None` if not found.
    fn try_get_keyed<T: ?Sized + Sync + Send + 'static>(&self, variant: &str) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        self.get_keyed_by_type_id(tid, variant)
            .or_else(|| self.get_keyed_any(type_name, variant))
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

    /// Resolve owned `T` (bypass cache, fresh each call).
    /// Returns `Err` if not registered or the service is Singleton.
    fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let arc_any = self
            .get_owned_any(type_name)
            .ok_or_else(|| RdiError::not_found(type_name, None))?;
        let arc_t: Arc<T> = arc_any
            .downcast::<Arc<T>>()
            .ok()
            .map(|d| Arc::clone(&*d))
            .ok_or_else(|| RdiError::not_found(type_name, None))?;
        Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(type_name))
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

    /// Resolve owned keyed `T`.
    /// Returns `Err` if not found or Singleton.
    fn get_keyed_owned<T: Send + Sync + 'static>(&self, variant: &str) -> Result<T, RdiError>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        let arc_any = self
            .get_keyed_owned_any(type_name, variant)
            .ok_or_else(|| RdiError::keyed_not_found(type_name, variant, None))?;
        let arc_t: Arc<T> = arc_any
            .downcast::<Arc<T>>()
            .ok()
            .map(|d| Arc::clone(&*d))
            .ok_or_else(|| RdiError::keyed_not_found(type_name, variant, None))?;
        Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(type_name))
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
    pub type_name: &'static str, // kept for IServiceResolver string-based resolution
    pub factory: ServiceFactory,
    pub async_factory: Option<AsyncServiceFactory>,
    pub lifetime: ServiceLifetime,
}

pub type ServiceFactory =
    Arc<dyn Fn(&dyn IServiceResolver) -> Arc<dyn Any + Send + Sync> + Send + Sync>;

/// Async variant of [`ServiceFactory`] — for services that require async
/// initialization (e.g., database connections, remote config loading).
///
/// Used with [`ServiceCollection::build_async`](crate::collection::ServiceCollection::build_async).
/// The factory receives an `Arc<ServiceProvider>` (instead of `&dyn IServiceResolver`)
/// because async closures cannot borrow from the caller's stack frame.
pub type AsyncServiceFactory = Arc<
    dyn Fn(Arc<ServiceProvider>) -> Pin<Box<dyn Future<Output = Arc<dyn Any + Send + Sync>> + Send>>
        + Send
        + Sync,
>;
