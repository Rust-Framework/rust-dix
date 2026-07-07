use crate::entry::IServiceResolver;
use crate::error::RdiError;
use crate::provider::ServiceProvider;
use std::any::Any;
use std::sync::Arc;

/// Trait for services that need explicit cleanup when a scope ends.
///
/// Implement this on scoped services that hold resources requiring
/// deterministic release (e.g., database connections, file handles).
/// The `Scope::dispose()` method calls `dispose()` on all registered
/// disposable services in the scope, and `Drop` on `Scope` calls
/// `dispose()` automatically as a safety net.
///
/// # Example
///
/// ```ignore
/// struct DbConnection { conn: RawConnection }
///
/// impl Disposable for DbConnection {
///     fn dispose(&mut self) {
///         self.conn.close();
///     }
/// }
/// ```
pub trait Disposable: Send + Sync {
    fn dispose(&mut self);
}

/// A scoped service provider — created via [`ServiceProvider::scope`].
///
/// 内部持有一个 scope 范围的 `ServiceProvider`（由 [`ServiceProvider::new_scope`] 派生），
/// 共享 root 的 store/singleton_cache/named，独立 `root_scoped_cache`。
///
/// 所有解析方法委托给内嵌的 `scope_provider`，确保：
/// - Scoped 服务在 scope 内缓存复用（命中 `scope_provider.root_scoped_cache`）。
/// - Transient 依赖通过 `scope_provider` 解析，正确绑定到当前 scope（MEDI 语义）。
/// - `#[inject(provider)]` 字段拿到的是 scope 范围的 provider，经该字段解析 Scoped
///   服务命中 scope 缓存——不泄漏到 root。
///
/// # 资源释放
///
/// 当 `Scope` 被 drop 时，其内部的 `scope_provider` 被释放，scoped 缓存中的
/// `Arc<T>` 实例随之释放。若服务实现了 [`Disposable`] trait，可调用
/// [`Scope::dispose`] 在 scope 结束前执行显式清理。
pub struct Scope {
    scope_provider: Arc<ServiceProvider>,
}

impl Scope {
    pub(crate) fn new(parent: Arc<ServiceProvider>) -> Self {
        Self {
            scope_provider: ServiceProvider::new_scope(&parent),
        }
    }

    /// Returns the scope-local `ServiceProvider` backing this scope.
    ///
    /// Exposed for advanced use cases (e.g. passing the scope provider to
    /// `ServiceProviderWrapper`). Most callers should use [`Self::get`] /
    /// [`Self::get_owned`] directly.
    pub fn scope_provider(&self) -> &Arc<ServiceProvider> {
        &self.scope_provider
    }

    /// Explicitly dispose all scoped services that implement [`Disposable`].
    ///
    /// Iterates through the scope's scoped cache and calls `dispose()` on
    /// each entry that can be downcast to a concrete type implementing
    /// `Disposable`. After calling this method, the scope is still usable
    /// but the disposable services may be in a released state.
    ///
    /// Called automatically on `Drop`. Call this explicitly when you need
    /// deterministic cleanup before the scope variable goes out of scope.
    pub fn dispose(&self) {
        self.scope_provider.dispose_scoped();
    }

    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get::<T>()
    }

    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.scope_provider.get_optional::<T>()
    }

    /// Alias for `get_optional()` with MEDI-inspired naming.
    pub fn get_service<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.scope_provider.get_service::<T>()
    }

    /// Alias for `get()` with MEDI-inspired naming.
    pub fn get_required_service<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.scope_provider.get_required_service::<T>()
    }

    /// Return all registered instances of the given type.
    /// MEDI-inspired naming.
    pub fn get_services<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.scope_provider.get_services::<T>()
    }

    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get_keyed::<T>(key)
    }

    /// Resolve an async service (transient or scoped) within this scope.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get_async::<T>().await
    }

    /// Resolve an async keyed service within this scope.
    pub async fn get_keyed_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get_keyed_async::<T>(key).await
    }

    /// Resolve owned `T` within this scope (bypass cache, fresh each call).
    /// Returns `Err` if not registered or the service is Singleton.
    pub fn get_owned<T: Send + Sync + 'static>(&self) -> Result<T, RdiError> {
        self.scope_provider.get_owned::<T>()
    }

    /// Resolve owned `T`, returning `None` if not registered or Singleton.
    pub fn try_get_owned<T: Send + Sync + 'static>(&self) -> Option<T> {
        self.scope_provider.try_get_owned::<T>()
    }

    /// Resolve owned keyed `T`.
    /// Returns `Err` if not found or Singleton.
    pub fn get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Result<T, RdiError> {
        self.scope_provider.get_keyed_owned::<T>(key)
    }

    /// Resolve owned keyed `T`, returning `None` if not found or Singleton.
    pub fn try_get_keyed_owned<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        self.scope_provider.try_get_keyed_owned::<T>(key)
    }

    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.scope_provider.get_all::<T>()
    }

    pub fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_named_any(name)
    }

    /// Register a named service (for `impl_service_locator!` macro).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.scope_provider.rdi_register_named(name, service);
    }

    /// Remove a named service (for `impl_service_locator!` macro).
    pub fn rdi_remove_named(&self, name: &str) {
        self.scope_provider.rdi_remove_named(name);
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl IServiceResolver for Scope {
    fn get_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_any(key)
    }
    fn get_keyed_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_keyed_any(key, variant)
    }
    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_all_any(key)
    }
    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_owned_any(key)
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_keyed_owned_any(key, variant)
    }
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        // 返回 scope 范围的 provider（不是 root），符合 MEDI 语义：
        // 在 scope 内解析的服务经 #[inject(provider)] 拿到此 provider，
        // 后续 get::<ScopedX>() 命中 scope 自己的 root_scoped_cache。
        Some(Arc::clone(&self.scope_provider))
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
        let a = s1.get::<Sd>().unwrap();
        let b = s1.get::<Sd>().unwrap();
        assert_eq!(a.0, b.0);
        let s2 = p.scope();
        let c = s2.get::<Sd>().unwrap();
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
        let o1: Arc<OuterTransient> = scope1.get().unwrap();
        assert_eq!(o1.mid.core.0, 0, "scope1 的 ScopedCore 应为序号 0");

        // scope1 内再次解析：OuterTransient/MidTransient 每次新建（Transient），
        // 但 ScopedCore 应复用 scope1.scoped_cache（序号仍 0）。
        let o1b: Arc<OuterTransient> = scope1.get().unwrap();
        assert_eq!(o1b.mid.core.0, 0, "scope1 应复用同一 ScopedCore");
        assert!(
            Arc::ptr_eq(&o1.mid.core, &o1b.mid.core),
            "scope1 内 ScopedCore 必须复用"
        );

        // scope2：独立子 scope，ScopedCore 工厂再次执行（序号 1）。
        let scope2 = provider.scope();
        let o2: Arc<OuterTransient> = scope2.get().unwrap();
        assert_eq!(o2.mid.core.0, 1, "scope2 的 ScopedCore 应为序号 1");
        assert!(
            !Arc::ptr_eq(&o1.mid.core, &o2.mid.core),
            "不同 scope 的 ScopedCore 必须独立"
        );

        // 根 scope：第三个独立实例（序号 2），证明子 scope 未泄漏到根。
        let root_c: Arc<ScopedCore> = provider.get().unwrap();
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
