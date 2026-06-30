use crate::cache::{AnyService, LazyCache};
use crate::entry::IServiceResolver;
use crate::lifetime::ServiceLifetime;
use crate::provider::ServiceProvider;
use std::any::{Any, TypeId};
use std::sync::Arc;

pub struct Scope {
    parent: Arc<ServiceProvider>,
    /// 子 scope 的 Scoped 实例缓存（per-key `OnceLock`，多线程下工厂只执行一次）。
    /// 与 ServiceProvider 的 root_scoped_cache 结构一致，统一懒初始化架构。
    scoped_cache: LazyCache,
}

impl Scope {
    pub(crate) fn new(parent: Arc<ServiceProvider>) -> Self {
        Self {
            parent,
            scoped_cache: LazyCache::new(),
        }
    }

    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.try_get::<T>()
            .unwrap_or_else(|| panic!("service not registered: {}", std::any::type_name::<T>()))
    }

    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.try_get::<T>()
    }

    /// Alias for `get_optional()` with MEDI-inspired naming.
    pub fn get_service<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.get_optional::<T>()
    }

    /// Alias for `get()` with MEDI-inspired naming.
    pub fn get_required_service<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.get::<T>()
    }

    /// Return all registered instances of the given type.
    /// MEDI-inspired naming.
    pub fn get_services<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.get_all::<T>()
    }

    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Arc<T> {
        self.try_get_keyed::<T>(key).unwrap_or_else(|| {
            panic!(
                "keyed service not registered: {}:{}",
                std::any::type_name::<T>(),
                key
            )
        })
    }

    /// Resolve owned `T` within this scope (bypass cache, fresh each call).
    /// Panics if not registered or the service is Singleton.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> T {
        self.try_get_owned::<T>().unwrap_or_else(|| {
            panic!(
                "get_owned: not registered or Singleton: {}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        let tid = TypeId::of::<T>();
        let entry = self
            .parent
            .entries_by_tid(&tid)?
            .iter()
            .find(|e| e.key.is_none())?;
        let arc_any = self.create_owned_any_by_entry(entry)?;
        let arc_t = ServiceProvider::extract(arc_any)?;
        Arc::try_unwrap(arc_t).ok()
    }

    /// Resolve owned keyed `T`. Panics if not found or Singleton.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> T {
        self.try_get_keyed_owned::<T>(key).unwrap_or_else(|| {
            panic!(
                "get_keyed_owned: not registered or Singleton: {}:{}",
                std::any::type_name::<T>(),
                key
            )
        })
    }

    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        let tid = TypeId::of::<T>();
        let entries = self.parent.entries_by_tid(&tid)?;
        let entry = entries.iter().find(|e| e.key.as_deref() == Some(key))?;
        let arc_any = self.create_owned_any_by_entry(entry)?;
        let arc_t = ServiceProvider::extract(arc_any)?;
        Arc::try_unwrap(arc_t).ok()
    }

    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        let tid = TypeId::of::<T>();
        if let Some(entries) = self.parent.entries_by_tid(&tid) {
            entries
                .iter()
                .filter_map(|e| {
                    let arc = self.get_any_by_entry(e)?;
                    ServiceProvider::extract(arc)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.parent.get_named_any(name)
    }

    fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let tid = TypeId::of::<T>();
        let entry = self
            .parent
            .entries_by_tid(&tid)?
            .iter()
            .find(|e| e.key.is_none())?;
        let arc = self.get_any_by_entry(entry)?;
        ServiceProvider::extract(arc)
    }

    fn try_get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let tid = TypeId::of::<T>();
        let entries = self.parent.entries_by_tid(&tid)?;
        let entry = entries.iter().find(|e| e.key.as_deref() == Some(key))?;
        let arc = self.get_any_by_entry(entry)?;
        ServiceProvider::extract(arc)
    }

    fn get_any_by_entry(&self, entry: &crate::entry::ServiceEntry) -> Option<AnyService> {
        match entry.lifetime {
            ServiceLifetime::Singleton => {
                // Singleton cache lives in parent's eager cache
                self.parent.get_any_by_entry(entry)
            }
            ServiceLifetime::Transient => {
                // Transient 工厂用 `self`（Scope）解析依赖：
                // 若 Transient 依赖 Scoped，依赖在当前子 scope 内解析（MEDI 语义），
                // 而非错误地回退到根 root_scoped_cache。
                Some((entry.factory)(self))
            }
            ServiceLifetime::Scoped => {
                // 子 scope 独立缓存：per-key OnceLock 保证多线程下工厂只执行一次，
                // 不回退到根 root_scoped_cache（MEDI 子 scope 隔离语义）。
                let factory = entry.factory.clone();
                let resolver = self as &dyn IServiceResolver;
                Some(
                    self.scoped_cache
                        .get_or_init_with(entry.cache_key, move || factory(resolver)),
                )
            }
        }
    }

    /// Bypass cache and invoke the factory fresh (owned resolution only).
    ///
    /// - Singleton → `None` (shared singleton cannot be owned).
    /// - Scoped/Transient → invoke factory directly without touching
    ///   `scoped_cache`. The factory uses `self` (Scope) as resolver, so
    ///   Scoped deps resolved transitively bind to this child scope (not the
    ///   root), consistent with the Transient-in-child-scope fix.
    fn create_owned_any_by_entry(&self, entry: &crate::entry::ServiceEntry) -> Option<AnyService> {
        match entry.lifetime {
            ServiceLifetime::Singleton => None,
            ServiceLifetime::Scoped | ServiceLifetime::Transient => {
                Some((entry.factory)(self as &dyn IServiceResolver))
            }
        }
    }
}

impl IServiceResolver for Scope {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(entries) = self.parent.entries_by_str(key) {
            for entry in entries {
                if entry.key.is_none() {
                    if let Some(r) = self.get_any_by_entry(entry) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.parent.entry_by_str(key, variant)?;
        self.get_any_by_entry(entry)
    }
    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        if let Some(entries) = self.parent.entries_by_str(key) {
            for entry in entries {
                if entry.key.is_none() {
                    if let Some(r) = self.create_owned_any_by_entry(entry) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let entry = self.parent.entry_by_str(key, variant)?;
        self.create_owned_any_by_entry(entry)
    }
}

impl Scope {
    /// Register a named service (for `impl_service_locator!` macro).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.parent.rdi_register_named(name, service);
    }

    /// Remove a named service (for `impl_service_locator!` macro).
    pub fn rdi_remove_named(&self, name: &str) {
        self.parent.rdi_remove_named(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::ServiceCollection;
    use std::sync::atomic::{AtomicU64, Ordering};
    #[derive(Debug, PartialEq)]
    struct Sd(u64);
    #[test]
    fn scoped_cached_per_scope() {
        static NXT: AtomicU64 = AtomicU64::new(0);
        let p = Arc::new(
            ServiceCollection::new()
                .scoped(|_| Arc::new(Sd(NXT.fetch_add(1, Ordering::SeqCst))))
                .build()
                .unwrap(),
        );
        let s1 = p.scope();
        let a = s1.get::<Sd>();
        let b = s1.get::<Sd>();
        assert_eq!(a.0, b.0);
        let s2 = p.scope();
        let c = s2.get::<Sd>();
        assert_ne!(a.0, c.0);
    }

    #[test]
    fn nested_transient_chain_scoped_binds_to_child_scope() {
        // 边界场景：双层嵌套 Transient 依赖链 → Scoped
        //   OuterTransient → MidTransient → ScopedCore
        // 验证最内层 ScopedCore 在子 scope 内解析（scope.scoped_cache），
        // 不回退到根 root_scoped_cache。
        // 严格性：任一层 Transient 若错误用 self.parent 解析依赖，
        // ScopedCore 都会泄漏到根，三 scope 序号会塌缩为相同值。
        static CORE_CALLS: AtomicU64 = AtomicU64::new(0);

        struct ScopedCore(u64);
        struct MidTransient {
            core: Arc<ScopedCore>,
        }
        struct OuterTransient {
            mid: Arc<MidTransient>,
        }

        let provider = Arc::new(
            ServiceCollection::new()
                .scoped(|_| {
                    let n = CORE_CALLS.fetch_add(1, Ordering::SeqCst);
                    Arc::new(ScopedCore(n))
                })
                .transient(|r| {
                    let any = r.get_any(std::any::type_name::<ScopedCore>()).unwrap();
                    let outer = any.downcast::<Arc<ScopedCore>>().unwrap();
                    let core: Arc<ScopedCore> = Arc::clone(&*outer);
                    Arc::new(MidTransient { core })
                })
                .transient(|r| {
                    let any = r.get_any(std::any::type_name::<MidTransient>()).unwrap();
                    let outer = any.downcast::<Arc<MidTransient>>().unwrap();
                    let mid: Arc<MidTransient> = Arc::clone(&*outer);
                    Arc::new(OuterTransient { mid })
                })
                .build()
                .unwrap(),
        );

        // scope1：首次解析 OuterTransient，ScopedCore 工厂执行（序号 0）。
        let scope1 = provider.scope();
        let o1: Arc<OuterTransient> = scope1.get();
        assert_eq!(o1.mid.core.0, 0, "scope1 的 ScopedCore 应为序号 0");

        // scope1 内再次解析：OuterTransient/MidTransient 每次新建（Transient），
        // 但 ScopedCore 应复用 scope1.scoped_cache（序号仍 0）。
        let o1b: Arc<OuterTransient> = scope1.get();
        assert_eq!(o1b.mid.core.0, 0, "scope1 应复用同一 ScopedCore");
        assert!(
            Arc::ptr_eq(&o1.mid.core, &o1b.mid.core),
            "scope1 内 ScopedCore 必须复用"
        );

        // scope2：独立子 scope，ScopedCore 工厂再次执行（序号 1）。
        let scope2 = provider.scope();
        let o2: Arc<OuterTransient> = scope2.get();
        assert_eq!(o2.mid.core.0, 1, "scope2 的 ScopedCore 应为序号 1");
        assert!(
            !Arc::ptr_eq(&o1.mid.core, &o2.mid.core),
            "不同 scope 的 ScopedCore 必须独立"
        );

        // 根 scope：第三个独立实例（序号 2），证明子 scope 未泄漏到根。
        let root_c: Arc<ScopedCore> = provider.get();
        assert_eq!(root_c.0, 2, "根 scope 的 ScopedCore 应为序号 2");
        assert!(!Arc::ptr_eq(&o1.mid.core, &root_c));
        assert!(!Arc::ptr_eq(&o2.mid.core, &root_c));

        // 全程工厂只执行 3 次（scope1、scope2、根各一次）。
        assert_eq!(
            CORE_CALLS.load(Ordering::SeqCst),
            3,
            "ScopedCore 工厂应执行 3 次（三 scope 各一次）"
        );
    }
}
