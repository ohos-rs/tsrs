//! OH checker-owned SDK callback selection and error policies.
use crate::*;
use tsrs_core::{OhSdkCheckPlugin, OhSdkPluginNodeSnapshot, OhSdkPluginResponse};
use serde_json::{json, Value};

impl Checker {
    fn oh_sdk_plugins(&self, tag: &str, kind: &str) -> Vec<&OhSdkCheckPlugin> {
        self.compiler_options.oh_sdk_check_plugins.as_deref().unwrap_or_default().iter()
            .filter(|plugin| plugin.os_name == self.oh_runtime_os() && plugin.tag == tag && plugin.kind == kind).collect()
    }
    pub(crate) fn oh_sdk_check_value(&self, tag: &str, required: &str, target: &str, scene: i32) -> Option<OhSdkPluginResponse> {
        let executor = self.compiler_options.oh_sdk_plugin_executor.as_ref()?;
        let mut plugins = self.oh_sdk_plugins(tag, "CompatibilityCheck");
        if plugins.is_empty() { plugins = self.oh_sdk_plugins(tag, ""); }
        for plugin in plugins {
            let result = executor.call(plugin, "value", vec![json!(required), json!(target), json!(scene)])
                .unwrap_or_else(|e| panic!("OH SDK value checker: {e}"));
            if result.found { return Some(result); }
        }
        None
    }
    pub(crate) fn oh_sdk_check_format(&self, tag: &str, version: &str) -> Option<OhSdkPluginResponse> {
        let executor = self.compiler_options.oh_sdk_plugin_executor.as_ref()?;
        for plugin in self.oh_sdk_plugins(tag, "FormatValidation") {
            let result = executor.call(plugin, "format", vec![json!(version)])
                .unwrap_or_else(|e| panic!("OH SDK format checker: {e}"));
            if result.found { return Some(result); }
        }
        None
    }
    pub(crate) fn oh_sdk_check_distribution(&self, tag: &str, version: &str) -> Option<OhSdkPluginResponse> {
        let executor = self.compiler_options.oh_sdk_plugin_executor.as_ref()?;
        let mut result = None;
        for plugin in self.oh_sdk_plugins(tag, "") {
            let Ok(current) = executor.call(plugin, "distribution", vec![json!(version)]) else { return result; };
            if current.found { result = Some(current); }
        }
        result
    }
    pub(crate) fn oh_sdk_match_build_version(&self, tag: &str, version: &str) -> Option<OhSdkPluginResponse> {
        let executor = self.compiler_options.oh_sdk_plugin_executor.as_ref()?;
        for plugin in self.oh_sdk_plugins(tag, "getBuildVersionRegex") {
            let result = executor.call(plugin, "regex", vec![json!(version)]).ok()?;
            if result.found { return Some(result); }
        }
        None
    }
    pub(crate) fn oh_sdk_check_syscap(&self, node: P<Node>, declaration: P<Node>) -> Option<OhSdkPluginResponse> {
        let executor = self.compiler_options.oh_sdk_plugin_executor.as_ref()?;
        let mut result = None;
        for plugin in self.compiler_options.oh_sdk_class_check_plugins.as_deref().unwrap_or_default() {
            if plugin.tag_name != "syscap" { continue; }
            let current = executor.check_syscap(plugin, node_snapshot(node), node_snapshot(declaration), self.oh_sdk_plugin_project_config())
                .unwrap_or_else(|e| panic!("OH SDK syscap checker: {e}"));
            if !current.found { continue; }
            if !current.check_result { return Some(current); }
            result = Some(current);
        }
        result
    }
    fn oh_sdk_plugin_project_config(&self) -> serde_json::Map<String, Value> {
        let o = self.compiler_options;
        if let Some(source) = &o.oh_sdk_plugin_project_config { return source.clone(); }
        let Value::Object(value) = json!({
            "runtimeOS": self.oh_runtime_os(), "originCompatibleSdkVersion": o.oh_origin_compatible_sdk_version,
            "compatibleSdkVersion": o.compatible_sdk_version, "compileSdkVersion": o.compile_sdk_version,
            "projectRootPath": o.oh_project_root_path, "projectPath": o.oh_project_path,
            "modulePath": o.oh_module_path, "etsLoaderPath": o.ets_loader_path,
            "externalApiPaths": o.oh_external_api_paths.as_deref().unwrap_or_default(),
            "requestPermissions": o.oh_request_permissions.as_deref().unwrap_or_default(),
            "syscapIntersectionSet": o.oh_syscap_intersection.as_deref().unwrap_or_default(),
            "syscapUnionSet": o.oh_syscap_union.as_deref().unwrap_or_default(),
            "deviceTypes": o.oh_device_types.as_deref().unwrap_or_default(),
            "isCrossplatform": o.oh_crossplatform == Tristate::True,
            "ignoreCrossplatformCheck": o.oh_ignore_crossplatform_check == Tristate::True,
            "compileMode": o.oh_compile_mode, "bundleType": o.oh_bundle_type,
            "strictMode": { "apiCompatibilityCheck": o.oh_api_compatibility_check }
        }) else { unreachable!() };
        value
    }
}
fn node_snapshot(node: P<Node>) -> OhSdkPluginNodeSnapshot {
    let file = ast::get_source_file_of_node(node).unwrap();
    let positions = file.get_position_map();
    OhSdkPluginNodeSnapshot { file_name: file.file_name().to_string(), source: file.text().to_string(),
        pos: positions.utf8_to_utf16(node.pos()), end: positions.utf8_to_utf16(node.end()), text: tsrs_scanner::get_text_of_node(node) }
}
