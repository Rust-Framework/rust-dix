mod common;

use rust_dicore::*;
use std::sync::Arc;

#[test]
fn iprovider_root_resolves() {
    let p = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 100 }))
        .build()
        .unwrap();
    let rp: Arc<dyn IProvider> = p;
    let any = rp.get_any(std::any::type_name::<common::MyService>()).unwrap();
    let svc: Arc<common::MyService> = Arc::clone(&*any.downcast::<Arc<common::MyService>>().unwrap());
    assert_eq!(svc.value, 100);
}

#[test]
fn iprovider_wrapped_resolves() {
    let root = ServiceCollection::new()
        .singleton(|_| Arc::new(common::MyService { value: 200 }))
        .build()
        .unwrap();
    let child = ServiceCollection::new().build().unwrap();
    let wrapper = ServiceProviderWrapper::new(child, root);
    let rp: Arc<dyn IProvider> = wrapper;
    let any = rp.get_any(std::any::type_name::<common::MyService>()).unwrap();
    let svc: Arc<common::MyService> = Arc::clone(&*any.downcast::<Arc<common::MyService>>().unwrap());
    assert_eq!(svc.value, 200);
}

#[test]
fn iprovider_get_named_any() {
    let p = ServiceCollection::new().build().unwrap();
    p.register_named("rdi_key", Arc::new(common::MyService { value: 5 }));
    let rp: Arc<dyn IProvider> = p;
    let retrieved = rp.get_named_any("rdi_key");
    assert!(retrieved.is_some());
}

#[test]
fn iprovider_register_and_remove_named() {
    let p = ServiceCollection::new().build().unwrap();
    let rp: Arc<dyn IProvider> = p;
    let svc = Arc::new(common::MyService { value: 3 });
    rp.rdi_register_named("test", svc);
    let retrieved = rp.get_named_any("test");
    assert!(retrieved.is_some());
    let down: Arc<common::MyService> = retrieved.unwrap().downcast::<common::MyService>().unwrap();
    assert_eq!(down.value, 3);
    rp.rdi_remove_named("test");
    assert!(rp.get_named_any("test").is_none());
}
