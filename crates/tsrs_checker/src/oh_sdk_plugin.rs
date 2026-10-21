//! OH checker-owned SDK callback selection and error policies.
use crate::*;
use tsrs_core::{OhSdkCheckPlugin, OhSdkPluginNodeSnapshot, OhSdkPluginResponse};
use serde_json::json;

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
    pub(crate) fn oh_sdk_node_snapshot(node: P<Node>) -> OhSdkPluginNodeSnapshot {
        let file = ast::get_source_file_of_node(node).unwrap();
        let positions = file.get_position_map();
        // scanner/utilities::get_text_of_node also handles missing nodes,
        // virtual substring bounds, JSDoc normalization and reparsed literals.
        let missing = ast::node_is_missing(Some(node));
        let node_start = node.pos().source_offset().unwrap_or(0);
        let node_end = node.end().source_offset().unwrap_or(0);
        let start = if missing { node_start } else { tsrs_scanner::skip_trivia(file.text(), node_start) };
        let text = (missing || node.is_virtual() && start > node_end
            || tsrs_scanner::is_jsdoc_type_expression_or_child(node)
            || node.flags().intersects(NodeFlags::ReparserTransformedLiteral))
            .then(|| tsrs_scanner::get_text_of_node(node));
        OhSdkPluginNodeSnapshot { file_name: file.file_name().to_string(), file_id: ast::get_node_id(file.as_node()).0,
            source: file.text(), pos: positions.utf8_to_utf16(node_start) as i32, end: positions.utf8_to_utf16(node_end) as i32,
            start: positions.utf8_to_utf16(start) as i32, text }
    }
}
