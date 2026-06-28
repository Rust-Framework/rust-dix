use crate::entry::{IServiceResolver, ServiceEntry, ServiceFactory};
use crate::error::RdiError;
use crate::lifetime::ServiceLifetime;
use crate::store::ServiceStore;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Validation resolver that tracks resolution chains for cycle detection.
struct ValidationResolver<'a> {
    store: &'a ServiceStore,
    /// Cache keys currently being resolved (for cycle detection).
    /// Uses `RwLock` because `IServiceResolver::get_any` takes `&self`
    /// and the trait requires `Send + Sync`.
    resolving: RwLock<HashSet<usize>>,
    /// TypeId → type_name map for error messages
    type_names: &'a HashMap<TypeId, &'static str>,
    /// Reusable instance cache — populated as factories execute during
    /// validation so that Phase 2 (eager singleton init) can reuse the
    /// results instead of re-executing factories (which would double-fire
    /// side effects like counters).
    cache: &'a RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
}

impl<'a> ValidationResolver<'a> {
    fn new(
        store: &'a ServiceStore,
        type_names: &'a HashMap<TypeId, &'static str>,
        cache: &'a RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
    ) -> Self {
        Self {
            store,
            resolving: RwLock::new(HashSet::new()),
            type_names,
            cache,
        }
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
}

impl<'a> IServiceResolver for ValidationResolver<'a> {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        // Find the entry by type_name
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self.store.get(tid)?.iter().find(|e| e.key.is_none())?;

        // Reuse cached instance if a prior validation pass already built it.
        if let Some(cached) = self.cache.read().unwrap().get(&entry.cache_key) {
            return Some(cached.clone());
        }

        // Check for cycle: if already being resolved, we have a cycle
        if self.resolving.read().unwrap().contains(&entry.cache_key) {
            let type_name = self.get_type_name(entry.cache_key);
            let chain: Vec<String> = self.resolving
                .read()
                .unwrap()
                .iter()
                .map(|&ck| self.get_type_name(ck))
                .collect();
            let cycle_desc = if chain.is_empty() {
                type_name.clone()
            } else {
                format!("{} -> {}", chain.join(" -> "), type_name)
            };
            // Use a panic with message that will be caught
            panic!("CIRCULAR_DEPENDENCY: {}", cycle_desc);
        }

        // Mark as resolving
        self.resolving.write().unwrap().insert(entry.cache_key);

        // Execute the factory (this may recursively resolve dependencies)
        // We catch panics to convert them to RdiError
        let result = (entry.factory)(self as &dyn IServiceResolver);

        // Mark as resolved
        self.resolving.write().unwrap().remove(&entry.cache_key);

        // Cache the instance so subsequent resolutions (including Phase 2
        // eager init) reuse it instead of re-executing the factory.
        self.cache
            .write()
            .unwrap()
            .insert(entry.cache_key, result.clone());

        Some(result)
    }

    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        // Find the entry by type_name + variant
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self
            .store
            .get(tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(variant))?;

        // Reuse cached instance if available.
        if let Some(cached) = self.cache.read().unwrap().get(&entry.cache_key) {
            return Some(cached.clone());
        }

        // Check for cycle
        if self.resolving.read().unwrap().contains(&entry.cache_key) {
            let type_name = self.get_type_name(entry.cache_key);
            let chain: Vec<String> = self.resolving
                .read()
                .unwrap()
                .iter()
                .map(|&ck| self.get_type_name(ck))
                .collect();
            let cycle_desc = if chain.is_empty() {
                type_name.clone()
            } else {
                format!("{} -> {}", chain.join(" -> "), type_name)
            };
            panic!("CIRCULAR_DEPENDENCY: {}", cycle_desc);
        }

        self.resolving.write().unwrap().insert(entry.cache_key);
        let result = (entry.factory)(self as &dyn IServiceResolver);
        self.resolving.write().unwrap().remove(&entry.cache_key);

        self.cache
            .write()
            .unwrap()
            .insert(entry.cache_key, result.clone());

        Some(result)
    }
}

pub struct ServiceProvider {
    store: ServiceStore,
    /// String → TypeId lookup for IServiceLocator string-based resolution.
    type_map: HashMap<&'static str, TypeId>,
    /// Eager-executed singleton cache. Indexed by cache_key.
    /// Non-singleton entries are not present.
    /// Uses RwLock for interior mutability: during eager initialization,
    /// a singleton factory may reference another singleton not yet populated.
    /// The lazy fallback in get_any_by_entry handles this by executing the
    /// factory on-demand and caching the result.
    singleton_cache: RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
    /// String-keyed registry for cross-DLL (cdylib) service access.
    /// Rust's `TypeId` differs across compilation units, so named
    /// lookup is the only reliable mechanism for plugin services.
    pub(crate) named: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
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
        let validation_cache: RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>> =
            RwLock::new(HashMap::new());
        Self::validate_dependencies(&store, &validation_cache)?;

        // Phase 1: collect all singleton entries.
        let singleton_entries: Vec<(usize, ServiceFactory)> = store
            .values()
            .flat_map(|entries| entries.iter())
            .filter(|e| e.lifetime == ServiceLifetime::Singleton)
            .map(|e| (e.cache_key, e.factory.clone()))
            .collect();

        // Phase 2: eagerly execute all singleton factories.
        // Reuse instances already built during validation; only execute
        // factories whose results are not yet cached.
        let sp = Self {
            store,
            type_map,
            singleton_cache: validation_cache,
            named: RwLock::new(HashMap::new()),
        };

        for (ck, factory) in &singleton_entries {
            // Skip if validation already built this instance.
            if sp.singleton_cache.read().unwrap().contains_key(ck) {
                continue;
            }
            let instance = (factory)(&sp as &dyn IServiceResolver);
            sp.singleton_cache.write().unwrap().insert(*ck, instance);
        }

        Ok(sp)
    }

    /// Validate all dependencies: detect cycles.
    /// Instances built during validation are stored in `cache` so they can
    /// be reused by the subsequent eager-init phase.
    fn validate_dependencies(
        store: &ServiceStore,
        cache: &RwLock<HashMap<usize, Arc<dyn Any + Send + Sync>>>,
    ) -> Result<(), RdiError> {
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
                if entry.lifetime == ServiceLifetime::Singleton {
                    let resolver = ValidationResolver::new(store, &type_names, cache);
                    let type_name = entry.type_name;
                    // Attempt to resolve - if there's a cycle, we'll catch the panic
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        resolver.get_any(type_name)
                    })) {
                        Ok(_) => {}
                        Err(panic_info) => {
                            // Check if it's our circular dependency panic
                            if let Some(msg) = panic_info.downcast_ref::<String>() {
                                if msg.starts_with("CIRCULAR_DEPENDENCY: ") {
                                    let cycle = msg.strip_prefix("CIRCULAR_DEPENDENCY: ").unwrap();
                                    return Err(RdiError::CircularDependency(cycle.to_string()));
                                }
                            }
                            if let Some(msg) = panic_info.downcast_ref::<&str>() {
                                if msg.starts_with("CIRCULAR_DEPENDENCY: ") {
                                    let cycle = msg.strip_prefix("CIRCULAR_DEPENDENCY: ").unwrap();
                                    return Err(RdiError::CircularDependency(cycle.to_string()));
                                }
                            }
                            // Other panic - re-panic
                            std::panic::panic_any(panic_info);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Resolve a service by type. Works uniformly for concrete types and trait objects.
    /// Panics if not registered.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.try_get::<T>()
            .unwrap_or_else(|| panic!("service not registered: {}", std::any::type_name::<T>()))
    }

    /// Resolve a service by type, returning `None` if not registered.
    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.try_get::<T>()
    }

    /// Resolve a keyed service by type and key. Panics if not found.
    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Arc<T> {
        self.try_get_keyed::<T>(key).unwrap_or_else(|| {
            panic!(
                "keyed service not registered: {}:{}",
                std::any::type_name::<T>(),
                key
            )
        })
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
        self.get::<T>()
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

    pub(crate) fn get_any_by_entry(
        &self,
        entry: &ServiceEntry,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        match entry.lifetime {
            ServiceLifetime::Singleton => {
                // Check eager cache (populated at build time).
                // If not found (e.g. cross-reference during eager init), execute
                // the factory on-demand as a lazy fallback.
                {
                    let cache = self.singleton_cache.read().unwrap();
                    if let Some(instance) = cache.get(&entry.cache_key) {
                        return Some(instance.clone());
                    }
                }
                // Lazy fallback: execute factory on-demand and cache.
                let instance = (entry.factory)(self);
                self.singleton_cache
                    .write()
                    .unwrap()
                    .insert(entry.cache_key, instance.clone());
                Some(instance)
            }
            ServiceLifetime::Transient | ServiceLifetime::Scoped => Some((entry.factory)(self)),
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

    /// Get entries by TypeId (used internally and by Scope).
    pub(crate) fn entries_by_tid(&self, tid: &TypeId) -> Option<&Vec<ServiceEntry>> {
        self.store.get(tid)
    }

    /// Find entry by string type_name + variant (for string-based resolution).
    pub(crate) fn entry_by_str(&self, type_name: &str, variant: &str) -> Option<&ServiceEntry> {
        let tid = self.type_map.get(type_name)?;
        self.store
            .get(tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(variant))
    }

    /// Get entries by string type_name (for string-based resolution).
    pub(crate) fn entries_by_str(&self, type_name: &str) -> Option<&Vec<ServiceEntry>> {
        let tid = self.type_map.get(type_name)?;
        self.store.get(tid)
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
