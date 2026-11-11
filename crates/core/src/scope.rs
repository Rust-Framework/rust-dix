use crate::cache::AnyService;
use crate::entry::IServiceResolver;
use crate::error::RdiError;
use crate::provider::ServiceProvider;
use std::any::{Any, TypeId};
use std::sync::{Arc, Weak};

/// 工厂 trait：创建独立 scope。可注入 singleton 服务按需创建 scope。
///
/// 典型场景：后台 worker singleton 需要按请求创建独立 scope，
/// 在 scope 内解析 Scoped 服务（如 request-scoped DbContext）。
///
/// built provider 内置此服务：`provider.get::<dyn ScopeFactory>()` 以及
/// `#[inject] Arc<dyn ScopeFactory>` 字段都可用，无需手动注册，eager
/// singleton 与懒解析服务一致。显式 `singleton::<dyn ScopeFactory>(...)`
/// 注册仍然优先（覆盖内置实现）。
///
/// 内置实现持有 provider 的 [`Weak`] 引用，因此 singleton 持有它不会形成
/// `容器 → singleton → 容器` 的 `Arc` 环；容器必须先于其创建的服务存活
/// （`create_scope` 在 provider 已释放时会 panic，与 MEDI 的容器生命周期
/// 约定一致）。若需要强引用语义，可自行注册实现。
pub trait ScopeFactory: Send + Sync {
    /// 创建一个独立 scope，共享 root 的 store/singleton_cache/named，
    /// 独立 scoped 缓存和 disposables。
    fn create_scope(&self) -> Scope;
}

/// 内置 [`ScopeFactory`] 实现：按需 upgrade provider 的弱引用。
///
/// 使用 `Weak` 而非 `Arc` 是有意设计：singleton 会持有该工厂，强引用会形成
/// `ServiceProvider::singleton_cache → singleton → ServiceProvider` 的引用环，
/// 导致容器在外部句柄释放后仍无法回收。
pub(crate) struct BuiltinScopeFactory {
    provider: Weak<ServiceProvider>,
}

impl BuiltinScopeFactory {
    pub(crate) fn new(provider: Weak<ServiceProvider>) -> Self {
        Self { provider }
    }
}

impl ScopeFactory for BuiltinScopeFactory {
    fn create_scope(&self) -> Scope {
        self.provider.upgrade().map(Scope::new).expect(
            "ScopeFactory::create_scope: ServiceProvider has been dropped; \
                 the DI container must outlive services created from it",
        )
    }
}

/// Create the built-in `dyn ScopeFactory` singleton for a provider.
pub(crate) fn builtin_scope_factory(provider: Weak<ServiceProvider>) -> Arc<dyn ScopeFactory> {
    Arc::new(BuiltinScopeFactory::new(provider))
}

/// Wrap a scope factory in the `Arc<Arc<T>>`-style storage format used by
/// [`IServiceResolver`] (`Arc::new(Arc<dyn T>)`).
pub(crate) fn builtin_scope_factory_any(factory: Arc<dyn ScopeFactory>) -> AnyService {
    Arc::new(factory) as AnyService
}

/// A scoped service provider — created via [`ServiceProvider::scope`].
///
/// 内部持有一个 scope 范围的 `ServiceProvider`（由 `ServiceProvider::new_scope` 派生），
/// 共享 root 的 store/singleton_cache/named，独立 scoped 缓存。
///
/// 所有解析方法委托给内嵌的 `scope_provider`，确保：
/// - Scoped 服务在 scope 内缓存复用。
/// - Transient 依赖通过 `scope_provider` 解析，正确绑定到当前 scope（MEDI 语义）。
/// - `#[inject(provider)]` 字段拿到的是 scope 范围的 provider，经该字段解析 Scoped
///   服务命中 scope 缓存——不泄漏到 root。
///
/// # 资源释放
///
/// [`Scope::dispose`]（以及 `Drop` 安全网）会按 **创建逆序** 释放该 scope 创建的
/// Scoped 实例（实例的 `Drop` 在最后一个引用释放时运行），并按注册逆序执行
/// [`ServiceProvider::register_disposable`] 注册的清理闭包。释放后 scope 解析返回
/// [`RdiError::ScopeDisposed`]。
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

    /// Dispose the scope: release scoped instances in reverse creation order,
    /// then run `register_disposable` cleanups in reverse registration order.
    ///
    /// Idempotent; also called by `Drop`. After disposal, resolution through
    /// this scope returns [`RdiError::ScopeDisposed`] (or `None`/empty for
    /// infallible lookups).
    pub fn dispose(&self) {
        self.scope_provider.dispose();
    }

    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get::<T>()
    }

    pub fn get_optional<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.scope_provider.get_optional::<T>()
    }

    pub fn get_keyed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Arc<T>, RdiError> {
        self.scope_provider.get_keyed::<T>(key)
    }

    /// Resolve an async service (transient or scoped) within this scope.
    pub async fn get_async<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, RdiError> {
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
    /// Return all **unkeyed** registered instances (MEDI `GetServices<T>()`).
    pub fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.scope_provider.get_all::<T>()
    }

    /// Return all **keyed** registered instances (MEDI `GetKeyedServices<T>()`).
    pub fn get_all_keyed<T: ?Sized + Send + Sync + 'static>(&self) -> Vec<Arc<T>> {
        self.scope_provider.get_all_keyed::<T>()
    }

    /// Whether `T` has a resolvable registration (MEDI `IServiceProviderIsService`).
    pub fn is_registered<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.scope_provider.is_registered::<T>()
    }

    /// Whether `T` has a registration under `key`.
    pub fn is_registered_keyed<T: ?Sized + Send + Sync + 'static>(&self, key: &str) -> bool {
        self.scope_provider.is_registered_keyed::<T>(key)
    }

    pub fn get_named_any(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_named_any(name)
    }

    /// Register a named service (for [`IProvider`](crate::bridge::IProvider) trait).
    pub fn rdi_register_named(&self, name: &str, service: Arc<dyn Any + Send + Sync>) {
        self.scope_provider.rdi_register_named(name, service);
    }

    /// Remove a named service (for [`IProvider`](crate::bridge::IProvider) trait).
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
    fn get_by_type_id(&self, tid: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_by_type_id(tid)
    }
    fn get_keyed_by_type_id(&self, tid: TypeId, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_keyed_by_type_id(tid, key)
    }
    fn get_all_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_all_any(key)
    }
    fn get_all_keyed_any(&self, key: &str) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_all_keyed_any(key)
    }
    fn get_all_any_result(&self, key: &str) -> Result<Vec<Arc<dyn Any + Send + Sync>>, RdiError> {
        self.scope_provider.get_all_any_result(key)
    }
    fn get_owned_any(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_owned_any(key)
    }
    fn try_get_any_result(
        &self,
        key: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        self.scope_provider.try_get_any_result(key)
    }
    fn try_get_keyed_any_result(
        &self,
        key: &str,
        variant: &str,
    ) -> Result<Option<Arc<dyn Any + Send + Sync>>, RdiError> {
        self.scope_provider.try_get_keyed_any_result(key, variant)
    }
    fn get_keyed_owned_any(&self, key: &str, variant: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.scope_provider.get_keyed_owned_any(key, variant)
    }
    fn provider_arc(&self) -> Option<Arc<ServiceProvider>> {
        // 返回 scope 范围的 provider（不是 root），符合 MEDI 语义：
        // 在 scope 内解析的服务经 #[inject(provider)] 拿到此 provider，
        // 后续 get::<ScopedX>() 命中 scope 自己的 scoped 缓存。
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
        let p = ServiceCollection::new()
            .scoped(|_| Arc::new(Sd(NXT.fetch_add(1, Ordering::SeqCst))))
            .build()
            .unwrap();
        let s1 = p.scope();
        let a = s1.get::<Sd>().unwrap();
        let b = s1.get::<Sd>().unwrap();
        assert_eq!(a.0, b.0);
        let s2 = p.scope();
        let c = s2.get::<Sd>().unwrap();
        assert_ne!(a.0, c.0);
    }

    #[test]
    fn disposed_scope_rejects_resolution() {
        let p = ServiceCollection::new()
            .scoped(|_| Arc::new(Sd(1)))
            .build()
            .unwrap();
        let scope = p.scope();
        assert!(scope.get::<Sd>().is_ok());
        scope.dispose();
        assert!(matches!(
            scope.get::<Sd>(),
            Err(crate::error::RdiError::ScopeDisposed)
        ));
        assert!(scope.get_optional::<Sd>().is_none());
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

        let provider = ServiceCollection::new()
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
            .unwrap();

        // scope1：首次解析 OuterTransient，ScopedCore 工厂执行（序号 0）。
        let scope1 = provider.scope();
        let o1: Arc<OuterTransient> = scope1.get().unwrap();
        assert_eq!(o1.mid.core.0, 0, "scope1 的 ScopedCore 应为序号 0");

        // scope1 内再次解析：OuterTransient/MidTransient 每次新建（Transient），
        // 但 ScopedCore 应复用 scope1 的 scoped 缓存（序号仍 0）。
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

        // 严格 scope 校验（0.8 默认）：root 不允许解析 Scoped 服务。
        assert!(matches!(
            provider.get::<ScopedCore>(),
            Err(crate::error::RdiError::ScopedFromRootProvider { .. })
        ));

        // 全程工厂只执行 2 次（scope1、scope2 各一次）。
        assert_eq!(
            CORE_CALLS.load(Ordering::SeqCst),
            2,
            "ScopedCore 工厂应执行 2 次（两个子 scope 各一次）"
        );
    }
}
