//! OH build-host SDK plugin descriptors (`core/ohsdkplugin.go`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OhSdkCheckPlugin {
    pub os_name: String,
    pub tag: String,
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub path: String,
    pub function_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OhSdkClassCheckPlugin {
    pub tag_name: String,
    pub path: String,
    pub class_name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OhSdkPluginNodeSnapshot {
    pub file_name: String,
    pub source: String,
    pub pos: i32,
    pub end: i32,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OhSdkPluginResponse {
    pub found: bool,
    pub result: bool,
    pub message: String,
    pub valid: bool,
    pub version: String,
    pub matched: bool,
    pub groups: Vec<String>,
    pub phase: String,
    pub error: String,
    pub check_result: bool,
    pub check_message: String,
}

/// Synchronous build-host ABI. The checker chooses exports and interprets
/// results; the host owns JavaScript loading and invocation.
pub trait OhSdkPluginExecutor: std::fmt::Debug + Send + Sync {
    fn call(&self, plugin: &OhSdkCheckPlugin, operation: &str, args: Vec<serde_json::Value>) -> Result<OhSdkPluginResponse, String>;
    fn check_syscap(&self, plugin: &OhSdkClassCheckPlugin, node: OhSdkPluginNodeSnapshot,
        declaration: OhSdkPluginNodeSnapshot, config: serde_json::Map<String, serde_json::Value>) -> Result<OhSdkPluginResponse, String>;
}
