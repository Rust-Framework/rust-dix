use crate::cache::{AnyService, LazyCache};
use crate::entry::{
    AsyncServiceFactory, IServiceResolver, ServiceDescriptor, ServiceEntry, ServiceFactory,
    ServiceLifetime, ServiceStore,
};
use crate::error::RdiError;
use crate::validation::ValidationResolver;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

pub struct ServiceProvider {
    /// Service registry. Shared across root + scope-derived providers via `Arc`.
    store: Arc<ServiceStore>,
    /// String → TypeId lookup for IServiceLocator string-based resolution.
    /// Shared across root + scope-derived providers via `Arc`.
    type_map: Arc<HashMap<&'static str, TypeId>>,
    /// Singleton 实例缓存（per-key `OnceLock`，多线程下工厂只执行一次）。
    /// build 阶段 validation + Phase 2 eager init 预热，运行时 lazy fallback。
    /// Shared across root + scope-derived providers via `Arc`（全局单例语义）。
    singleton_cache: Arc<LazyCache>,
    /// Root scope 的 Scoped 实例缓存。
    ///
    /// ServiceProvider 本身即 MEDI 的 root scope：从根直接解析 Scoped 服务时，
    /// 实例在此缓存中复用，语义等同于在根 scope 内单例化。
    /// 子 Scope（通过 `create_scope()` 创建）拥有独立的 scoped_cache，不回退到此缓存。
    /// per-key `OnceLock` 保证多线程下工厂只执行一次，无 TOCTOU 竞态。
    /// **每个 provider 独立**：root 与 scope-derived 各自持有一份。
    root_scoped_cache: LazyCache,
    /// String-keyed registry for cross-DLL (cdylib) service access.
    /// Rust's `TypeId` differs across compilation units, so named
    /// lookup is the only reliable mechanism for plugin services.
    /// Shared across root + scope-derived providers via `Arc`.
    pub(crate) named: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    /// Self-referential weak pointer, initialized by [`Self::into_arc`] or
    /// [`Self::new_scope`]. Enables [`IServiceResolver::provider_arc`] to hand
    /// out `Arc<ServiceProvider>` to services using `#[inject(provider)]`.
    /// **每个 provider 独立**：scope-derived provider 的 self_weak 指向自身。
    self_weak: OnceLock<Weak<ServiceProvider>>,
    /// Cleanup callbacks for scoped services implementing [`Disposable`](crate::scope::Disposable).
    /// Populated by [`Self::register_disposable`], drained by [`Self::dispose_scoped`].
    /// **每个 provider 独立**：scope-derived provider 的 disposables 绑定到当前 scope。
    disposables: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl ServiceProvider {
    pub(crate) fn new(store: ServiceStore) -> Result<Self, RdiError> {
        // Build type_name → TypeId lookup table for string-based resolution
        let mut type_map = HashMap::new();
        for (&tid, entries) in &store {
            if let Some(e) = entries.first() {
                type_map.entry(e.type_name).or_insert(tid);
            }
        }

        // Phase 0: Validate dependencies (cycle detection).
        // The validation resolver executes singleton factories to detect
        // cycles, and caches the resulting instances so Phase 2 can reuse
        // them instead of re-executing the factories (which would double-fire
        // side effects like counters / log lines).
        let singleton_cache = Arc::new(LazyCache::new());
        Self::validate_dependencies(&store, &singleton_cache)?;

        // Phase 1: collect all singleton entries (skip async — handled by build_async).
        let singleton_entries: Vec<(usize, ServiceFactory)> = store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_none())
            .map(|e| (e.cache_key, e.factory.clone()))
            .collect();

        // Phase 2: eagerly execute all singleton factories.
        // Reuse instances already built during validation; only execute
        // factories whose results are not yet cached.
        let sp = Self {
            store: Arc::new(store),
            type_map: Arc::new(type_map),
            singleton_cache,
            root_scoped_cache: LazyCache::new(),
            named: Arc::new(RwLock::new(HashMap::new())),
            self_weak: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
        };

        for (ck, factory) in &singleton_entries {
            // LazyCache::get_or_init_with 内部查缓存，validation 已构建的跳过。
            let resolver = &sp as &dyn IServiceResolver;
            let factory = factory.clone();
            sp.singleton_cache
                .get_or_init_with(*ck, move || factory(resolver));
        }

        Ok(sp)
    }

    /// Build a `ServiceProvider` with async factory support.
    ///
    /// Async singleton factories are executed during build and their results
    /// are cached. Async transient/scoped factories are stored for later
    /// resolution via `get_async` / `get_keyed_async`.
    ///
    /// Returns `Arc<ServiceProvider>` so that `provider_arc()` works for
    /// async resolution (async factories receive `Arc<ServiceProvider>`).
    pub(crate) async fn new_async(
        descriptors: Vec<ServiceDescriptor>,
    ) -> Result<Arc<Self>, RdiError> {
        // Build ServiceStore
        let mut store = ServiceStore::new();
        for (n, d) in descriptors.into_iter().enumerate() {
            let e = ServiceEntry {
                cache_key: n,
                key: d.key,
                type_name: d.type_name,
                factory: d.factory,
                async_factory: d.async_factory,
                lifetime: d.lifetime,
            };
            store.entry(d.type_id).or_default().push(e);
        }

        // Build type_name → TypeId lookup
        let mut type_map = HashMap::new();
        for (&tid, entries) in &store {
            if let Some(e) = entries.first() {
                type_map.entry(e.type_name).or_insert(tid);
            }
        }

        // Phase 0: Validate sync singleton dependencies (async singletons skipped).
        let singleton_cache = Arc::new(LazyCache::new());
        Self::validate_dependencies(&store, &singleton_cache)?;

        // Phase 1: create the provider in an Arc (self_weak stays valid).
        let sp_arc = Arc::new(Self {
            store: Arc::new(store),
            type_map: Arc::new(type_map),
            singleton_cache,
            root_scoped_cache: LazyCache::new(),
            named: Arc::new(RwLock::new(HashMap::new())),
            self_weak: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
        });
        sp_arc
            .self_weak
            .get_or_init(|| Arc::downgrade(&sp_arc));

        // Phase 2: eagerly execute sync singleton factories.
        let singleton_entries: Vec<(usize, ServiceFactory)> = sp_arc
            .store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_none())
            .map(|e| (e.cache_key, e.factory.clone()))
            .collect();

        for (ck, factory) in &singleton_entries {
            let resolver = sp_arc.as_ref() as &dyn IServiceResolver;
            let factory = factory.clone();
            sp_arc
                .singleton_cache
                .get_or_init_with(*ck, move || factory(resolver));
        }

        // Phase 3: execute async singleton factories.
        let async_singletons: Vec<(usize, AsyncServiceFactory)> = sp_arc
            .store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_some())
            .map(|e| (e.cache_key, e.async_factory.clone().unwrap()))
            .collect();

        for (ck, async_factory) in async_singletons {
            let result = async_factory(Arc::clone(&sp_arc)).await;
            sp_arc.singleton_cache.get_or_init_with(ck, || result);
        }

        Ok(sp_arc)
    }

    /// 创建一个 scope 范围的 ServiceProvider，共享 store/type_map/singleton_cache/named，
    /// 独立 root_scoped_cache 和 self_weak。
    ///
    /// 供 [`Scope::new`](crate::scope::Scope::new) 调用。这使 `#[inject(provider)]`
    /// 字段拿到的是 **scope 范围的 provider** 而非根 provider——经该字段解析 Scoped 服务
    /// 会命中 scope 自己的 `root_scoped_cache`，符合 MEDI `IServiceScope` 语义。
    ///
    /// 与 [`Self::into_arc`] 一样初始化 self_weak，使 scope provider 自身可被
    /// `provider_arc()` 返回。
    pub(crate) fn new_scope(parent: &Arc<Self>) -> Arc<Self> {
        let sp = Self {
            store: Arc::clone(&parent.store),
            type_map: Arc::clone(&parent.type_map),
            singleton_cache: Arc::clone(&parent.singleton_cache),
            root_scoped_cache: LazyCache::new(),
            named: Arc::clone(&parent.named),
            self_weak: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
        };
        let arc = Arc::new(sp);
        let this: &Self = &arc;
        this.self_weak.get_or_init(|| Arc::downgrade(&arc));
        arc
    }

    /// Register a cleanup closure for a scoped service.
    ///
    /// Called by users who want to ensure their scoped services are cleaned up
    /// deterministically when the scope ends. The closure is invoked when
    /// [`Self::dispose_scoped`] is called (via [`Scope::dispose`](crate::scope::Scope::dispose)
    /// or [`Scope`](crate::scope::Scope) Drop).
    pub fn register_disposable(&self, cleanup: Box<dyn FnOnce() + Send>) {
        self.disposables.lock().unwrap().push(cleanup);
    }

    /// Drain and execute all registered cleanup closures.
    ///
    /// Called by [`Scope::dispose`](crate::scope::Scope::dispose).
    pub(crate) fn dispose_scoped(&self) {
        let cleanup: Vec<Box<dyn FnOnce() + Send>> =
            std::mem::take(&mut *self.disposables.lock().unwrap());
        for f in cleanup {
            f();
        }
    }

    /// Validate all dependencies: detect cycles and captive dependencies.
    /// Instances built during validation are stored in `cache` so they can
    /// be reused by the subsequent eager-init phase.
    fn validate_dependencies(store: &ServiceStore, cache: &LazyCache) -> Result<(), RdiError> {
        // Build TypeId → type_name map for error messages
        let type_names: HashMap<TypeId, &'static str> = store
            .iter()
            .map(|(&tid, entries)| {
                let name = entries.first().map(|e| e.type_name).unwrap_or("<unknown>");
                (tid, name)
            })
            .collect();

        // Validate each singleton by attempting to resolve it.
        // Route through `get_any` (instead of calling `entry.factory`
        // directly) so the shared cache is populated and Phase 2 can reuse
        // the instances instead of re-executing the factories.
        for entries in store.values() {
            for entry in entries {
                if entry.lifetime == ServiceLifetime::Singleton
                    && entry.async_factory.is_none()
                {
                    let resolver = ValidationResolver::new(store, &type_names, cache);
                    let type_name = entry.type_name;

                    // `get_any` returns `None` on validation errors (cycle/captive);
                    // the error is stored in `resolver.last_error` and retrieved via
                    // `take_error()`. We still use `catch_unwind` to handle user
                    // factory panics (triggered when `get_any` returns `None` from
                    // a recursive resolution).
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        resolver.get_any(type_name)
                    }));

                    // Check framework validation errors first (cycle/captive).
                    if let Some(err) = resolver.take_error() {
                        return Err(err);
                    }

                    match result {
                        Ok(_) => {}
                        Err(panic_info) => {
                            // User factory panic — wrap in RdiError for consistent
                            // error reporting.
                            let payload = if let Some(s) = panic_info.downcast_ref::<String>() {
                                s.clone()
                            } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                                s.to_string()
                            } else {
                                "unknown".to_string()
                            };
                            return Err(RdiError::FactoryPanic {
                                service: type_name.to_string(),
                                payload,
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Resolve a service by type. Works uniformly for concrete types and trait objects.
    /// Returns `Err(RdiError::ServiceNotFound)` if not registered.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        self.try_get::<T>()
            .ok_or(RdiError::ServiceNotFound(type_name))
    }

    /// Resolve a service by type, returning `None` if not registered.
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.try_get::<T>()
    }

    /// Resolve an async service (transient or scoped) by type.
    ///
    /// Calls the async factory stored in the service entry. Singletons are
    /// resolved synchronously (already cached during `build_async`).
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
            .ok_or(RdiError::ServiceNotFound(type_name))?;

        // Singletons are already cached during build_async — use sync path.
        if entry.lifetime == ServiceLifetime::Singleton {
            return self.get::<T>();
        }

        if let Some(async_factory) = &entry.async_factory {
            let provider_arc = self
                .provider_arc()
                .ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
            let arc_any = async_factory(provider_arc).await;
            Self::extract(arc_any).ok_or(RdiError::ServiceNotFound(type_name))
        } else {
            self.get::<T>()
        }
    }

    /// Resolve an async keyed service by type and key.
    pub async fn get_keyed_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.as_deref() == Some(key)))
            .ok_or(RdiError::KeyedServiceNotFound {
                key: key.to_string(),
                ty: type_name,
            })?;

        // Singletons are already cached during build_async — use sync path.
        if entry.lifetime == ServiceLifetime::Singleton {
            return self.get_keyed::<T>(key);
        }

        if let Some(async_factory) = &entry.async_factory {
            let provider_arc = self
                .provider_arc()
                .ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
            let arc_any = async_factory(provider_arc).await;
            Self::extract(arc_any).ok_or(RdiError::KeyedServiceNotFound {
                key: key.to_string(),
                ty: type_name,
            })
        } else {
            self.get_keyed::<T>(key)
        }
    }

    /// Resolve a keyed service by type and key.
    /// Returns `Err(RdiError::KeyedServiceNotFound)` if not found.
    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        self.try_get_keyed::<T>(key).ok_or(RdiError::KeyedServiceNotFound {
            key: key.to_string(),
            ty: type_name,
        })
    }

    /// Resolve owned `T` (bypass cache, fresh each call).
    /// Returns `Err` if not registered or the service is Singleton.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        self.try_get_owned::<T>()
            .ok_or(RdiError::ServiceNotFound(type_name))
    }

    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        let tid = TypeId::of::<T>();
        let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
        let arc_any = self.create_owned_any_by_entry(entry)?;
        let arc_t = Self::extract(arc_any)?;
        Arc::try_unwrap(arc_t).ok()
    }

    /// Resolve owned keyed `T`.
    /// Returns `Err` if not found or Singleton.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        self.try_get_keyed_owned::<T>(key).ok_or(RdiError::KeyedServiceNotFound {
            key: key.to_string(),
            ty: type_name,
        })
    }

    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(key))?;
        let arc_any = self.create_owned_any_by_entry(entry)?;
        let arc_t = Self::extract(arc_any)?;
        Arc::try_unwrap(arc_t).ok()
    }

    /// Return all registered instances of the given type.
    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        let tid = TypeId::of::<T>();
        match self.store.get(&tid) {
            Some(entries) => entries
                .iter()
                .filter_map(|e| {
                    let arc = self.get_any_by_entry(e)?;
                    Self::extract(arc)
                })
                .collect(),
            None => Vec::new(),
        }
    }

    /// Create a new service scope.
    ///
    /// Analogous to `IServiceProvider.CreateScope()` in MEDI.
    /// Scoped-lifetime services are cached within the returned scope.
    pub fn scope(self: &Arc<Self>) -> crate::scope::Scope {
        crate::scope::Scope::new(self.clone())
    }

    /// Wrap this provider in an `Arc` and enable `provider_arc()` resolution.
    ///
    /// Services using `#[inject(provider)]` require the provider to be
    /// created via this method (not `Arc::new(provider)`) so that the
    /// self-referential weak pointer is initialized.
    pub fn into_arc(self) -> Arc<Self> {
        let arc = Arc::new(self);
        let this: &Self = &arc;
        this.self_weak.get_or_init(|| Arc::downgrade(&arc));
        arc
    }

    /// Alias for `scope()` with MEDI-inspired naming.
    pub fn create_scope(self: &Arc<Self>) -> crate::scope::Scope {
        self.scope()
    }

    /// Alias for `get()` with MEDI-inspired naming (`GetService<T>()`).
    pub fn get_service<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.get_optional::<T>()
    }

    /// Resolve a service by type, panicking if not registered.
    /// MEDI-inspired naming (`GetRequiredService<T>()`).
    pub fn get_required_service<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.get::<T>().unwrap()
    }

    /// Return all registered instances of the given type.
    /// MEDI-inspired naming (`GetServices<T>()`).
    pub fn get_services<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.get_all::<T>()
    }

    fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let tid = TypeId::of::<T>();
        let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
        let arc = self.get_any_by_entry(entry)?;
        Self::extract(arc)
    }

    fn try_get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(key))?;
        let arc = self.get_any_by_entry(entry)?;
        Self::extract(arc)
    }

    pub(crate) fn get_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
        match entry.lifetime {
            ServiceLifetime::Singleton => {
                // LazyCache::get_or_init_with：build 阶段已预热的走 fast path，
                // 未预热的（交叉引用边缘情况）懒执行工厂并缓存。
                // per-key OnceLock 保证多线程下工厂只执行一次，无 TOCTOU 竞态。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                let svc_name = entry.type_name;
                Some(self.singleton_cache.get_or_init_with(entry.cache_key, move || {
                    Self::run_factory_safely(svc_name, move || factory(resolver))
                }))
            }
            ServiceLifetime::Scoped => {
                // Root scope 缓存：ServiceProvider 即 root scope，
                // 从根解析 Scoped 服务在 root_scoped_cache 内复用。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                let svc_name = entry.type_name;
                Some(self.root_scoped_cache.get_or_init_with(entry.cache_key, move || {
                    Self::run_factory_safely(svc_name, move || factory(resolver))
                }))
            }
            ServiceLifetime::Transient => {
                let svc_name = entry.type_name;
                Some(Self::run_factory_safely(svc_name, || (entry.factory)(self)))
            }
        }
    }

    /// Bypass cache and invoke the factory fresh (owned resolution only).
    ///
    /// - Singleton → `None` (shared singleton cannot be owned).
    /// - Scoped/Transient → invoke factory directly without touching
    ///   `singleton_cache` / `root_scoped_cache`. The factory uses `self`
    ///   (ServiceProvider, i.e. root scope) as resolver, so Scoped deps
    ///   resolved transitively bind to the root scope (consistent with
    ///   `get_any_by_entry`'s Scoped root-cache semantics).
    pub(crate) fn create_owned_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
        match entry.lifetime {
            ServiceLifetime::Singleton => None,
            ServiceLifetime::Scoped | ServiceLifetime::Transient => {
                let svc_name = entry.type_name;
                Some(Self::run_factory_safely(svc_name, || {
                    (entry.factory)(self as &dyn IServiceResolver)
                }))
            }
        }
    }

    /// 在 catch_unwind 中执行工厂，捕获 panic 并转换为类型安全的 RdiError。
    ///
    /// - 若 panic payload 是 RdiError（循环依赖/captive），保持原样重新抛出
    ///   （上层 `validate_dependencies` 统一 downcast 处理）。
    /// - 若是其他 panic，包装为 `RdiError::FactoryPanic` 重新抛出（类型安全，
    ///   运行期用户可通过 catch_unwind 识别）。
    ///
    /// 配合 LazyCache 的无锁工厂执行设计（工厂在 RwLock 临界区外执行），
    /// 确保 panic 不会让任何 RwLock 中毒——catch_unwind 捕获 panic 后，
    /// 调用方仍可正常使用 ServiceProvider 后续解析。
    fn run_factory_safely<F>(service_name: &'static str, factory: F) -> AnyService
    where
        F: FnOnce() -> AnyService,
    {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(factory)) {
            Ok(v) => v,
            Err(payload) => {
                // RdiError 直接重新抛出，保留原始错误类型（循环依赖/captive）。
                if payload.is::<RdiError>() {
                    std::panic::panic_any(payload);
                }
                let msg = payload
                    .downcast_ref::<&'static str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().map(|s| s.clone()))
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                std::panic::panic_any(RdiError::FactoryPanic {
                    service: service_name.to_string(),
                    payload: msg,
                });
            }
        }
    }

    /// Extract `Arc<T>` from `Arc<Arc<T>>` stored inside `Arc<dyn Any>`.
    /// The factory double-wraps: inner `Arc<T>`, outer `Arc<dyn Any>`.
    pub(crate) fn extract<T: ?Sized + Send + Sync + 'static>(
        arc: Arc<dyn Any + Send + Sync>,
    ) -> Option<Arc<T>> {
        let double: Arc<Arc<T>> = arc.downcast::<Arc<T>>().ok()?;
        Some(Arc::clone(&*double))
    }

    /// Find entry by string type_name + variant (for string-based resolution).
    pub(crate) fn entry_by_str(&self, type_name: &str, variant: &str) -> Option<&ServiceEntry> {
        let tid = self.type_map.get(type_name)?;
        self.store
            .get(tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(variant))
    }

    /// Cross-DLL safe named service resolution (generic).
    pub fn get_named<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.named
            .read()
            .unwrap()
            .get(name)?
            .clone()
            .downcast::<T>()
            .ok()
    }

    /// Non-generic named resolution; returns `Arc<dyn Any>` for trait-object dispatch.
    pub fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.named.read().unwrap().get(name).cloned()
    }

    /// Register a named service for cross-DLL plugin access.
    pub fn register_named<T: Send + Sync + 'static>(&self, name: &str, service: Arc<T>) {
        self.named
            .write()
            .unwrap()
            .insert(name.to_string(), service);
    }

    /// Remove a named service (for plugin unload).
    pub fn remove_named(&self, name: &str) {
        self.named.write().unwrap().remove(name);
    }

    /// Register a named service (for `impl_service_locator!` macro).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.named
            .write()
            .unwrap()
            .insert(name.to_string(), service);
    }

    /// Remove a named service (for `impl_service_locator!` macro).
    pub fn rdi_remove_named(&self, name: &str) {
        self.named.write().unwrap().remove(name);
    }
}

impl IServiceResolver for ServiceProvider {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_map.get(key)?;
        let entry = self.store.get(tid)?.iter().find(|e| e.key.is_none())?;
        self.get_any_by_entry(entry)
    }
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, variant)?;
        self.get_any_by_entry(entry)
    }
    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        // 复用 type_map → store 路径，遍历某 TypeId 下所有 entry（default + keyed），
        // 逐个 get_any_by_entry 收集。空注册返回空 Vec，不 panic。
        let tid = match self.type_map.get(key) {
            Some(t) => t,
            None => return Vec::new(),
        };
        match self.store.get(tid) {
            Some(entries) => entries
                .iter()
                .filter_map(|e| self.get_any_by_entry(e))
                .collect(),
            None => Vec::new(),
        }
    }
    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_map.get(key)?;
        let entry = self.store.get(tid)?.iter().find(|e| e.key.is_none())?;
        self.create_owned_any_by_entry(entry)
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, variant)?;
        self.create_owned_any_by_entry(entry)
    }
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        self.self_weak.get().and_then(|w| w.upgrade())
    }
}

#[cfg(test)]
mod tests {
    use crate::collection::ServiceCollection;
    use std::sync::Arc;

    #[derive(Debug, PartialEq)]
    struct Calc(i32);
    #[test]
    fn optional_missing() {
        let p = ServiceCollection::new().build().unwrap();
        assert!(p.get_optional::<i32>().is_none());
    }
    #[test]
    fn all_basic() {
        let p = ServiceCollection::new()
            .keyed_singleton("a", |_| Arc::new(Calc(1)))
            .keyed_singleton("b", |_| Arc::new(Calc(2)))
            .build()
            .unwrap();
        assert_eq!(p.get_all::<Calc>().len(), 2);
    }
}
