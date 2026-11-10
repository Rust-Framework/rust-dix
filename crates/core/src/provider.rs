use crate::cache::{AnyService, LazyCache};
use crate::entry::{
    AsyncServiceFactory, IServiceResolver, ServiceEntry, ServiceFactory, ServiceLifetime,
    ServiceStore,
};
use crate::error::RdiError;
use crate::scope::ScopeFactory;
use crate::validation::ValidationResolver;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

thread_local! {
    /// Runtime resolution stack for Scoped/Transient cycle detection.
    static RESOLVING: RefCell<Vec<(usize, &'static str)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct ResolveGuard;

impl Drop for ResolveGuard {
    fn drop(&mut self) {
        RESOLVING.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

pub(crate) fn enter_resolve(
    cache_key: usize,
    type_name: &'static str,
) -> Result<ResolveGuard, RdiError> {
    RESOLVING.with(|stack| {
        let mut s = stack.borrow_mut();
        if s.iter().any(|(k, _)| *k == cache_key) {
            let mut chain: Vec<&str> = s.iter().map(|(_, n)| *n).collect();
            chain.push(type_name);
            return Err(RdiError::CircularDependency(chain.join(" -> ")));
        }
        s.push((cache_key, type_name));
        Ok(ResolveGuard)
    })
}

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
        // Reject async registrations on the sync build path.
        if let Some(entry) = store
            .values()
            .flat_map(|entries| entries.iter())
            .find(|e| e.async_factory.is_some())
        {
            return Err(RdiError::AsyncServiceRequiresBuildAsync {
                service: entry.type_name.to_string(),
            });
        }

        // Build type_name → TypeId lookup table for string-based resolution
        let mut type_map = HashMap::new();
        for (&tid, entries) in &store {
            if let Some(e) = entries.first() {
                type_map.entry(e.type_name).or_insert(tid);
            }
        }

        let singleton_cache = Arc::new(LazyCache::new());

        // Phase 1: create the provider in an `Arc` **before** dependency
        // validation. Singleton factories may legitimately depend on the
        // container itself (`#[inject(provider)]`, `#[inject] Arc<dyn
        // ScopeFactory>`), so `provider_arc()` and the built-in ScopeFactory
        // must be resolvable while validation executes those factories.
        let sp_arc = Arc::new(Self {
            store: Arc::new(store),
            type_map: Arc::new(type_map),
            singleton_cache,
            root_scoped_cache: LazyCache::new(),
            named: Arc::new(RwLock::new(HashMap::new())),
            self_weak: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
        });
        sp_arc.self_weak.get_or_init(|| Arc::downgrade(&sp_arc));

        // Phase 2: Validate dependencies (cycle detection, captive dependency)
        // and cache the singleton instances built along the way.
        Self::validate_dependencies(&sp_arc)?;

        // Phase 3: eagerly execute any sync singleton whose factory was not
        // already run by validation.
        Self::init_sync_singletons(&sp_arc);
        Ok(sp_arc)
    }

    /// Collect sync singleton entries and eagerly execute their factories.
    ///
    /// Reuses instances cached by validation (build time) so factories aren't
    /// double-fired. Called by both `new` and `new_async` (async singletons skipped).
    fn init_sync_singletons(sp_arc: &Arc<Self>) {
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
    }

    /// Build a `ServiceProvider` with async factory support.
    ///
    /// Async singleton factories are executed during build and their results
    /// are cached. Async transient/scoped factories are stored for later
    /// resolution via `get_async` / `get_keyed_async`.
    ///
    /// Returns `Arc<ServiceProvider>` so that `provider_arc()` works for
    /// async resolution (async factories receive `Arc<ServiceProvider>`).
    pub(crate) async fn new_async(store: ServiceStore) -> Result<Arc<Self>, RdiError> {
        // Build type_name → TypeId lookup
        let mut type_map = HashMap::new();
        for (&tid, entries) in &store {
            if let Some(e) = entries.first() {
                type_map.entry(e.type_name).or_insert(tid);
            }
        }

        let singleton_cache = Arc::new(LazyCache::new());

        // Phase 1: create the provider in an Arc **before** validation so that
        // singleton factories depending on `ServiceProvider` / `ScopeFactory`
        // can be constructed eagerly (same contract as the sync `build()`).
        let sp_arc = Arc::new(Self {
            store: Arc::new(store),
            type_map: Arc::new(type_map),
            singleton_cache,
            root_scoped_cache: LazyCache::new(),
            named: Arc::new(RwLock::new(HashMap::new())),
            self_weak: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
        });
        sp_arc.self_weak.get_or_init(|| Arc::downgrade(&sp_arc));

        // Phase 2: validate sync singleton dependencies and cache instances.
        Self::validate_dependencies(&sp_arc)?;

        // Phase 3: eagerly execute sync singletons not already built.
        Self::init_sync_singletons(&sp_arc);

        // Phase 4: execute async singleton factories.
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
    /// Called by [`Scope::dispose`](crate::scope::Scope::dispose). Acquires the
    /// disposables lock (waits if a concurrent `register_disposable` is in
    /// progress), takes the queue, then runs cleanups **without** holding the
    /// lock — so cleanup callbacks may safely call `register_disposable` again.
    pub(crate) fn dispose_scoped(&self) {
        let to_run: Vec<Box<dyn FnOnce() + Send>> = {
            let mut guard = self.disposables.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        for cleanup in to_run {
            cleanup();
        }
    }

    /// Validate all dependencies: detect cycles and captive dependencies.
    /// Instances built during validation are stored in the provider's singleton
    /// cache so they can be reused by the subsequent eager-init phase.
    ///
    /// Validation runs **after** the provider `Arc` exists: factories may inject
    /// the provider itself (`#[inject(provider)]`) or the built-in
    /// [`ScopeFactory`], which requires `provider_arc()` and `self_weak` to be
    /// live. The validation resolver still executes factories itself, so
    /// cycle/captive-dependency accounting is preserved.
    fn validate_dependencies(sp: &Arc<Self>) -> Result<(), RdiError> {
        // Build TypeId → type_name map for error messages
        let type_names: HashMap<TypeId, &'static str> = sp
            .store
            .iter()
            .map(|(&tid, entries)| {
                let name = entries.first().map(|e| e.type_name).unwrap_or("<unknown>");
                (tid, name)
            })
            .collect();

        // Validate each sync singleton by attempting to resolve it.
        // Scoped/Transient are not eagerly executed at build (avoids side effects);
        // their cycles are detected at runtime via a thread-local resolution stack.
        for entries in sp.store.values() {
            for entry in entries {
                if entry.lifetime == ServiceLifetime::Singleton && entry.async_factory.is_none() {
                    let resolver = ValidationResolver::new(
                        &sp.store,
                        &type_names,
                        &sp.singleton_cache,
                        Arc::clone(sp),
                    );
                    let type_name = entry.type_name;
                    let key = entry.key.clone();

                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &key {
                            Some(k) => resolver.get_keyed_any(type_name, k),
                            None => resolver.get_any(type_name),
                        }));

                    if let Some(err) = resolver.take_error() {
                        return Err(err);
                    }

                    match result {
                        Ok(_) => {}
                        Err(panic_info) => {
                            return Err(RdiError::from_panic_payload(type_name, panic_info));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Resolve a service by type. Works uniformly for concrete types and trait objects.
    /// Returns `Err(RdiError::ServiceNotFound)` if not registered.
    ///
    /// [`ScopeFactory`] is always available from a built provider (no manual
    /// registration required). An explicit `singleton::<dyn ScopeFactory>(...)`
    /// registration still takes precedence.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        match self.resolve_optional::<T>() {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(RdiError::not_found(type_name, None)),
            Err(e) => Err(e),
        }
    }

    /// Resolve a service by type, returning `None` if not registered.
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.resolve_optional::<T>().ok().flatten()
    }

    /// Resolve an async service (transient or scoped) by type.
    ///
    /// - Singletons are resolved synchronously (already cached during `build_async`).
    /// - Scoped services are cached in `root_scoped_cache` (one instance per scope).
    /// - Transient services call the async factory fresh each time.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
            .ok_or_else(|| RdiError::not_found(type_name, None))?;

        if entry.lifetime == ServiceLifetime::Singleton {
            return self.get::<T>();
        }

        match self.resolve_async_any(entry, type_name).await? {
            Some(arc_any) => {
                Self::extract(arc_any).ok_or_else(|| RdiError::not_found(type_name, None))
            }
            None => self.get::<T>(),
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
            .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None))?;

        if entry.lifetime == ServiceLifetime::Singleton {
            return self.get_keyed::<T>(key);
        }

        match self.resolve_async_any(entry, type_name).await? {
            Some(arc_any) => Self::extract(arc_any)
                .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None)),
            None => self.get_keyed::<T>(key),
        }
    }

    /// Resolve async Scoped/Transient factory, returning `Some(AnyService)`.
    /// Returns `None` if the entry has no async factory (caller falls back to sync path).
    async fn resolve_async_any(
        &self,
        entry: &ServiceEntry,
        type_name: &'static str,
    ) -> Result<Option<AnyService>, RdiError> {
        match entry.lifetime {
            ServiceLifetime::Scoped => {
                let async_factory = entry
                    .async_factory
                    .clone()
                    .ok_or_else(|| RdiError::not_found(type_name, None))?;
                let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
                self.root_scoped_cache
                    .get_or_init_with_async(entry.cache_key, || {
                        Self::run_async_factory_safely(type_name, async_factory, provider_arc)
                    })
                    .await
                    .map(Some)
            }
            ServiceLifetime::Transient => {
                if let Some(async_factory) = entry.async_factory.clone() {
                    let provider_arc =
                        self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                            service: type_name.to_string(),
                            payload: "provider_arc() returned None for async resolution".into(),
                        })?;
                    Self::run_async_factory_safely(type_name, async_factory, provider_arc)
                        .await
                        .map(Some)
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Execute an async factory and catch panics, converting to `RdiError::FactoryPanic`.
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
            Err(payload) => Err(RdiError::from_panic_payload(type_name, payload)),
        }
    }

    /// Resolve a keyed service by type and key.
    /// Returns `Err(RdiError::KeyedServiceNotFound)` if not found.
    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        match self.resolve_keyed_optional::<T>(key) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(RdiError::keyed_not_found(type_name, key, None)),
            Err(e) => Err(e),
        }
    }

    /// Resolve owned `T` (bypass cache, fresh each call).
    /// Returns `Err` if not registered or the service is Singleton.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
            .ok_or_else(|| RdiError::not_found(type_name, None))?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return Err(RdiError::OwnedNotSupported(type_name));
        }
        let arc_any = match std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.create_owned_any_by_entry(entry)
        })) {
            Ok(Some(v)) => v,
            Ok(None) => return Err(RdiError::not_found(type_name, None)),
            Err(payload) => return Err(RdiError::from_panic_payload(type_name, payload)),
        };
        let arc_t = Self::extract(arc_any).ok_or_else(|| RdiError::not_found(type_name, None))?;
        Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(type_name))
    }

    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        self.get_owned().ok()
    }

    /// Resolve owned keyed `T`.
    /// Returns `Err` if not found or Singleton.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.as_deref() == Some(key)))
            .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None))?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return Err(RdiError::OwnedNotSupported(type_name));
        }
        let arc_any = match std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.create_owned_any_by_entry(entry)
        })) {
            Ok(Some(v)) => v,
            Ok(None) => return Err(RdiError::keyed_not_found(type_name, key, None)),
            Err(payload) => return Err(RdiError::from_panic_payload(type_name, payload)),
        };
        let arc_t = Self::extract(arc_any)
            .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None))?;
        Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(type_name))
    }

    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        self.get_keyed_owned(key).ok()
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

    #[allow(dead_code)]
    fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.resolve_optional::<T>().ok().flatten()
    }

    #[allow(dead_code)]
    fn try_get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        self.resolve_keyed_optional::<T>(key).ok().flatten()
    }

    /// Resolve container-provided (built-in) services that need no registration.
    ///
    /// - `dyn ScopeFactory`: weak-backed factory that creates independent scopes.
    /// - `ServiceProvider`: the container itself (MEDI `IServiceProvider` parity),
    ///   enabling `#[inject] Arc<ServiceProvider>` in addition to the dedicated
    ///   `#[inject(provider)]` field attribute.
    fn builtin_service_by_type_id(&self, tid: TypeId) -> Option<AnyService> {
        if tid == TypeId::of::<dyn ScopeFactory>() {
            return Some(crate::scope::builtin_scope_factory_any(
                self.self_weak.get()?.clone(),
            ));
        }
        if tid == TypeId::of::<ServiceProvider>() {
            let provider = self.self_weak.get()?.upgrade()?;
            return Some(Arc::new(provider) as AnyService);
        }
        None
    }

    /// String-keyed variant of [`Self::builtin_service_by_type_id`] for the
    /// `IServiceResolver::get_any` path used by the inject macros.
    fn builtin_service_by_name(&self, key: &str) -> Option<AnyService> {
        if key == std::any::type_name::<dyn ScopeFactory>() {
            return Some(crate::scope::builtin_scope_factory_any(
                self.self_weak.get()?.clone(),
            ));
        }
        if key == std::any::type_name::<ServiceProvider>() {
            let provider = self.self_weak.get()?.upgrade()?;
            return Some(Arc::new(provider) as AnyService);
        }
        None
    }

    /// Resolve `T`, mapping inject/`panic_any(RdiError)` factory failures to [`RdiError`].
    fn resolve_optional<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Option<Arc<T>>, RdiError> {
        let tid = TypeId::of::<T>();
        if let Some(entry) = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
        {
            let arc = self.resolve_entry_catching(entry)?;
            return Ok(Self::extract(arc));
        }
        // Built-in container services (ScopeFactory / ServiceProvider) are
        // always available from a built provider; explicit registrations win
        // because the store lookup above runs first.
        Ok(self.builtin_service_by_type_id(tid).and_then(Self::extract))
    }

    fn resolve_keyed_optional<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<Arc<T>>, RdiError> {
        let tid = TypeId::of::<T>();
        let Some(entry) = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.as_deref() == Some(key)))
        else {
            return Ok(None);
        };
        let arc = self.resolve_entry_catching(entry)?;
        Ok(Self::extract(arc))
    }

    pub(crate) fn resolve_entry_catching(
        &self,
        entry: &ServiceEntry,
    ) -> Result<AnyService, RdiError> {
        match std::panic::catch_unwind(AssertUnwindSafe(|| self.get_any_by_entry(entry))) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(RdiError::not_found(entry.type_name, None)),
            Err(payload) => Err(RdiError::from_panic_payload(entry.type_name, payload)),
        }
    }

    pub(crate) fn get_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
        let _guard = match enter_resolve(entry.cache_key, entry.type_name) {
            Ok(g) => g,
            Err(e) => std::panic::panic_any(e),
        };
        match entry.lifetime {
            ServiceLifetime::Singleton => {
                // LazyCache::get_or_init_with：build 阶段已预热的走 fast path，
                // 未预热的（交叉引用边缘情况）懒执行工厂并缓存。
                // per-key OnceLock 保证多线程下工厂只执行一次，无 TOCTOU 竞态。
                // 工厂 panic 时 OnceLock 不缓存（std 行为），下次解析重试。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                Some(
                    self.singleton_cache
                        .get_or_init_with(entry.cache_key, move || factory(resolver)),
                )
            }
            ServiceLifetime::Scoped => {
                // Root scope 缓存：ServiceProvider 即 root scope，
                // 从根解析 Scoped 服务在 root_scoped_cache 内复用。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                Some(
                    self.root_scoped_cache
                        .get_or_init_with(entry.cache_key, move || factory(resolver)),
                )
            }
            ServiceLifetime::Transient => Some((entry.factory)(self)),
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
                let _guard = match enter_resolve(entry.cache_key, entry.type_name) {
                    Ok(g) => g,
                    Err(e) => std::panic::panic_any(e),
                };
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
            .expect("ScopeFactory::create_scope: ServiceProvider self_weak not initialized")
    }
}

impl IServiceResolver for ServiceProvider {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(tid) = self.type_map.get(key) {
            if let Some(entry) = self
                .store
                .get(tid)
                .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
            {
                return self.get_any_by_entry(entry);
            }
        }
        // Built-in services resolve through the string path used by the inject
        // macros (`#[inject] Arc<dyn ScopeFactory>`, `Arc<ServiceProvider>`).
        self.builtin_service_by_name(key)
    }
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, variant)?;
        self.get_any_by_entry(entry)
    }
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(entry) = self
            .store
            .get(&tid)
            .and_then(|entries| entries.iter().find(|e| e.key.is_none()))
        {
            return self.get_any_by_entry(entry);
        }
        self.builtin_service_by_type_id(tid)
    }
    fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
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
