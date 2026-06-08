mod common;

use lrdi::*;
use std::sync::Arc;

#[test]
fn module_factory_with_resolver() {
    #[lrdi::module]
    mod mf {
        lrdi::inject!(factory singleton: super::common::Logger => super::common::Logger { prefix: "factory".into() });
    }
    let p = mf::__rdi_build_provider_mf().unwrap();
    assert_eq!(p.get::<common::Logger>().prefix, "factory");
}

#[test]
fn module_plugin_trait() {
    #[lrdi::module]
    mod plugin_svc {
        lrdi::inject!(singleton: dyn super::common::IPlugin => super::common::TestPlugin);
    }
    let provider = plugin_svc::__rdi_build_provider_plugin_svc().unwrap();
    let plugin: Arc<dyn common::IPlugin> = provider.get::<dyn common::IPlugin>();
    assert_eq!(plugin.name(), "test_plugin");
}
