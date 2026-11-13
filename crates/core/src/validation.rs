use crate::cache::LazyCache;
use crate::entry::{IServiceResolver, ServiceEntry, ServiceLifetime, ServiceStore};
use crate::error::RdiError;
use crate::key::ServiceKey;
use crate::provider::ServiceProvider;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Validation resolver that tracks resolution chains for cycle detection
/// and captive dependency detection (Singleton depending on Scoped).
///
/// Both checks run during `build()` by executing singleton factories
/// through this resolver. Errors are stored in `last_error` instead of using
/// `panic_any`, making the control flow explicit and idiomatic.
///
/// The provider `Arc` is available during validation (the container is created
/// before validation runs), so factories may inject the container itself via
/// `#[inject(provider)]` or the built-in [`crate::ScopeFactory`]. Resolving
/// services through that injected provider *inside a constructor* is rejected:
/// it would bypass the build-time cycle/captive checks and make the validation
/// result depend on store iteration order. Store the handle and resolve lazily in
/// methods instead (see [`crate::provider::BuildFactoryGuard`]).
///
/// Single-service lookups follow the runtime contract: **last registration
/// wins** (MEDI semantics).
///
/// A synchronous singleton factory cannot await an async service. When such a
/// resolution is attempted during validation the resolver reports a precise
/// [`RdiError::SyncSingletonDependsOnAsyncService`] instead of the generic
/// async-build error.
///
/// `AnyKey` fallback registrations are not eagerly validated (they need a
/// requested key); when a validated factory requests one, the keyed factory is
/// executed for that key without being cached.
///
/// The `catch_unwind` in `validate_dependencies` only catches user factory
/// panics (triggered by `None` returns from this resolver), not framework errors.
pub(crate) struct ValidationResolver<'a> {
    store: &'a ServiceStore,
    /// 解析栈：按解析顺序记录 cache_key，用于循环依赖检测与有序路径输出。
    resolving: RwLock<Vec<usize>>,
    /// 解析栈：记录当前解析链中每个节点的 (cache_key, lifetime)，
    /// 用于 captive dependency 检测（Singleton 依赖 Scoped）。
    stack: RwLock<Vec<(usize, ServiceLifetime)>>,
    /// TypeId → type_name map for error messages
    type_names: &'a HashMap<TypeId, &'static str>,
    /// Reusable instance cache — populated as factories execute during
    /// validation so that Phase 3 (eager singleton init) can reuse the
    /// results instead of re-executing factories.
    cache: &'a LazyCache,
    /// The provider being validated. Available to factories through
    /// `provider_arc()` / the built-in ScopeFactory.
    provider: Arc<ServiceProvider>,
    /// The sync singleton currently being validated (for precise async diagnostics).
    consumer: &'static str,
    /// Stores the first validation error (cycle or captive) encountered.
    /// Cleared by [`Self::take_error`] for consumption by the caller.
    last_error: Mutex<Option<RdiError>>,
}

impl<'a> ValidationResolver<'a> {
    pub(crate) fn new(
        store: &'a ServiceStore,
        type_names: &'a HashMap<TypeId, &'static str>,
        cache: &'a LazyCache,
        provider: Arc<ServiceProvider>,
        consumer: &'static str,
    ) -> Self {
        Self {
            store,
            resolving: RwLock::new(Vec::new()),
            stack: RwLock::new(Vec::new()),
            type_names,
            cache,
            provider,
            consumer,
            last_error: Mutex::new(None),
        }
    }

    /// Consume the stored validation error, if any.
    pub(crate) fn take_error(&self) -> Option<RdiError> {
        self.last_error.lock().unwrap().take()
    }

    fn get_type_name(&self, cache_key: usize) -> String {
        self.store
            .iter()
            .find(|(_, entries)| entries.iter().any(|e| e.cache_key == cache_key))
            .and_then(|(_, entries)| entries.first())
            .map(|e| {
                e.type_name
                    .split("::")
                    .last()
                    .unwrap_or(e.type_name)
                    .to_string()
            })
            .unwrap_or_else(|| format!("service {}", cache_key))
    }

    /// Last unkeyed registration for `tid` (MEDI: later registrations override
    /// earlier ones when resolving a single service).
    fn unkeyed_entry(&self, tid: TypeId) -> Option<&ServiceEntry> {
        self.store.get(&tid)?.iter().rev().find(|e| e.key.is_none())
    }

    /// Last registration for `tid` + `key`; falls back to the `AnyKey` entry.
    fn keyed_entry(&self, tid: TypeId, key: &ServiceKey) -> Option<&ServiceEntry> {
        let entries = self.store.get(&tid)?;
        entries
            .iter()
            .rev()
            .find(|e| e.key.as_ref() == Some(key))
            .or_else(|| {
                entries
                    .iter()
                    .rev()
                    .find(|e| e.key.as_ref().is_some_and(ServiceKey::is_any))
            })
    }

    fn tid_by_name(&self, key: &str) -> Option<TypeId> {
        self.type_names
            .iter()
            .find(|(_, name)| **name == key)
            .map(|(tid, _)| *tid)
    }

    /// Precisely report "sync singleton depends on async service" (including
    /// transitive dependencies).
    fn reject_async_dependency(&self, entry: &ServiceEntry) -> bool {
        if entry.is_async() {
            *self.last_error.lock().unwrap() = Some(RdiError::SyncSingletonDependsOnAsyncService {
                singleton: self.consumer.to_string(),
                async_service: entry.type_name.to_string(),
            });
            return true;
        }
        false
    }

    /// Captive dependency（囚徒依赖）检测。
    ///
    /// 检测 Singleton 依赖链中解析 Scoped 服务的情况。命中时存储错误到
    /// `last_error` 并返回 `true`，调用方应尽早返回以避免继续执行。
    ///
    /// 回溯查找的是**任意 Singleton 祖先**（非仅直接父节点），因此间接链
    /// `Singleton → Transient → Scoped` 也能被检出。
    fn check_captive_dependency(&self, entry: &ServiceEntry) -> bool {
        if entry.lifetime == ServiceLifetime::Scoped {
            let stack = self.stack.read().unwrap();
            if let Some(&(ancestor_ck, ServiceLifetime::Singleton)) = stack
                .iter()
                .find(|(_, lt)| *lt == ServiceLifetime::Singleton)
            {
                let singleton_name = self.get_type_name(ancestor_ck);
                let scoped_name = self.get_type_name(entry.cache_key);
                *self.last_error.lock().unwrap() = Some(RdiError::SingletonDependsOnScoped {
                    singleton: singleton_name,
                    scoped: scoped_name,
                });
                return true;
            }
        }
        false
    }

    /// Check for circular dependency. Stores error in `last_error` and returns
    /// `true` if a cycle is detected. Outputs the resolution path in order.
    fn check_cycle(&self, entry: &ServiceEntry) -> bool {
        let resolving = self.resolving.read().unwrap();
        if resolving.contains(&entry.cache_key) {
            let type_name = self.get_type_name(entry.cache_key);
            let cycle_desc = if resolving.is_empty() {
                type_name
            } else {
                let chain: Vec<String> =
                    resolving.iter().map(|&ck| self.get_type_name(ck)).collect();
                format!("{} -> {}", chain.join(" -> "), type_name)
            };
            *self.last_error.lock().unwrap() = Some(RdiError::CircularDependency(cycle_desc));
            return true;
        }
        false
    }

    /// Push entry onto resolution stack, execute factory, pop stack.
    /// Returns `None` if a validation error was detected.
    fn resolve_via_factory(&self, entry: &ServiceEntry) -> Option<Arc<dyn Any + Send + Sync>> {
        // Captive check first.
        if self.check_captive_dependency(entry) {
            return None;
        }
        // Cycle check.
        if self.check_cycle(entry) {
            return None;
        }

        self.resolving.write().unwrap().push(entry.cache_key);
        self.stack
            .write()
            .unwrap()
            .push((entry.cache_key, entry.lifetime));

        // Keep the runtime resolution stack in sync so a factory that resolves
        // further services through an injected provider participates in cycle
        // detection instead of recursing indefinitely.
        let resolve_guard = match crate::provider::enter_resolve(entry.cache_key, entry.type_name) {
            Ok(guard) => guard,
            Err(err) => {
                self.stack.write().unwrap().pop();
                self.resolving.write().unwrap().pop();
                *self.last_error.lock().unwrap() = Some(err);
                return None;
            }
        };

        let result = {
            let _build_guard = crate::provider::BuildFactoryGuard::enter();
            (entry.factory)(self as &dyn IServiceResolver)
        };
        drop(resolve_guard);

        self.stack.write().unwrap().pop();
        self.resolving.write().unwrap().pop();

        // Only cache Singleton results.
        if entry.lifetime == ServiceLifetime::Singleton {
            self.cache.prefill(entry.cache_key, result.clone());
        }

        Some(result)
    }

    /// Resolve an `AnyKey` fallback entry for a concrete requested key.
    ///
    /// Captive-dependency checks use the validation stack; re-entrancy is
    /// tracked per `(entry, requested key)` so nested resolution of a different
    /// key is legitimate.
    fn resolve_any_keyed(
        &self,
        entry: &ServiceEntry,
        requested: &ServiceKey,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        if self.reject_async_dependency(entry) {
            return None;
        }
        if self.check_captive_dependency(entry) {
            return None;
        }
        // A sibling consumer may already have initialized this requested-key
        // singleton during the same validation pass: reuse it (single
        // initialization, identity preserved) instead of constructing another.
        if entry.lifetime == ServiceLifetime::Singleton {
            if let Some(cached) = self
                .provider
                .cached_any_singleton(entry.cache_key, requested)
            {
                return Some(cached);
            }
        }
        let token = crate::provider::ResolveToken::AnyKey(entry.cache_key, requested.stable_hash());
        let resolve_guard = match crate::provider::enter_resolve_token(token, entry.type_name) {
            Ok(guard) => guard,
            Err(err) => {
                *self.last_error.lock().unwrap() = Some(err);
                return None;
            }
        };
        let factory = entry.keyed_factory.as_ref()?;
        let result = {
            let _build_guard = crate::provider::BuildFactoryGuard::enter();
            (factory)(self as &dyn IServiceResolver, requested)
        };
        drop(resolve_guard);

        // A singleton fallback consumed during validation must be cached so the
        // runtime picks up the same instance (single initialization, identity).
        if entry.lifetime == ServiceLifetime::Singleton {
            self.provider
                .prefill_any_keyed_singleton(entry.cache_key, requested, result.clone());
        }
        Some(result)
    }

    /// Resolve an entry, consulting the cache first (singletons built earlier in
    /// the validation pass); `AnyKey` fallbacks are resolved for `requested`.
    fn resolve_entry_for(
        &self,
        entry: &ServiceEntry,
        requested: Option<&ServiceKey>,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        if entry.is_async() {
            self.reject_async_dependency(entry);
            return None;
        }
        if entry.is_any_key() {
            return match requested {
                Some(key) => self.resolve_any_keyed(entry, key),
                None => None,
            };
        }
        if let Some(cached) = self.cache.get(entry.cache_key) {
            return Some(cached);
        }
        self.resolve_via_factory(entry)
    }

    /// Validate exactly `entry` (used by the build-time validation pass).
    ///
    /// Resolving the entry directly (instead of looking it up by type name)
    /// keeps overridden duplicate registrations validated individually and
    /// avoids resolving the *last* registration while validating an earlier one.
    pub(crate) fn validate_entry(
        &self,
        entry: &ServiceEntry,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        self.resolve_entry_for(entry, None)
    }
}

impl IServiceResolver for ValidationResolver<'_> {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(tid) = self.tid_by_name(key) {
            if let Some(entry) = self.unkeyed_entry(tid) {
                return self.resolve_entry_for(entry, None);
            }
        }
        // Built-in services available during validation too: eager singletons
        // may inject `Arc<dyn ScopeFactory>` / `Arc<ServiceProvider>` through
        // the macro string path. Reuse the provider's cached ScopeFactory so the
        // injected instance is identical to `provider.get::<dyn ScopeFactory>()`.
        if key == std::any::type_name::<dyn crate::scope::ScopeFactory>() {
            return self.provider.builtin_scope_factory_any_cached();
        }
        if key == std::any::type_name::<ServiceProvider>() {
            let provider: Arc<ServiceProvider> = Arc::clone(&self.provider);
            return Some(Arc::new(provider) as Arc<dyn Any + Send + Sync>);
        }
        None
    }

    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.tid_by_name(key)?;
        let requested = ServiceKey::from(variant);
        let entry = self.keyed_entry(tid, &requested)?;
        self.resolve_entry_for(entry, Some(&requested))
    }

    /// TypeId 直查：与 [`Self::get_any`] 的常规服务路径一致。
    ///
    /// 必需覆盖：build 期间的 singleton 工厂收到的就是本 resolver，
    /// [`IServiceResolver::get_by_type_id`]（及其 typed 封装
    /// [`IServiceResolverExt::resolve_arc`](crate::IServiceResolverExt::resolve_arc)）
    /// 依赖它；不覆盖时工厂内 TypeId 解析会恒为 `None`。内置服务
    /// （`ScopeFactory`/`ServiceProvider`）仍由 [`Self::get_any`] 的字符串路径兜底。
    fn get_by_type_id(&self, tid: std::any::TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.unkeyed_entry(tid)
            .and_then(|entry| self.resolve_entry_for(entry, None))
    }

    fn get_keyed_any_key(
        &self,
        key: &str,
        variant: &ServiceKey,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.tid_by_name(key)?;
        let entry = self.keyed_entry(tid, variant)?;
        self.resolve_entry_for(entry, Some(variant))
    }

    fn try_get_keyed_any_result_key(
        &self,
        key: &str,
        variant: &ServiceKey,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        Ok(self.get_keyed_any_key(key, variant))
    }

    fn get_keyed_owned_any_key(
        &self,
        key: &str,
        variant: &ServiceKey,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.tid_by_name(key)?;
        let entry = self.keyed_entry(tid, variant)?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return None;
        }
        self.resolve_entry_for(entry, Some(variant))
    }

    fn try_get_keyed_owned_any_key(
        &self,
        key: &str,
        variant: &ServiceKey,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        Ok(self.get_keyed_owned_any_key(key, variant))
    }

    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        let Some(tid) = self.tid_by_name(key) else {
            return Vec::new();
        };
        let Some(entries) = self.store.get(&tid) else {
            return Vec::new();
        };
        entries
            .iter()
            .filter(|e| e.key.is_none())
            .filter_map(|entry| self.resolve_entry_for(entry, None))
            .collect()
    }

    fn get_all_keyed_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        let Some(tid) = self.tid_by_name(key) else {
            return Vec::new();
        };
        let Some(entries) = self.store.get(&tid) else {
            return Vec::new();
        };
        entries
            .iter()
            .filter(|e| e.key.is_some() && !e.is_any_key())
            .filter_map(|entry| self.resolve_entry_for(entry, None))
            .collect()
    }

    fn get_all_any_result(&self, key: &str) -> Result<Vec<Arc<dyn Any + Send + Sync>>, RdiError> {
        let mut out = Vec::new();
        let Some(tid) = self.tid_by_name(key) else {
            return Ok(out);
        };
        let Some(entries) = self.store.get(&tid) else {
            return Ok(out);
        };
        for entry in entries.iter().filter(|e| e.key.is_none()) {
            match self.resolve_entry_for(entry, None) {
                Some(v) => out.push(v),
                None => {
                    return Err(self
                        .take_error()
                        .unwrap_or_else(|| RdiError::not_found(entry.type_name, None)))
                }
            }
        }
        Ok(out)
    }

    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.tid_by_name(key)?;
        let entry = self.unkeyed_entry(tid)?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return None;
        }
        self.resolve_entry_for(entry, None)
    }

    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.tid_by_name(key)?;
        let requested = ServiceKey::from(variant);
        let entry = self.keyed_entry(tid, &requested)?;
        if entry.lifetime == ServiceLifetime::Singleton {
            return None;
        }
        self.resolve_entry_for(entry, Some(&requested))
    }

    /// The real container is available during validation: eager singletons that
    /// declare `#[inject(provider)]` receive it here (this is what makes
    /// `ServiceProvider` / `ScopeFactory` injection work for build-time
    /// singletons).
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        Some(Arc::clone(&self.provider))
    }
}
