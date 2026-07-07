mod common;

use rust_dix::*;
use std::sync::Arc;

#[test]
fn wrapper_get_keyed_fallback_to_root() {
    let root = ServiceCollection::new()
        .keyed_singleton("a", |_| {
            Arc::new(common::Logger {
                prefix: "root".into(),
            })
        })
        .build()
        .unwrap();
    let child = ServiceCollection::new().build().unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let svc: Arc<common::Logger> = w.get_keyed("a").unwrap();
    assert_eq!(svc.prefix, "root");
}

#[test]
fn wrapper_get_named_child_first() {
    let root = ServiceCollection::new().build().unwrap();
    let child = ServiceCollection::new().build().unwrap();
    let root_svc = Arc::new(common::Logger {
        prefix: "root".into(),
    });
    let child_svc = Arc::new(common::Logger {
        prefix: "child".into(),
    });
    root.register_named("log", root_svc);
    child.register_named("log", child_svc.clone());
    let w = ServiceProviderWrapper::new(child.clone(), root.clone());
    let retrieved: Arc<common::Logger> = w.get_named("log").unwrap();
    assert_eq!(retrieved.prefix, "child");
}

#[test]
fn wrapper_get_named_fallback_to_root() {
    let root = ServiceCollection::new().build().unwrap();
    let child = ServiceCollection::new().build().unwrap();
    root.register_named("shared", Arc::new(common::MyService { value: 99 }));
    let w = ServiceProviderWrapper::new(child, root);
    let retrieved: Arc<common::MyService> = w.get_named("shared").unwrap();
    assert_eq!(retrieved.value, 99);
}

#[test]
fn wrapper_get_named_any_child_first() {
    let root = ServiceCollection::new().build().unwrap();
    let child = ServiceCollection::new().build().unwrap();
    root.register_named("key", Arc::new(common::MyService { value: 1 }));
    child.register_named("key", Arc::new(common::MyService { value: 2 }));
    let w = ServiceProviderWrapper::new(child, root);
    let retrieved = w.get_named_any("key").unwrap();
    let svc: Arc<common::MyService> = retrieved.downcast::<common::MyService>().unwrap();
    assert_eq!(svc.value, 2);
}

#[test]
fn wrapper_get_all_combines_both() {
    let root = ServiceCollection::new()
        .keyed_singleton("a", |_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    let child = ServiceCollection::new()
        .keyed_singleton("b", |_| Arc::new(common::MyService { value: 2 }))
        .build()
        .unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let all: Vec<Arc<common::MyService>> = w.get_all();
    assert_eq!(all.len(), 2);
}

#[test]
fn wrapper_owned_resolution_child_first() {
    // Owned 解析：child 注册 Transient，wrapper.get_owned() 成功
    let root = ServiceCollection::new().build().unwrap();
    let child = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 42 }))
        .build()
        .unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let owned: common::MyService = w.get_owned().unwrap();
    assert_eq!(owned.value, 42);
}

#[test]
fn wrapper_owned_fallback_to_root() {
    // child 未注册，root 注册 Transient，wrapper.get_owned() 回退到 root
    let root = ServiceCollection::new()
        .transient(|_| Arc::new(common::MyService { value: 99 }))
        .build()
        .unwrap();
    let child = ServiceCollection::new().build().unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let owned: common::MyService = w.get_owned().unwrap();
    assert_eq!(owned.value, 99);
}

#[test]
fn wrapper_owned_singleton_returns_none() {
    // Singleton 服务不能被 owned，wrapper.get_owned() 返回 Err
    let root = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 1 }))
        .build()
        .unwrap();
    let child = ServiceCollection::new().build().unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    assert!(w.get_owned::<common::MyService>().is_err());
}

#[tokio::test]
async fn wrapper_async_resolution_child_first() {
    // async 解析：child 注册 async_transient，wrapper.get_async() 成功
    let root = ServiceCollection::new().build().unwrap();
    let child = ServiceCollection::new()
        .async_transient(|_| Box::pin(async { Arc::new(common::MyService { value: 7 }) }))
        .build_async()
        .await
        .unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let svc: Arc<common::MyService> = w.get_async().await.unwrap();
    assert_eq!(svc.value, 7);
}

#[tokio::test]
async fn wrapper_async_fallback_to_root() {
    // child 未注册 async，root 注册 async_transient，wrapper.get_async() 回退到 root
    let root = ServiceCollection::new()
        .async_transient(|_| Box::pin(async { Arc::new(common::MyService { value: 11 }) }))
        .build_async()
        .await
        .unwrap();
    let child = ServiceCollection::new().build_async().await.unwrap();
    let w = ServiceProviderWrapper::new(child, root);
    let svc: Arc<common::MyService> = w.get_async().await.unwrap();
    assert_eq!(svc.value, 11);
}
