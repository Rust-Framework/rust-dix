mod common;

use std::sync::Arc;

#[test]
fn module_factory_with_resolver() {
    #[rust_dix::module]
    mod mf {
        rust_dix::register!(factory singleton: super::common::Logger => super::common::Logger { prefix: "factory".into() });
    }
    let p = mf::__rdi_build_provider_mf().unwrap();
    assert_eq!(p.get::<common::Logger>().unwrap().prefix, "factory");
}

#[test]
fn module_plugin_trait() {
    #[rust_dix::module]
    mod plugin_svc {
        rust_dix::register!(singleton: dyn super::common::IPlugin => super::common::TestPlugin);
    }
    let provider = plugin_svc::__rdi_build_provider_plugin_svc().unwrap();
    let plugin: Arc<dyn common::IPlugin> = provider.get::<dyn common::IPlugin>().unwrap();
    assert_eq!(plugin.name(), "test_plugin");
}
