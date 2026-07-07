use crate::cache::{AnyService, LazyCache};
use crate::entry::{
    AsyncServiceFactory, IServiceResolver, ServiceDescriptor, ServiceEntry, ServiceFactory,
    ServiceLifetime, ServiceStore,
};
use crate::error::RdiError;
use crate::scope::ScopeFactory;
use crate::validation::ValidationResolver;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

pub struct ServiceProvider {
    /// Service registry. Shared across root + scope-derived providers via `Arc`.
    store: Arc<ServiceStore>,
    /// String → TypeId lookup for IServiceResolver string-based resolution.
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
    /// 子 Scope（通过 [`ServiceProvider::scope`] 创建）拥有独立的 scoped_cache，不回退到此缓存。
    /// per-key `OnceLock` 保证多线程下工厂只执行一次，无 TOCTOU 竞态。
    /// **每个 provider 独立**：root 与 scope-derived 各自持有一份。
    root_scoped_cache: LazyCache,
    /// String-keyed registry for cross-DLL (cdylib) service access.
    /// Rust's `TypeId` differs across compilation units, so named
    /// lookup is the only reliable mechanism for plugin services.
    /// Shared across root + scope-derived providers via `Arc`.
    pub(crate) named: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    /// Self-referential weak pointer, initialized by [`Self::new`] or
    /// [`Self::new_scope`]. Enables [`IServiceResolver::provider_arc`] to hand
    /// out `Arc<ServiceProvider>` to services using `#[inject(provider)]`.
    /// **每个 provider 独立**：scope-derived provider 的 self_weak 指向自身。
    self_weak: OnceLock<Weak<ServiceProvider>>,
    /// Cleanup closures for scoped services registered via [`Self::register_disposable`].
    /// Populated during factory execution, drained by `Self::dispose_scoped`.
    /// **每个 provider 独立**：scope-derived provider 的 disposables 绑定到当前 scope。
    disposables: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl ServiceProvider {
    pub(crate) fn new(store: ServiceStore) -> Result<Arc<Self>, RdiError> {
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

        // Phase 2: create the provider in an Arc and initialize self_weak.
        // `build()` returns `Arc<ServiceProvider>` so `provider_arc()` works
        // out of the box — no `into_arc()` step needed.
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

        // Phase 3: eagerly execute sync singleton factories.
        // Reuse instances already built during validation; only execute
        // factories whose results are not yet cached.
        for (ck, factory) in &singleton_entries {
            // LazyCache::get_or_init_with 内部查缓存，validation 已构建的跳过。
            let resolver = sp_arc.as_ref() as &dyn IServiceResolver;
            let factory = factory.clone();
            sp_arc
                .singleton_cache
                .get_or_init_with(*ck, move || factory(resolver));
        }

        Ok(sp_arc)
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
    /// 与 [`Self::new`] 一样初始化 self_weak，使 scope provider 自身可被
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
    /// `Self::dispose_scoped` is called (via [`Scope::dispose`](crate::scope::Scope::dispose)
    /// or [`Scope`](crate::scope::Scope) Drop).
    pub fn register_disposable(&self, cleanup: Box<dyn FnOnce() + Send>) {
        self.disposables.lock().unwrap().push(cleanup);
    }

    /// Drain and execute all registered cleanup closures.
    ///
    /// Called by [`Scope::dispose`](crate::scope::Scope::dispose). Uses `try_lock`
    /// to avoid deadlock when a concurrent resolution is in progress (factory
    /// holding the lock via [`Self::register_disposable`]). If the lock is
    /// contended, disposal is skipped — `Drop` is a safety net, not a guarantee.
    pub(crate) fn dispose_scoped(&self) {
        // try_lock 避免与并发 register_disposable 死锁。
        // 锁不上时跳过——并发解析仍在进行，disposable 留待下次 dispose 或被丢弃。
        let to_run: Vec<Box<dyn FnOnce() + Send>> = match self.disposables.try_lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => return,
        };
        // 释放锁后再执行闭包（避免持锁调用用户代码，防止重入死锁）
        for cleanup in to_run {
            cleanup();
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
            .ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })
    }

    /// Resolve a service by type, returning `None` if not registered.
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.try_get::<T>()
    }

    /// Resolve an async service (transient or scoped) by type.
    ///
    /// - Singletons are resolved synchronously (already cached during `build_async`).
    /// - Scoped services are cached in `root_scoped_cache` (one instance per scope).
    /// - Transient services call the async factory fresh each time.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
            .ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })?;

        match entry.lifetime {
            ServiceLifetime::Singleton => self.get::<T>(),
            ServiceLifetime::Scoped => {
                let async_factory = entry.async_factory.clone().ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })?;
                let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
                let arc_any = self.root_scoped_cache.get_or_init_with_async(entry.cache_key, || {
                    Self::run_async_factory_safely(type_name, async_factory, provider_arc)
                }).await?;
                Self::extract(arc_any).ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })
            }
            ServiceLifetime::Transient => {
                if let Some(async_factory) = entry.async_factory.clone() {
                    let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                        service: type_name.to_string(),
                        payload: "provider_arc() returned None for async resolution".into(),
                    })?;
                    let arc_any = Self::run_async_factory_safely(type_name, async_factory, provider_arc).await?;
                    Self::extract(arc_any).ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })
                } else {
                    self.get::<T>()
                }
            }
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

        match entry.lifetime {
            ServiceLifetime::Singleton => self.get_keyed::<T>(key),
            ServiceLifetime::Scoped => {
                let async_factory = entry.async_factory.clone().ok_or(RdiError::KeyedServiceNotFound {
                    key: key.to_string(),
                    ty: type_name,
                })?;
                let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
                let arc_any = self.root_scoped_cache.get_or_init_with_async(entry.cache_key, || {
                    Self::run_async_factory_safely(type_name, async_factory, provider_arc)
                }).await?;
                Self::extract(arc_any).ok_or(RdiError::KeyedServiceNotFound {
                    key: key.to_string(),
                    ty: type_name,
                })
            }
            ServiceLifetime::Transient => {
                if let Some(async_factory) = entry.async_factory.clone() {
                    let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                        service: type_name.to_string(),
                        payload: "provider_arc() returned None for async resolution".into(),
                    })?;
                    let arc_any = Self::run_async_factory_safely(type_name, async_factory, provider_arc).await?;
                    Self::extract(arc_any).ok_or(RdiError::KeyedServiceNotFound {
                        key: key.to_string(),
                        ty: type_name,
                    })
                } else {
                    self.get_keyed::<T>(key)
                }
            }
        }
    }

    /// 执行 async 工厂并捕获 panic，转为 `RdiError::FactoryPanic`。
    ///
    /// async 工厂 panic 无法自然传播到 `.await` 点之外（会终止 task），
    /// 需用 `catch_unwind` 捕获并转为类型安全的 `RdiError`。
    async fn run_async_factory_safely(
        type_name: &'static str,
        async_factory: AsyncServiceFactory,
        provider_arc: Arc<ServiceProvider>,
    ) -> Result<AnyService, RdiError> {
        let fut = async_factory(provider_arc);
        match futures::FutureExt::catch_unwind(AssertUnwindSafe(fut)).await {
            Ok(arc_any) => Ok(arc_any),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&'static str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                Err(RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: msg,
                })
            }
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
            .ok_or(RdiError::ServiceNotFound { ty: type_name, context: None })
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
                // 工厂 panic 时 OnceLock 不缓存（std 行为），下次解析重试。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                Some(self.singleton_cache.get_or_init_with(entry.cache_key, move || {
                    factory(resolver)
                }))
            }
            ServiceLifetime::Scoped => {
                // Root scope 缓存：ServiceProvider 即 root scope，
                // 从根解析 Scoped 服务在 root_scoped_cache 内复用。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                Some(self.root_scoped_cache.get_or_init_with(entry.cache_key, move || {
                    factory(resolver)
                }))
            }
            ServiceLifetime::Transient => {
                Some((entry.factory)(self))
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
                Some((entry.factory)(self as &dyn IServiceResolver))
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

    /// Register a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.named
            .write()
            .unwrap()
            .insert(name.to_string(), service);
    }

    /// Remove a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_remove_named(&self, name: &str) {
        self.named.write().unwrap().remove(name);
    }
}

impl ScopeFactory for ServiceProvider {
    fn create_scope(&self) -> crate::scope::Scope {
        // 通过 self_weak 获取 Arc<Self>（new() / new_async() 已初始化 self_weak）。
        self.self_weak
            .get()
            .and_then(|w| w.upgrade())
            .map(crate::scope::Scope::new)
            .expect("ScopeFactory::create_scope called before self_weak initialization")
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
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.store.get(&tid)?.iter().find(|e| e.key.is_none())?;
        self.get_any_by_entry(entry)
    }
    fn get_keyed_by_type_id(
        &self,
        tid: TypeId,
        key: &str,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self
            .store
            .get(&tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(key))?;
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
