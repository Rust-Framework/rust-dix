//! 懒初始化服务缓存。
//!
//! [`LazyCache`] 用 per-key [`OnceLock`] 保证多线程下工厂只执行一次，
//! 且工厂在无全局锁状态下执行，避免递归解析（Scoped 依赖 Scoped）时死锁。
//! 统一用于 [`crate::provider::ServiceProvider`] 的 singleton 缓存与 root scope 缓存。

use futures::lock::Mutex as FuturesMutex;
use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};

/// 类型擦除的服务实例。
pub(crate) type AnyService = Arc<dyn Any + Send + Sync>;

/// 懒初始化缓存。
///
/// 结构：`RwLock<HashMap<cache_key, Arc<OnceLock<AnyService>>>>`
///
/// - **读快速路径**：读锁查 cell，`OnceLock::get` 命中即 clone Arc 返回。
/// - **写慢路径**：cell 不存在时，写锁插入空 `OnceLock`，立即释放写锁，
///   再在无锁状态下 `OnceLock::get_or_init` 执行工厂。
/// - **并发安全**：`OnceLock::get_or_init` 保证多线程下工厂只执行一次，
///   彻底消除 check-then-act 竞态（TOCTOU）。
/// - **无死锁**：工厂执行时不持有任何全局锁，递归解析依赖可安全获取
///   相同 `LazyCache` 的其他 key（不同 cell，无锁竞争）。
pub(crate) struct LazyCache {
    cells: RwLock<HashMap<usize, Arc<OnceLock<AnyService>>>>,
    /// Per-key async factory 执行锁：保证 async factory 只执行一次。
    /// 仅 `get_or_init_with_async` 使用，sync 路径不受影响。
    async_locks: RwLock<HashMap<usize, Arc<FuturesMutex<Option<AnyService>>>>>,
}

impl LazyCache {
    pub fn new() -> Self {
        Self {
            cells: RwLock::new(HashMap::new()),
            async_locks: RwLock::new(HashMap::new()),
        }
    }

    /// 查询已缓存的实例，不执行工厂。
    pub fn get(&self, key: usize) -> Option<AnyService> {
        self.cells
            .read()
            .unwrap()
            .get(&key)
            .and_then(|cell| cell.get().cloned())
    }

    /// 预填充：build 阶段 validation 已构建实例，直接填入。
    /// 若 cell 已初始化则 no-op（validation 单线程，不会冲突）。
    pub fn prefill(&self, key: usize, instance: AnyService) {
        let cell = {
            let mut cells = self.cells.write().unwrap();
            cells
                .entry(key)
                .or_insert_with(|| Arc::new(OnceLock::new()))
                .clone()
        };
        let _ = cell.set(instance);
    }

    /// 懒初始化：若已缓存则返回，否则在无锁状态下执行工厂并缓存。
    ///
    /// 多线程同时 miss 时，`OnceLock::get_or_init` 保证只有一个线程
    /// 执行工厂，其余线程等待并复用结果。
    pub fn get_or_init_with<F>(&self, key: usize, factory: F) -> AnyService
    where
        F: FnOnce() -> AnyService,
    {
        // Fast path: 读锁查已初始化的 cell。
        {
            let cells = self.cells.read().unwrap();
            if let Some(cell) = cells.get(&key) {
                if let Some(instance) = cell.get() {
                    return instance.clone();
                }
                // cell 存在但未初始化：另一线程正在初始化。
                // clone Arc 后释放读锁，通过 get_or_init 等待。
                let cell = cell.clone();
                drop(cells);
                return cell.get_or_init(factory).clone();
            }
        }
        // Slow path: cell 不存在，写锁创建空 cell 后立即释放。
        let cell = {
            let mut cells = self.cells.write().unwrap();
            cells
                .entry(key)
                .or_insert_with(|| Arc::new(OnceLock::new()))
                .clone()
        };
        // 无锁状态下 get_or_init：工厂执行期间不持有任何全局锁，
        // 递归解析依赖可安全获取相同 LazyCache 的其他 key。
        cell.get_or_init(factory).clone()
    }

    /// 懒初始化（async 版本）：若已缓存则返回，否则在 `FuturesMutex` 保护下
    /// 执行 async factory 并缓存。
    ///
    /// 与 sync `get_or_init_with` 的区别：`OnceLock::get_or_init` 不支持 async
    /// 闭包，故用 per-key `FuturesMutex<Option<AnyService>>` 替代。多 future
    /// 同时 miss 时，只有一个 future 执行 factory，其余 await 等待结果。
    ///
    /// factory 返回 `Result`：成功时缓存并返回 `Ok`，失败时不缓存（允许重试）。
    pub async fn get_or_init_with_async<F, Fut, E>(
        &self,
        key: usize,
        factory: F,
    ) -> Result<AnyService, E>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<AnyService, E>> + Send,
        E: Send,
    {
        // Fast path: 查 OnceLock（sync 缓存）
        if let Some(cached) = self.get(key) {
            return Ok(cached);
        }

        // Slow path: 获取 per-key async 锁
        let lock = {
            let mut locks = self.async_locks.write().unwrap();
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(FuturesMutex::new(None)))
                .clone()
        };

        let mut guard = lock.lock().await;
        // 双重检查：其他 future 可能已填充
        if let Some(ref instance) = *guard {
            return Ok(instance.clone());
        }
        // 再次检查 OnceLock（可能被 sync 路径填充）
        if let Some(cached) = self.get(key) {
            *guard = Some(cached.clone());
            return Ok(cached);
        }

        // 执行 async factory
        let instance = factory().await?;
        *guard = Some(instance.clone());
        // 填入 OnceLock，供后续 sync/async fast path 使用
        self.prefill(key, instance.clone());
        Ok(instance)
    }
}

impl Default for LazyCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    #[test]
    fn get_or_init_executes_factory_once_under_concurrency() {
        static CALLS: AtomicU64 = AtomicU64::new(0);
        let cache = LazyCache::new();
        let cache = Arc::new(cache);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let c = cache.clone();
            handles.push(thread::spawn(move || {
                c.get_or_init_with(42, || {
                    CALLS.fetch_add(1, Ordering::SeqCst);
                    Arc::new(99_u64) as AnyService
                })
            }));
        }
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        assert_eq!(CALLS.load(Ordering::SeqCst), 1, "工厂应只执行一次");
        for r in &results {
            let v = r.downcast_ref::<u64>().unwrap();
            assert_eq!(*v, 99);
        }
    }

    #[test]
    fn prefill_skips_reinit() {
        let cache = LazyCache::new();
        cache.prefill(1, Arc::new(7_u64) as AnyService);
        // 再次 prefill 不覆盖
        cache.prefill(1, Arc::new(999_u64) as AnyService);
        let instance = cache.get(1).unwrap();
        let v = instance.downcast_ref::<u64>().unwrap();
        assert_eq!(*v, 7);
    }

    #[test]
    fn get_returns_none_for_missing() {
        let cache = LazyCache::new();
        assert!(cache.get(0).is_none());
    }
}
