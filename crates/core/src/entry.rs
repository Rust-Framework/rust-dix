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
///
/// # Panic contract of `Option`-returning lookups
///
/// [`Self::get_any`], [`Self::get_keyed_any`], [`Self::get_by_type_id`] and
/// [`Self::get_keyed_by_type_id`] return `Option` because they answer "found or
/// not". For a *registered* service that must not be resolved at this point
/// (root scope validation under `validate_scopes`, a disposed provider/scope,
/// circular dependency) implementations panic with a **typed [`RdiError`]**
/// payload instead of returning `None`, so required-resolution APIs
/// ([`Self::get`], `ServiceProvider::get`, …) can catch it and surface a precise
/// `Err`. Constructor injection relies on this to propagate the real cause
/// through user factories.
///
/// Infallible probing must use [`Self::try_get_any_result`] /
/// [`Self::try_get_keyed_any_result`] (or `try_get` / `try_get_keyed`), which
/// map such failures to `None`/`Err` without panicking.
pub trait IServiceResolver: Send + Sync {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>;
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>>;

    /// TypeId 直查（默认 `None`，`ServiceProvider` 等覆盖为 O(1) 直查）。
    ///
    /// 外部实现者若需优化可覆盖；不覆盖则 [`Self::get`] 默认实现 fallback
    /// 到 [`Self::get_any`] 字符串路径。
    ///
    /// 覆盖实现须遵循 trait 级 panic 契约（注册但当前不可解析 → typed
    /// [`RdiError`] payload panic，供 required API 捕获；可选探测请用
    /// [`Self::try_get_any_result`]）。
    fn get_by_type_id(&self, _tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }
    /// TypeId 直查 keyed 版本（默认 `None`）。
    ///
    /// 覆盖实现须遵循 trait 级 panic 契约（见 [`Self::get_by_type_id`]）。
    fn get_keyed_by_type_id(&self, _tid: TypeId, _key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

    /// Type-erased polymorphic resolution: return all **unkeyed** registered
    /// instances of the given type, in registration order.
    ///
    /// Equivalent to C# MEDI `GetServices<T>()` / `IEnumerable<IFoo>`
    /// constructor injection, which also excludes keyed services.
    /// Keyed implementations are enumerated with [`Self::get_all_keyed_any`].
    ///
    /// Default empty `Vec` keeps external implementors backward-compatible.
    fn get_all_any(&self, _key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        Vec::new()
    }

    /// Type-erased polymorphic resolution over **keyed** registrations only
    /// (MEDI `GetKeyedServices<T>(KeyedService.AnyKey)`).
    ///
    /// Default empty `Vec` keeps external implementors backward-compatible.
    fn get_all_keyed_any(&self, _key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        Vec::new()
    }

    /// Error-reporting variant of [`Self::get_all_any`], used by the inject
    /// macros for `Vec<Arc<T>>` fields.
    ///
    /// Containers that can fail (scope validation, disposal, factory errors)
    /// override it so constructor injection surfaces a typed [`RdiError`]
    /// instead of silently returning a partial/empty list. The default keeps
    /// external implementors source-compatible.
    fn get_all_any_result(&self, key: &str) -> Result<Vec<Arc<dyn Any + Send + Sync>>, RdiError> {
        Ok(self.get_all_any(key))
    }

    /// Return an `Arc<ServiceProvider>` for on-demand service resolution.
    ///
    /// Used by `#[inject(provider)]` to give a service access to the DI container
    /// so it can resolve scoped/owned instances on-demand (e.g., per-request
    /// `DbContext` in a web handler, or per-tick scopes in a singleton worker).
    ///
    /// Available during `build()` as well: validation executes singleton
    /// factories against the real provider, so `#[inject(provider)]` works in
    /// eagerly built singletons too.
    ///
    /// Returns `None` only for resolvers not backed by a `ServiceProvider`.
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

    /// Resolve a service by type, returning `None` if it is not registered or
    /// not resolvable at this point (e.g. a scoped service under root scope
    /// validation, or a disposed provider/scope).
    ///
    /// Uses [`Self::try_get_any_result`]; containers override it with an
    /// infallible lookup so `try_get` never panics for framework-level failures.
    fn try_get<T: ?Sized + Sync + Send + 'static>(&self) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        self.try_get_any_result(type_name)
            .ok()
            .flatten()
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
    }

    /// Fallible, type-erased lookup used by [`Self::try_get`].
    ///
    /// Default implementation delegates to [`Self::get_any`] (so external
    /// resolvers keep working); containers that can fail override it so
    /// infallible lookups can distinguish "absent" from "resolution error"
    /// without panicking.
    fn try_get_any_result(
        &self,
        key: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        Ok(self.get_any(key))
    }

    /// Resolve a keyed service by type and key. Returns `Err` if not found.
    fn get_keyed<T: ?Sized + Sync + Send + 'static>(
        &self,
        variant: &str,
    ) -> Result<Arc<T>, RdiError>
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

    /// Resolve a keyed service by type and key, returning `None` if not found or
    /// not resolvable at this point (e.g. root scope validation, disposed
    /// provider/scope). Never panics for framework-level failures.
    fn try_get_keyed<T: ?Sized + Sync + Send + 'static>(&self, variant: &str) -> Option<Arc<T>>
    where
        Self: Sized,
    {
        let type_name = std::any::type_name::<T>();
        self.try_get_keyed_any_result(type_name, variant)
            .ok()
            .flatten()
            .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
    }

    /// Fallible, type-erased keyed lookup used by [`Self::try_get_keyed`].
    ///
    /// Default implementation delegates to [`Self::get_keyed_any`]; containers
    /// override it to return `None`/`Err` without panicking.
    fn try_get_keyed_any_result(
        &self,
        key: &str,
        variant: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        Ok(self.get_keyed_any(key, variant))
    }

    /// Resolve all **unkeyed** registered instances of `T`, in registration
    /// order.
    ///
    /// Equivalent to C# MEDI `GetServices<T>()` (which also excludes keyed
    /// services). Types implementing `IServiceResolver` get this for free via
    /// `get_all_any`; types with an inherent `get_all<T>` method (e.g.
    /// `ServiceProvider`, `Scope`) use that instead — Rust's method resolution
    /// gives inherent methods priority over trait methods.
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
