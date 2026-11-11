use crate::cache::{AnyService, LazyCache};
use crate::entry::{
    AsyncServiceFactory, IServiceResolver, ServiceEntry, ServiceFactory, ServiceLifetime,
    ServiceStore,
};
use crate::error::RdiError;
use crate::scope::ScopeFactory;
use crate::validation::ValidationResolver;
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

thread_local! {
    /// Runtime resolution stack for Scoped/Transient cycle detection.
    static RESOLVING: RefCell<Vec<(usize, &'static str)>> = const { RefCell::new(Vec::new()) };
    /// Non-zero while a build-time singleton factory is executing.
    ///
    /// Resolving services through an injected provider inside a constructor
    /// would bypass build-time cycle/captive validation *and* make the outcome
    /// depend on store iteration order, so it is rejected deterministically.
    static BUILD_FACTORY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct ResolveGuard;

impl Drop for ResolveGuard {
    fn drop(&mut self) {
        RESOLVING.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// RAII marker for “a build-time singleton factory is currently executing”.
pub(crate) struct BuildFactoryGuard;

impl BuildFactoryGuard {
    pub(crate) fn enter() -> Self {
        BUILD_FACTORY_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for BuildFactoryGuard {
    fn drop(&mut self) {
        BUILD_FACTORY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Reject provider-mediated resolution while a constructor is running.
///
/// Construction should declare dependencies as fields; dynamic resolution must
/// happen later in methods, where no build-time validation constraint applies.
fn reject_build_time_provider_resolution(type_name: &'static str) {
    if BUILD_FACTORY_DEPTH.with(|depth| depth.get()) > 0 {
        std::panic::panic_any(RdiError::FactoryPanic {
            service: type_name.to_string(),
            payload: format!(
                "resolving services through the injected provider from a build-time constructor \
                 is not supported: store the provider/ScopeFactory and resolve lazily in methods \
                 (attempted to resolve {type_name})"
            ),
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

/// Check the current (synchronous) resolution chain without pushing a frame.
///
/// Used by async factories: a nested `await` of a service that is already being
/// resolved would dead-lock on its own per-key async lock, so it is reported as
/// [`RdiError::CircularDependency`] instead.
fn check_not_resolving(cache_key: usize, type_name: &'static str) -> Result<(), RdiError> {
    RESOLVING.with(|stack| {
        let s = stack.borrow();
        if s.iter().any(|(k, _)| *k == cache_key) {
            let mut chain: Vec<&str> = s.iter().map(|(_, n)| *n).collect();
            chain.push(type_name);
            return Err(RdiError::CircularDependency(chain.join(" -> ")));
        }
        Ok(())
    })
}

struct AsyncPollGuard {
    cache_key: usize,
    type_name: &'static str,
}

impl Drop for AsyncPollGuard {
    fn drop(&mut self) {
        let _ = self.cache_key;
        let _ = self.type_name;
        RESOLVING.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Future wrapper that marks a resolution as "in chain" **for the duration of a
/// single poll**.
///
/// This is what makes async re-entrancy detection precise: the marker is only
/// visible to nested resolutions started synchronously inside the same poll
/// chain (which is where the self-await dead-lock would occur). Concurrent
/// tasks polled on the same thread interleave between polls, so they never see
/// each other's markers and cannot produce false `CircularDependency` errors.
struct AsyncPollGuardFuture<F> {
    inner: std::pin::Pin<Box<F>>,
    cache_key: usize,
    type_name: &'static str,
}

impl<F: std::future::Future> std::future::Future for AsyncPollGuardFuture<F> {
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // `Self` is `Unpin` (it only holds a `Pin<Box<F>>` and `Copy` fields).
        let this = self.get_mut();
        RESOLVING.with(|stack| stack.borrow_mut().push((this.cache_key, this.type_name)));
        let _guard = AsyncPollGuard {
            cache_key: this.cache_key,
            type_name: this.type_name,
        };
        this.inner.as_mut().poll(cx)
    }
}

fn poll_guard<F: std::future::Future>(
    cache_key: usize,
    type_name: &'static str,
    future: F,
) -> AsyncPollGuardFuture<F> {
    AsyncPollGuardFuture {
        inner: Box::pin(future),
        cache_key,
        type_name,
    }
}

/// Drop cached instances, collecting (instead of propagating) `Drop` panics.
///
/// Disposal must finish releasing the remaining instances even when a user
/// `Drop` impl panics; the first panic is re-raised by the caller after all
/// cleanup work completed (and swallowed during unwinding).
fn drop_instances_safely(instances: Vec<AnyService>) -> Option<Box<dyn Any + Send>> {
    let mut first_panic: Option<Box<dyn Any + Send>> = None;
    for instance in instances {
        if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(|| drop(instance))) {
            if first_panic.is_none() {
                first_panic = Some(payload);
            }
        }
    }
    first_panic
}

/// Options for building a [`ServiceProvider`].
///
/// `#[non_exhaustive]`: construct via [`ServiceProviderOptions::new`] and the
/// `with_*` builder methods (`Default` equals `new()`), so future options stay
/// additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceProviderOptions {
    /// Reject resolving `Scoped` services from the root provider.
    ///
    /// Mirrors MEDI `ValidateScopes` as enabled by the .NET (ASP.NET Core) host
    /// in the Development environment: resolving a scoped service outside a
    /// scope is a misconfiguration, because the root caches it for the container
    /// lifetime (the "scoped becomes singleton" anti-pattern).
    ///
    /// - `true` (default): `get`/`get_async` (and constructor dependencies)
    ///   return [`RdiError::ScopedFromRootProvider`] when they would resolve a
    ///   `Scoped` service from the root provider. Infallible lookups
    ///   (`get_optional`, `get_all`, `get_all_keyed`, `try_*`) treat such a
    ///   service as not resolvable at the root. `get_owned` is exempt for the
    ///   owned instance itself (it is caller-owned and never enters a scope
    ///   cache); its transitive dependencies are still validated.
    /// - `false`: restores rust-dix ≤ 0.7 root-scope behavior (the root provider
    ///   caches scoped instances and acts as the root scope).
    pub validate_scopes: bool,
}

impl ServiceProviderOptions {
    /// Strict defaults: `validate_scopes = true`.
    pub fn new() -> Self {
        Self {
            validate_scopes: true,
        }
    }

    /// Set scope validation explicitly.
    pub fn with_validate_scopes(mut self, validate_scopes: bool) -> Self {
        self.validate_scopes = validate_scopes;
        self
    }

    /// Convenience preset matching MEDI's bare `BuildServiceProvider()`:
    /// scope validation disabled (root-scope resolution allowed).
    pub fn lenient() -> Self {
        Self {
            validate_scopes: false,
        }
    }
}

impl Default for ServiceProviderOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// State shared between the root provider and every scope created from it.
///
/// Disposing the root marks the whole tree disposed; scopes created earlier can
/// no longer resolve services.
pub(crate) struct SharedProviderState {
    disposed: AtomicBool,
}

impl SharedProviderState {
    fn new() -> Self {
        Self {
            disposed: AtomicBool::new(false),
        }
    }
}

pub struct ServiceProvider {
    /// Service registry. Shared across root + scope-derived providers via `Arc`.
    store: Arc<ServiceStore>,
    /// String → TypeId lookup for IServiceResolver string-based resolution.
    /// Shared across root + scope-derived providers via `Arc`.
    type_map: Arc<HashMap<&'static str, TypeId>>,
    /// Singleton 实例缓存（per-key `OnceLock`，多线程下工厂只执行一次）。
    /// build 阶段 validation + eager init 预热，运行时 lazy fallback。
    /// Shared across root + scope-derived providers via `Arc`（全局单例语义）。
    /// 记录首次创建顺序，供 [`Self::dispose`] 逆序释放。
    singleton_cache: Arc<LazyCache>,
    /// 当前 provider 的 Scoped 实例缓存。
    ///
    /// Root provider 即 root scope：`validate_scopes = true`（默认）时 root 不允许
    /// 解析 Scoped 服务；`validate_scopes = false` 时实例在此缓存中复用，语义等同
    /// 于在根 scope 内单例化。子 Scope（通过 [`ServiceProvider::scope`] 创建）拥有
    /// 独立的 scoped 缓存，不回退到根。per-key `OnceLock` 保证多线程下工厂只执行
    /// 一次，无 TOCTOU 竞态。**每个 provider 独立**。
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
    /// Built-in `dyn ScopeFactory` singleton, created lazily on first resolution.
    /// It holds a `Weak` provider reference, so caching it cannot create an
    /// `Arc` cycle (MEDI parity: the scope factory is itself a singleton).
    scope_factory_cache: OnceLock<Arc<dyn ScopeFactory>>,
    /// Cleanup closures for scoped services registered via [`Self::register_disposable`].
    /// Populated during factory execution, drained (reverse registration order) by
    /// `Self::dispose`.
    /// **每个 provider 独立**：scope-derived provider 的 disposables 绑定到当前 scope。
    disposables: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    /// Root/scope shared state (`disposed` flag). Root dispose marks the tree dead.
    shared: Arc<SharedProviderState>,
    /// This provider disposed flag (root dispose or scope dispose/Drop).
    disposed: AtomicBool,
    /// Root provider (not a scope): only the root applies scope validation.
    is_root: bool,
    /// Whether Scoped services may be resolved from this root provider.
    validate_scopes: bool,
}

impl ServiceProvider {
    pub(crate) fn new_with(
        store: ServiceStore,
        options: ServiceProviderOptions,
    ) -> Result<Arc<Self>, RdiError> {
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
            scope_factory_cache: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
            shared: Arc::new(SharedProviderState::new()),
            disposed: AtomicBool::new(false),
            is_root: true,
            validate_scopes: options.validate_scopes,
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

    /// Collect sync singleton entries in **registration order** and eagerly
    /// execute their factories.
    ///
    /// Reuses instances cached by validation (build time) so factories aren't
    /// double-fired. Called by both `new` and `new_async` (async singletons skipped).
    ///
    /// Ordering is deterministic (sorted by `cache_key` = registration index)
    /// instead of relying on `HashMap` iteration order, so side effects and
    /// validation errors are reproducible across runs.
    fn init_sync_singletons(sp_arc: &Arc<Self>) {
        let mut singleton_entries: Vec<(usize, ServiceFactory)> = sp_arc
            .store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_none())
            .map(|e| (e.cache_key, e.factory.clone()))
            .collect();
        singleton_entries.sort_by_key(|(cache_key, _)| *cache_key);

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
    pub(crate) async fn new_async_with(
        store: ServiceStore,
        options: ServiceProviderOptions,
    ) -> Result<Arc<Self>, RdiError> {
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
            scope_factory_cache: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
            shared: Arc::new(SharedProviderState::new()),
            disposed: AtomicBool::new(false),
            is_root: true,
            validate_scopes: options.validate_scopes,
        });
        sp_arc.self_weak.get_or_init(|| Arc::downgrade(&sp_arc));

        // Phase 2: validate sync singleton dependencies and cache instances.
        Self::validate_dependencies(&sp_arc)?;

        // Phase 3: eagerly execute sync singletons not already built.
        Self::init_sync_singletons(&sp_arc);

        // Phase 4: initialize async singletons dependency-first.
        //
        // Registration order must not matter: an async singleton factory may
        // `await` another async singleton registered later. `ensure_async_singleton`
        // recursively initializes dependencies (single-flight, per-poll cycle
        // detection), so the resulting creation order is a deterministic
        // dependency-first topological order rather than registration order.
        let mut async_singletons: Vec<&ServiceEntry> = sp_arc
            .store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_some())
            .collect();
        async_singletons.sort_by_key(|e| e.cache_key);

        for entry in async_singletons {
            sp_arc.ensure_async_singleton(entry).await?;
        }

        Ok(sp_arc)
    }

    /// Ensure that an async singleton entry is initialized exactly once.
    ///
    /// Dependency-driven and registration-order independent: if a factory
    /// `await`s an async singleton that has not been initialized yet, the nested
    /// `get_async` call re-enters this method and recursively initializes the
    /// dependency first (single-flight via the shared async cache).
    ///
    /// Recursive self/cross cycles are detected by the per-poll resolution guard
    /// and reported as [`RdiError::CircularDependency`] instead of dead-locking.
    /// Also used as a lazy safety net by `get_async` after build.
    async fn ensure_async_singleton(&self, entry: &ServiceEntry) -> Result<(), RdiError> {
        self.check_alive()?;
        if self.singleton_cache.get(entry.cache_key).is_some() {
            return Ok(());
        }
        let Some(async_factory) = entry.async_factory.clone() else {
            // Synchronous singleton: already eagerly initialized by build.
            return Ok(());
        };

        // Nested self-await would dead-lock on the per-key async cache lock.
        check_not_resolving(entry.cache_key, entry.type_name)?;

        let key = entry.cache_key;
        let type_name = entry.type_name;
        let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
            service: type_name.to_string(),
            payload: "provider_arc() returned None for async singleton initialization".into(),
        })?;

        self.singleton_cache
            .get_or_init_with_async(key, || {
                poll_guard(
                    key,
                    type_name,
                    Self::run_async_factory_safely(type_name, async_factory, provider_arc),
                )
            })
            .await?;
        Ok(())
    }

    /// 创建一个 scope 范围的 ServiceProvider，共享 store/type_map/singleton_cache/named，
    /// 独立 scoped 缓存、disposables 与 self_weak。
    ///
    /// 供 [`Scope::new`](crate::scope::Scope::new) 调用。这使 `#[inject(provider)]`
    /// 字段拿到的是 **scope 范围的 provider** 而非根 provider——经该字段解析 Scoped 服务
    /// 会命中 scope 自己的 scoped 缓存，符合 MEDI `IServiceScope` 语义。
    pub(crate) fn new_scope(parent: &Arc<Self>) -> Arc<Self> {
        let sp = Self {
            store: Arc::clone(&parent.store),
            type_map: Arc::clone(&parent.type_map),
            singleton_cache: Arc::clone(&parent.singleton_cache),
            root_scoped_cache: LazyCache::new(),
            named: Arc::clone(&parent.named),
            self_weak: OnceLock::new(),
            scope_factory_cache: OnceLock::new(),
            disposables: Mutex::new(Vec::new()),
            shared: Arc::clone(&parent.shared),
            disposed: AtomicBool::new(false),
            is_root: false,
            validate_scopes: parent.validate_scopes,
        };
        let arc = Arc::new(sp);
        let this: &Self = &arc;
        this.self_weak.get_or_init(|| Arc::downgrade(&arc));
        arc
    }

    /// Register a cleanup closure for scoped services/owned resources.
    ///
    /// Closures run (in **reverse registration order**) when
    /// [`Self::dispose`] is called — either explicitly or by [`Scope`](crate::Scope)'s
    /// `Drop` for scope providers.
    pub fn register_disposable(&self, cleanup: Box<dyn FnOnce() + Send>) {
        self.disposables.lock().unwrap().push(cleanup);
    }

    /// Dispose this provider: release cached instances in reverse creation order,
    /// run registered cleanups in reverse registration order, and mark the
    /// provider unusable.
    ///
    /// - Root provider: also marks the whole provider tree disposed (existing
    ///   scopes can no longer resolve), drains the singleton cache, and clears
    ///   named registrations. Singleton instances are released in reverse
    ///   creation order (LIFO), so dependents drop before their dependencies.
    /// - Scope provider: releases the scoped instances created in that scope and
    ///   runs its cleanups. Singletons are untouched.
    ///
    /// Idempotent. Instances are released by dropping the container's strong
    /// references; a service's `Drop` runs when no other owner keeps it alive.
    /// Services obtained via `get_owned` are caller-owned and are not tracked.
    ///
    /// The caller must not resolve services concurrently with disposal (same
    /// contract as MEDI's `ServiceProvider.Dispose`). A panicking cleanup closure
    /// does not stop the remaining cleanups; the first panic is re-raised after
    /// disposal (and swallowed when the thread is already unwinding, e.g. during
    /// `Scope`'s `Drop`).
    pub fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.is_root {
            self.shared.disposed.store(true, Ordering::SeqCst);
        }

        // Release instances (reverse LIFO order). User `Drop` panics are
        // collected so the remaining instances/cleanups still run.
        let mut first_panic = drop_instances_safely(self.root_scoped_cache.drain_reverse());

        if self.is_root {
            // Release singletons (reverse creation order) and clear plugins.
            let singleton_panic = drop_instances_safely(self.singleton_cache.drain_reverse());
            if first_panic.is_none() {
                first_panic = singleton_panic;
            }
            self.named.write().unwrap().clear();
        }

        let cleanup_panic = self.run_cleanups();
        if first_panic.is_none() {
            first_panic = cleanup_panic;
        }

        if let Some(payload) = first_panic {
            if !std::thread::panicking() {
                std::panic::resume_unwind(payload);
            }
        }
    }

    /// Drain and execute cleanup closures in reverse registration order.
    ///
    /// The first cleanup panic is returned to the caller instead of being
    /// raised here, so disposal can finish all remaining work first. During
    /// unwinding (drop path) the panic is swallowed to avoid a double-panic
    /// abort.
    fn run_cleanups(&self) -> Option<Box<dyn Any + Send>> {
        let mut cleanups: Vec<Box<dyn FnOnce() + Send>> = {
            let mut guard = self.disposables.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        cleanups.reverse();

        let mut first_panic: Option<Box<dyn Any + Send>> = None;
        for cleanup in cleanups {
            if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(cleanup)) {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                }
            }
        }
        first_panic
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
    ///
    /// Validation order is deterministic (sorted by `cache_key` = registration
    /// index), so the first reported error is stable across runs.
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

        let mut singletons: Vec<&ServiceEntry> = sp
            .store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton && e.async_factory.is_none())
            .collect();
        singletons.sort_by_key(|e| e.cache_key);

        // Validate each sync singleton by attempting to resolve it.
        // Scoped/Transient are not eagerly executed at build (avoids side effects);
        // their cycles are detected at runtime via a thread-local resolution stack.
        //
        // Each registration is validated individually: duplicate registrations
        // (MEDI override semantics) are all constructed eagerly, and each factory
        // must pass the cycle/captive checks under its own cache key.
        for entry in singletons {
            let resolver = ValidationResolver::new(
                &sp.store,
                &type_names,
                &sp.singleton_cache,
                Arc::clone(sp),
            );
            let type_name = entry.type_name;

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                resolver.validate_entry(entry)
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

        Ok(())
    }

    // ── Registration lookup (single-service resolution: last registration wins) ──

    /// Last unkeyed registration for `tid` (MEDI: later registrations override
    /// earlier ones when resolving a single service).
    fn unkeyed_entry(&self, tid: TypeId) -> Option<&ServiceEntry> {
        self.store.get(&tid)?.iter().rev().find(|e| e.key.is_none())
    }

    /// Last registration for `tid` + `key` (same-key duplicates: last wins).
    fn keyed_entry(&self, tid: TypeId, key: &str) -> Option<&ServiceEntry> {
        self.store
            .get(&tid)?
            .iter()
            .rev()
            .find(|e| e.key.as_deref() == Some(key))
    }

    fn entry_by_str(&self, type_name: &str, key: Option<&str>) -> Option<&ServiceEntry> {
        let tid = *self.type_map.get(type_name)?;
        match key {
            Some(k) => self.keyed_entry(tid, k),
            None => self.unkeyed_entry(tid),
        }
    }

    // ── Liveness / scope validation ──

    /// Whether this provider (or its root) has been disposed.
    fn check_alive(&self) -> Result<(), RdiError> {
        if self.shared.disposed.load(Ordering::SeqCst) {
            return Err(RdiError::ProviderDisposed);
        }
        if self.disposed.load(Ordering::SeqCst) {
            return Err(RdiError::ScopeDisposed);
        }
        Ok(())
    }

    /// MEDI `ValidateScopes` equivalent: a scoped service must not be resolved
    /// from the root provider (it would be cached for the container lifetime).
    fn check_scope_validation(&self, entry: &ServiceEntry) -> Result<(), RdiError> {
        if entry.lifetime == ServiceLifetime::Scoped && self.is_root && self.validate_scopes {
            return Err(RdiError::ScopedFromRootProvider {
                service: entry.type_name.to_string(),
            });
        }
        Ok(())
    }

    fn check_resolvable(&self, entry: &ServiceEntry) -> Result<(), RdiError> {
        self.check_alive()?;
        self.check_scope_validation(entry)
    }

    // ── Resolution cores ──

    /// Resolve a single entry, converting liveness/scope violations, cycles and
    /// factory panics into typed [`RdiError`].
    fn resolve_entry_catching(&self, entry: &ServiceEntry) -> Result<AnyService, RdiError> {
        self.check_resolvable(entry)?;
        match std::panic::catch_unwind(AssertUnwindSafe(|| self.get_any_by_entry(entry))) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(RdiError::not_found(entry.type_name, None)),
            Err(payload) => Err(RdiError::from_panic_payload(entry.type_name, payload)),
        }
    }

    /// Resolve a single entry, panicking with a typed [`RdiError`] on liveness or
    /// scope violations (used by [`IServiceResolver`], whose contract is
    /// infallible `Option`; required-resolution APIs catch these panics and
    /// return `Err`).
    fn resolve_entry_panicking(&self, entry: &ServiceEntry) -> AnyService {
        if let Err(err) = self.check_resolvable(entry) {
            std::panic::panic_any(err);
        }
        match self.get_any_by_entry(entry) {
            Some(v) => v,
            None => std::panic::panic_any(RdiError::not_found(entry.type_name, None)),
        }
    }

    /// Type-erased resolution by type name, for the resolver trait.
    ///
    /// Single-service semantics: **last registration wins** (MEDI), both for
    /// unkeyed and for same-key keyed registrations.
    fn resolve_unkeyed_optional<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Option<Arc<T>>, RdiError> {
        self.check_alive()?;
        if let Some(entry) = self.unkeyed_entry(TypeId::of::<T>()) {
            return Ok(Self::extract(self.resolve_entry_catching(entry)?));
        }
        // Built-in container services (ScopeFactory / ServiceProvider) are
        // always available from a built provider; explicit registrations win
        // because the store lookup above runs first.
        Ok(self
            .builtin_service_by_type_id(TypeId::of::<T>())
            .and_then(Self::extract))
    }

    /// Resolve a service by type. Works uniformly for concrete types and trait objects.
    /// Returns `Err(RdiError::ServiceNotFound)` if not registered.
    ///
    /// [`ScopeFactory`] is always available from a built provider (no manual
    /// registration required). An explicit `singleton::<dyn ScopeFactory>(...)`
    /// registration still takes precedence. Later registrations override earlier
    /// ones (MEDI semantics).
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        match self.resolve_unkeyed_optional::<T>() {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(RdiError::not_found(type_name, None)),
            Err(e) => Err(e),
        }
    }

    /// Resolve a service by type, returning `None` if it is not registered **or
    /// not resolvable at this point** (e.g. a scoped service under root scope
    /// validation, or a disposed provider). Use [`Self::get`] when you need the
    /// concrete error.
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.resolve_unkeyed_optional::<T>().ok().flatten()
    }

    /// Resolve an async service (transient or scoped) by type.
    ///
    /// - Singletons are resolved synchronously (already cached during `build_async`).
    /// - Scoped services are cached in this provider's scoped cache (one instance per scope).
    /// - Transient services call the async factory fresh each time.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .unkeyed_entry(tid)
            .ok_or_else(|| RdiError::not_found(type_name, None))?;

        if entry.lifetime == ServiceLifetime::Singleton {
            // Async singletons are initialized dependency-first; this is also a
            // lazy safety net after build. Registration order is irrelevant.
            self.ensure_async_singleton(entry).await?;
            return self.get::<T>();
        }
        self.check_resolvable(entry)?;

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
            .keyed_entry(tid, key)
            .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None))?;

        if entry.lifetime == ServiceLifetime::Singleton {
            self.ensure_async_singleton(entry).await?;
            return self.get_keyed::<T>(key);
        }
        self.check_resolvable(entry)?;

        match self.resolve_async_any(entry, type_name).await? {
            Some(arc_any) => Self::extract(arc_any)
                .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None)),
            None => self.get_keyed::<T>(key),
        }
    }

    /// Resolve async Scoped/Transient factory, returning `Some(AnyService)`.
    /// Returns `None` if the entry has no async factory (caller falls back to the
    /// synchronous resolution path).
    ///
    /// A resolution guard is taken for the duration of the async factory so a
    /// factory that recursively awaits a service already being resolved returns
    /// [`RdiError::CircularDependency`] instead of dead-locking on its own cache
    /// lock. (Detection is thread-local and covers the recursive poll chain; an
    /// async factory must not recurse into the service it is building.)
    async fn resolve_async_any(
        &self,
        entry: &ServiceEntry,
        type_name: &'static str,
    ) -> Result<Option<AnyService>, RdiError> {
        match entry.lifetime {
            ServiceLifetime::Scoped => {
                // Sync-registered scoped service: fall back to the synchronous
                // path (caller resolves via `get`), preserving mixed sync/async
                // graphs.
                let Some(async_factory) = entry.async_factory.clone() else {
                    return Ok(None);
                };
                // Nested self-await would dead-lock on the per-key async lock.
                check_not_resolving(entry.cache_key, entry.type_name)?;
                let provider_arc = self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                    service: type_name.to_string(),
                    payload: "provider_arc() returned None for async resolution".into(),
                })?;
                poll_guard(
                    entry.cache_key,
                    entry.type_name,
                    self.root_scoped_cache
                        .get_or_init_with_async(entry.cache_key, || {
                            Self::run_async_factory_safely(type_name, async_factory, provider_arc)
                        }),
                )
                .await
                .map(Some)
            }
            ServiceLifetime::Transient => {
                if let Some(async_factory) = entry.async_factory.clone() {
                    check_not_resolving(entry.cache_key, entry.type_name)?;
                    let provider_arc =
                        self.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
                            service: type_name.to_string(),
                            payload: "provider_arc() returned None for async resolution".into(),
                        })?;
                    poll_guard(
                        entry.cache_key,
                        entry.type_name,
                        Self::run_async_factory_safely(type_name, async_factory, provider_arc),
                    )
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
        if let Some(entry) = self.keyed_entry(TypeId::of::<T>(), key) {
            let any = self.resolve_entry_catching(entry)?;
            return Self::extract(any)
                .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None));
        }
        Err(RdiError::keyed_not_found(type_name, key, None))
    }

    /// Resolve owned `T` (bypass cache, fresh each call).
    /// Returns `Err` if not registered or the service is Singleton.
    ///
    /// Owned resolution hands the instance to the caller: it is not tracked by
    /// the container and stays valid after `dispose`. Scoped ownership bypasses
    /// the scope cache, so a root scope-validation error is not raised for the
    /// owned instance itself; its *dependencies* still obey scope validation.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .unkeyed_entry(tid)
            .ok_or_else(|| RdiError::not_found(type_name, None))?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return Err(RdiError::OwnedNotSupported(type_name));
        }
        self.check_alive()?;
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

    /// Resolve owned `T`, returning `None` if not registered, a Singleton, or the
    /// provider/scope is disposed.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        self.get_owned().ok()
    }

    /// Resolve owned keyed `T`.
    /// Returns `Err` if not found or Singleton.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> {
        let type_name = std::any::type_name::<T>();
        let tid = TypeId::of::<T>();
        let entry = self
            .keyed_entry(tid, key)
            .ok_or_else(|| RdiError::keyed_not_found(type_name, key, None))?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return Err(RdiError::OwnedNotSupported(type_name));
        }
        self.check_alive()?;
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

    /// Resolve owned keyed `T`, returning `None` if not found, a Singleton, or
    /// the provider/scope is disposed.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        self.get_keyed_owned(key).ok()
    }

    /// Return all **unkeyed** registered instances of the given type, in
    /// registration order (MEDI `GetServices<T>()` / `IEnumerable<T>` parity).
    ///
    /// Keyed implementations are excluded — use [`Self::get_all_keyed`] for
    /// those. Under root scope validation, scoped services are not resolvable at
    /// the root and are omitted; other resolution failures panic with a typed
    /// [`RdiError`] payload (mirroring MEDI, where enumeration throws).
    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.collect_all::<T>(false)
    }

    /// Return all **keyed** registered instances of the given type, in
    /// registration order (MEDI `GetKeyedServices<T>(KeyedService.AnyKey)` parity).
    pub fn get_all_keyed<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.collect_all::<T>(true)
    }

    fn collect_all<T: ?Sized + Send + Sync + 'static>(&self, keyed_only: bool) -> Vec<Arc<T>> {
        if self.check_alive().is_err() {
            return Vec::new();
        }
        let Some(entries) = self.store.get(&TypeId::of::<T>()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.iter().filter(|e| e.key.is_some() == keyed_only) {
            match self.resolve_entry_catching(entry) {
                Ok(any) => {
                    if let Some(v) = Self::extract(any) {
                        out.push(v);
                    }
                }
                // Root scope validation: scoped services are not resolvable at
                // the root, so they do not appear in enumerations.
                Err(RdiError::ScopedFromRootProvider { .. }) => {}
                Err(err) => std::panic::panic_any(err),
            }
        }
        out
    }

    /// Create a new service scope.
    ///
    /// Analogous to `IServiceProvider.CreateScope()` in MEDI.
    /// Scoped-lifetime services are cached within the returned scope.
    pub fn scope(self: &Arc<Self>) -> crate::scope::Scope {
        crate::scope::Scope::new(self.clone())
    }

    /// Whether `T` has a resolvable registration (or is a built-in container
    /// service). MEDI `IServiceProviderIsService` parity.
    ///
    /// Reports registration state only: it is still `true` after [`Self::dispose`].
    pub fn is_registered<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        let tid = TypeId::of::<T>();
        self.store
            .get(&tid)
            .is_some_and(|entries| entries.iter().any(|e| e.key.is_none()))
            || tid == TypeId::of::<dyn ScopeFactory>()
            || tid == TypeId::of::<ServiceProvider>()
    }

    /// Whether `T` has a registration under `key`.
    pub fn is_registered_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> bool {
        self.store
            .get(&TypeId::of::<T>())
            .is_some_and(|entries| entries.iter().any(|e| e.key.as_deref() == Some(key)))
    }

    /// Built-in `dyn ScopeFactory` singleton for this provider (weak-backed).
    fn builtin_scope_factory(&self) -> Option<Arc<dyn ScopeFactory>> {
        let weak = self.self_weak.get()?.clone();
        Some(Arc::clone(
            self.scope_factory_cache
                .get_or_init(|| crate::scope::builtin_scope_factory(weak)),
        ))
    }

    fn builtin_scope_factory_any(&self) -> Option<AnyService> {
        Some(crate::scope::builtin_scope_factory_any(
            self.builtin_scope_factory()?,
        ))
    }

    /// Cached built-in `dyn ScopeFactory` service for the validation resolver.
    ///
    /// Exposed `pub(crate)` so eager singletons receive the same factory
    /// instance as `provider.get::<dyn ScopeFactory>()` (singleton identity).
    pub(crate) fn builtin_scope_factory_any_cached(&self) -> Option<AnyService> {
        self.builtin_scope_factory_any()
    }

    /// Resolve container-provided (built-in) services that need no registration.
    ///
    /// - `dyn ScopeFactory`: weak-backed singleton factory that creates scopes.
    /// - `ServiceProvider`: the container itself (MEDI `IServiceProvider` parity),
    ///   enabling `#[inject] Arc<ServiceProvider>` in addition to the dedicated
    ///   `#[inject(provider)]` field attribute.
    fn builtin_service_by_type_id(&self, tid: TypeId) -> Option<AnyService> {
        if tid == TypeId::of::<dyn ScopeFactory>() {
            return self.builtin_scope_factory_any();
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
            return self.builtin_scope_factory_any();
        }
        if key == std::any::type_name::<ServiceProvider>() {
            let provider = self.self_weak.get()?.upgrade()?;
            return Some(Arc::new(provider) as AnyService);
        }
        None
    }

    pub(crate) fn get_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
        reject_build_time_provider_resolution(entry.type_name);
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
    ///   `singleton_cache` / scoped cache. The factory uses `self`
    ///   (ServiceProvider, i.e. current scope) as resolver, so Scoped
    ///   dependencies resolved transitively bind to this provider's scoped cache.
    pub(crate) fn create_owned_any_by_entry(&self, entry: &ServiceEntry) -> Option<AnyService> {
        reject_build_time_provider_resolution(entry.type_name);
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
    ///
    /// No-op after the provider/scope has been disposed (the named registry is
    /// cleared by [`Self::dispose`]).
    pub fn register_named<T: Send + Sync + 'static>(&self, name: &str, service: Arc<T>) {
        if self.check_alive().is_err() {
            return;
        }
        self.named
            .write()
            .unwrap()
            .insert(name.to_string(), service);
    }

    /// Remove a named service (for plugin unload).
    pub fn remove_named(&self, name: &str) {
        if self.check_alive().is_err() {
            return;
        }
        self.named.write().unwrap().remove(name);
    }

    /// Register a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    ///
    /// No-op after the provider/scope has been disposed.
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        if self.check_alive().is_err() {
            return;
        }
        self.named
            .write()
            .unwrap()
            .insert(name.to_string(), service);
    }

    /// Remove a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_remove_named(&self, name: &str) {
        if self.check_alive().is_err() {
            return;
        }
        self.named.write().unwrap().remove(name);
    }

    /// Built-in container services resolvable through the trait path, honoring
    /// liveness (a disposed provider no longer serves them; the violation is
    /// raised as a typed panic so required-resolution APIs convert it to `Err`).
    fn builtin_service_checked(&self, key: &str) -> Option<AnyService> {
        if let Err(err) = self.check_alive() {
            std::panic::panic_any(err);
        }
        self.builtin_service_by_name(key)
    }

    fn builtin_service_by_type_id_checked(&self, tid: TypeId) -> Option<AnyService> {
        if let Err(err) = self.check_alive() {
            std::panic::panic_any(err);
        }
        self.builtin_service_by_type_id(tid)
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
        if let Some(entry) = self.entry_by_str(key, None) {
            return Some(self.resolve_entry_panicking(entry));
        }
        // Built-in services resolve through the string path used by the inject
        // macros (`#[inject] Arc<dyn ScopeFactory>`, `Arc<ServiceProvider>`).
        self.builtin_service_checked(key)
    }

    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, Some(variant))?;
        Some(self.resolve_entry_panicking(entry))
    }

    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(entry) = self.unkeyed_entry(tid) {
            return Some(self.resolve_entry_panicking(entry));
        }
        self.builtin_service_by_type_id_checked(tid)
    }

    fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.keyed_entry(tid, key)?;
        Some(self.resolve_entry_panicking(entry))
    }

    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.collect_all_any(key, false)
    }

    fn get_all_keyed_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.collect_all_any(key, true)
    }

    fn get_all_any_result(&self, key: &str) -> Result<Vec<Arc<dyn Any + Send + Sync>>, RdiError> {
        self.check_alive()?;
        let Some(tid) = self.type_map.get(key) else {
            return Ok(Vec::new());
        };
        let Some(entries) = self.store.get(tid) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in entries.iter().filter(|e| e.key.is_none()) {
            out.push(self.resolve_entry_catching(entry)?);
        }
        Ok(out)
    }

    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, None)?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return None;
        }
        self.check_alive().ok()?;
        self.create_owned_any_by_entry(entry)
    }

    /// Infallible lookup: reports absence instead of panicking for scope
    /// violations / disposal (still returns `Err` for factory failures so
    /// callers could distinguish, but [`IServiceResolver::try_get`] maps it to
    /// `None`).
    fn try_get_any_result(&self, key: &str) -> Result<Option<AnyService>, RdiError> {
        self.check_alive()?;
        if let Some(entry) = self.entry_by_str(key, None) {
            return self.resolve_entry_catching(entry).map(Some);
        }
        Ok(self.builtin_service_by_name(key))
    }

    /// Keyed variant of [`IServiceResolver::try_get_any_result`].
    fn try_get_keyed_any_result(
        &self,
        key: &str,
        variant: &str,
    ) -> Result<Option<AnyService>, RdiError> {
        self.check_alive()?;
        match self.entry_by_str(key, Some(variant)) {
            Some(entry) => self.resolve_entry_catching(entry).map(Some),
            None => Ok(None),
        }
    }

    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.entry_by_str(key, Some(variant))?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return None;
        }
        self.check_alive().ok()?;
        self.create_owned_any_by_entry(entry)
    }

    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        self.self_weak.get().and_then(|w| w.upgrade())
    }
}

impl ServiceProvider {
    fn collect_all_any(&self, key: &str, keyed_only: bool) -> Vec<Arc<dyn Any + Send + Sync>> {
        if self.check_alive().is_err() {
            return Vec::new();
        }
        let Some(tid) = self.type_map.get(key) else {
            return Vec::new();
        };
        let Some(entries) = self.store.get(tid) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.iter().filter(|e| e.key.is_some() == keyed_only) {
            match self.resolve_entry_catching(entry) {
                Ok(any) => out.push(any),
                Err(RdiError::ScopedFromRootProvider { .. }) => {}
                Err(err) => std::panic::panic_any(err),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceProviderOptions;
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
    fn get_all_includes_only_unkeyed() {
        let p = ServiceCollection::new()
            .keyed_singleton("a", |_| Arc::new(Calc(1)))
            .keyed_singleton("b", |_| Arc::new(Calc(2)))
            .build()
            .unwrap();
        assert_eq!(p.get_all::<Calc>().len(), 0, "get_all excludes keyed");
        assert_eq!(p.get_all_keyed::<Calc>().len(), 2);
    }
    #[test]
    fn later_registration_wins() {
        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(Calc(1)))
            .singleton(|_| Arc::new(Calc(2)))
            .build()
            .unwrap();
        assert_eq!(
            p.get::<Calc>().unwrap().0,
            2,
            "MEDI: last registration wins"
        );
    }
    #[test]
    fn root_scoped_is_rejected_by_default() {
        let p = ServiceCollection::new()
            .scoped(|_| Arc::new(Calc(9)))
            .build()
            .unwrap();
        match p.get::<Calc>() {
            Err(crate::error::RdiError::ScopedFromRootProvider { service }) => {
                assert!(service.contains("Calc"));
            }
            other => panic!("expected ScopedFromRootProvider, got {other:?}"),
        }
        assert!(p.get_optional::<Calc>().is_none());
        assert!(p.get_all::<Calc>().is_empty());
    }
    #[test]
    fn lenient_options_restore_root_scope() {
        let p = ServiceCollection::new()
            .scoped(|_| Arc::new(Calc(9)))
            .build_with(ServiceProviderOptions::lenient())
            .unwrap();
        assert_eq!(p.get::<Calc>().unwrap().0, 9);
        assert!(Arc::ptr_eq(
            &p.get::<Calc>().unwrap(),
            &p.get::<Calc>().unwrap()
        ));
    }
    #[test]
    fn scope_resolution_works_and_is_cached() {
        let p = ServiceCollection::new()
            .scoped(|_| Arc::new(Calc(3)))
            .build()
            .unwrap();
        let s1 = p.scope();
        let a = s1.get::<Calc>().unwrap();
        let b = s1.get::<Calc>().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let s2 = p.scope();
        assert!(!Arc::ptr_eq(&a, &s2.get::<Calc>().unwrap()));
    }
    #[test]
    fn is_registered_reports_lookup_state() {
        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(Calc(1)))
            .keyed_singleton("k", |_| Arc::new(Calc(2)))
            .build()
            .unwrap();
        assert!(p.is_registered::<Calc>());
        assert!(p.is_registered::<crate::ServiceProvider>());
        assert!(p.is_registered::<dyn crate::ScopeFactory>());
        assert!(!p.is_registered::<String>());
        assert!(p.is_registered_keyed::<Calc>("k"));
        assert!(!p.is_registered_keyed::<Calc>("missing"));
    }
    #[test]
    fn dispose_is_idempotent_and_blocks_resolution() {
        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(Calc(1)))
            .build()
            .unwrap();
        p.dispose();
        p.dispose();
        assert!(matches!(
            p.get::<Calc>(),
            Err(crate::error::RdiError::ProviderDisposed)
        ));
        assert!(p.get_optional::<Calc>().is_none());
        assert!(p.get_all::<Calc>().is_empty());
    }
    #[test]
    fn dispose_releases_singletons_in_reverse_creation_order() {
        use std::sync::Mutex;
        static DROPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        #[derive(Debug)]
        struct Tracked(&'static str);
        impl Drop for Tracked {
            fn drop(&mut self) {
                DROPS.lock().unwrap().push(self.0);
            }
        }
        struct First(#[allow(dead_code)] Arc<Tracked>);
        struct Second(#[allow(dead_code)] Arc<Tracked>);

        let p = ServiceCollection::new()
            .singleton(|_| Arc::new(First(Arc::new(Tracked("first")))))
            .singleton(|_| Arc::new(Second(Arc::new(Tracked("second")))))
            .build()
            .unwrap();
        p.dispose();

        let drops = DROPS.lock().unwrap().clone();
        assert_eq!(
            drops,
            vec!["second", "first"],
            "singletons must be released in reverse creation order"
        );
    }

    #[test]
    fn eager_singletons_are_created_in_registration_order() {
        use std::sync::Mutex;
        static CREATED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        #[derive(Debug)]
        struct A;
        #[derive(Debug)]
        struct B;
        let p = ServiceCollection::new()
            .singleton(|_| {
                CREATED.lock().unwrap().push("a");
                Arc::new(A)
            })
            .singleton(|_| {
                CREATED.lock().unwrap().push("b");
                Arc::new(B)
            })
            .build()
            .unwrap();
        assert_eq!(
            *CREATED.lock().unwrap(),
            vec!["a", "b"],
            "eager init must be deterministic (registration order)"
        );
        drop(p);
    }

    #[test]
    fn async_singleton_panic_becomes_typed_error() {
        let result = futures::executor::block_on(
            ServiceCollection::new()
                .async_singleton::<Calc>(|_| Box::pin(async { panic!("boom") }))
                .build_async(),
        );
        match result {
            Ok(_) => panic!("build_async must fail when an async singleton factory panics"),
            Err(crate::error::RdiError::FactoryPanic { payload, .. }) => {
                assert!(payload.contains("boom"), "payload={payload}");
            }
            Err(other) => panic!("expected FactoryPanic, got {other:?}"),
        }
    }
}
