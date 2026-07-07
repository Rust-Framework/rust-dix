use crate::cache::LazyCache;
use crate::entry::{IServiceResolver, ServiceEntry, ServiceLifetime, ServiceStore};
use crate::error::RdiError;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

/// Validation resolver that tracks resolution chains for cycle detection
/// and captive dependency detection (Singleton depending on Scoped).
///
/// Both checks run during `build()` (Phase 0) by executing singleton factories
/// through this resolver. Errors are stored in `last_error` instead of using
/// `panic_any`, making the control flow explicit and idiomatic.
///
/// The `catch_unwind` in `validate_dependencies` only catches user factory
/// panics (triggered by `None` returns from this resolver), not framework errors.
pub(crate) struct ValidationResolver<'a> {
    store: &'a ServiceStore,
    /// Cache keys currently being resolved (for cycle detection).
    resolving: RwLock<HashSet<usize>>,
    /// 解析栈：记录当前解析链中每个节点的 (cache_key, lifetime)，
    /// 用于 captive dependency 检测（Singleton 依赖 Scoped）。
    stack: RwLock<Vec<(usize, ServiceLifetime)>>,
    /// TypeId → type_name map for error messages
    type_names: &'a HashMap<TypeId, &'static str>,
    /// Reusable instance cache — populated as factories execute during
    /// validation so that Phase 2 (eager singleton init) can reuse the
    /// results instead of re-executing factories.
    cache: &'a LazyCache,
    /// Stores the first validation error (cycle or captive) encountered.
    /// Cleared by [`Self::take_error`] for consumption by the caller.
    last_error: Mutex<Option<RdiError>>,
}

impl<'a> ValidationResolver<'a> {
    pub(crate) fn new(
        store: &'a ServiceStore,
        type_names: &'a HashMap<TypeId, &'static str>,
        cache: &'a LazyCache,
    ) -> Self {
        Self {
            store,
            resolving: RwLock::new(HashSet::new()),
            stack: RwLock::new(Vec::new()),
            type_names,
            cache,
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
                *self.last_error.lock().unwrap() =
                    Some(RdiError::SingletonDependsOnScoped {
                        singleton: singleton_name,
                        scoped: scoped_name,
                    });
                return true;
            }
        }
        false
    }

    /// Check for circular dependency. Stores error in `last_error` and returns
    /// `true` if a cycle is detected.
    fn check_cycle(&self, entry: &ServiceEntry) -> bool {
        if self.resolving.read().unwrap().contains(&entry.cache_key) {
            let type_name = self.get_type_name(entry.cache_key);
            let chain: Vec<String> = self
                .resolving
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
            *self.last_error.lock().unwrap() = Some(RdiError::CircularDependency(cycle_desc));
            return true;
        }
        false
    }

    /// Push entry onto resolution stack, execute factory, pop stack.
    /// Returns `None` if a validation error was detected.
    fn resolve_via_factory(
        &self,
        entry: &ServiceEntry,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        // Captive check first.
        if self.check_captive_dependency(entry) {
            return None;
        }
        // Cycle check.
        if self.check_cycle(entry) {
            return None;
        }

        self.resolving.write().unwrap().insert(entry.cache_key);
        self.stack
            .write()
            .unwrap()
            .push((entry.cache_key, entry.lifetime));

        let result = (entry.factory)(self as &dyn IServiceResolver);

        self.stack.write().unwrap().pop();
        self.resolving.write().unwrap().remove(&entry.cache_key);

        // Only cache Singleton results.
        if entry.lifetime == ServiceLifetime::Singleton {
            self.cache.prefill(entry.cache_key, result.clone());
        }

        Some(result)
    }
}

impl IServiceResolver for ValidationResolver<'_> {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self.store.get(tid)?.iter().find(|e| e.key.is_none())?;

        if let Some(cached) = self.cache.get(entry.cache_key) {
            return Some(cached);
        }

        self.resolve_via_factory(entry)
    }

    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self
            .store
            .get(tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(variant))?;

        if let Some(cached) = self.cache.get(entry.cache_key) {
            return Some(cached);
        }

        self.resolve_via_factory(entry)
    }

    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self.store.get(tid)?.iter().find(|e| e.key.is_none())?;

        // Captive check first.
        if self.check_captive_dependency(entry) {
            return None;
        }
        // Cycle check.
        if self.check_cycle(entry) {
            return None;
        }

        self.resolving.write().unwrap().insert(entry.cache_key);
        self.stack
            .write()
            .unwrap()
            .push((entry.cache_key, entry.lifetime));
        let result = (entry.factory)(self as &dyn IServiceResolver);
        self.stack.write().unwrap().pop();
        self.resolving.write().unwrap().remove(&entry.cache_key);

        Some(result)
    }

    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let tid = self.type_names.iter().find(|(_, name)| **name == key)?.0;
        let entry = self
            .store
            .get(tid)?
            .iter()
            .find(|e| e.key.as_deref() == Some(variant))?;

        if self.check_captive_dependency(entry) {
            return None;
        }
        if self.check_cycle(entry) {
            return None;
        }

        self.resolving.write().unwrap().insert(entry.cache_key);
        self.stack
            .write()
            .unwrap()
            .push((entry.cache_key, entry.lifetime));
        let result = (entry.factory)(self as &dyn IServiceResolver);
        self.stack.write().unwrap().pop();
        self.resolving.write().unwrap().remove(&entry.cache_key);

        Some(result)
    }
}