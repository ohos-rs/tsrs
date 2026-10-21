//! OH build-host SDK plugin descriptors (`core/ohsdkplugin.go`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock, mpsc};

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

#[derive(Clone, Debug)]
pub struct OhSdkPluginNodeSnapshot {
    pub file_name: String,
    /// The immutable parsed `SourceFile` node ID; a new parse has a new ID even
    /// if its arena address is reused after an embedded program is released.
    pub file_id: u64,
    /// Borrowed only during submission while its program is alive.
    /// The executor must not retain this arena-backed reference.
    pub source: &'static str,
    pub pos: i32,
    pub end: i32,
    pub start: i32,
    pub text: Option<String>,
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
    /// Transport-only: an earlier checker short-circuited this node's chain.
    pub skipped: bool,
}

pub type OhSdkSyscapResults = Vec<Result<Option<OhSdkPluginResponse>, String>>;

/// Owns only a reply channel, never borrowed source text or checker-region data.
pub struct OhSdkPendingSyscap(mpsc::Receiver<OhSdkSyscapResults>);
impl OhSdkPendingSyscap {
    pub fn channel() -> (mpsc::Sender<OhSdkSyscapResults>, Self) {
        let (sender, receiver) = mpsc::channel();
        (sender, Self(receiver))
    }
    pub fn wait(self) -> OhSdkSyscapResults {
        self.0.recv().expect("OH SDK syscap host dropped its completion without responding")
    }
}

/// Function callbacks remain synchronous. Syscap submission copies all borrowed
/// input before returning; completion owns only transport values, not AST nodes.
pub trait OhSdkPluginExecutor: std::fmt::Debug + Send + Sync {
    fn call(&self, plugin: &OhSdkCheckPlugin, operation: &str, args: Vec<serde_json::Value>) -> Result<OhSdkPluginResponse, String>;
    fn check_syscap_batch(&self, plugins: &[OhSdkClassCheckPlugin],
        nodes: Vec<(OhSdkPluginNodeSnapshot, OhSdkPluginNodeSnapshot)>,
        config: Arc<serde_json::Map<String, serde_json::Value>>) -> OhSdkSyscapResults {
        self.submit_syscap_batch(plugins, nodes, config).wait()
    }
    fn submit_syscap_batch(&self, plugins: &[OhSdkClassCheckPlugin],
        nodes: Vec<(OhSdkPluginNodeSnapshot, OhSdkPluginNodeSnapshot)>,
        config: Arc<serde_json::Map<String, serde_json::Value>>) -> OhSdkPendingSyscap;
}

/// Identity for `CompilerOptions` equality: executors are runtime handles, so the same handle compares equal
/// and distinct ones do not (the options' serialized configuration is what the watcher compares).
impl PartialEq for dyn OhSdkPluginExecutor {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Derived configuration for an immutable compiler-options snapshot.
///
/// Cloning options clears the derived cache: the clone can be reconfigured before publication (including
/// incremental updates), while explicit configs share `Arc`.
#[derive(Debug, Default)]
pub struct OhSdkPluginConfigCache(OnceLock<Arc<serde_json::Map<String, Value>>>);
impl Clone for OhSdkPluginConfigCache {
    fn clone(&self) -> Self { Self::default() }
}
/// The cache is derived data: compare what it holds (nothing before first use, so fresh options are equal).
impl PartialEq for OhSdkPluginConfigCache {
    fn eq(&self, other: &Self) -> bool {
        self.0.get().map(std::convert::AsRef::as_ref)
            == other.0.get().map(std::convert::AsRef::as_ref)
    }
}
impl OhSdkPluginConfigCache {
    pub fn get(&self, o: &crate::CompilerOptions) -> Arc<serde_json::Map<String, Value>> {
        if let Some(source) = &o.oh_sdk_plugin_project_config { return Arc::clone(source); }
        Arc::clone(self.0.get_or_init(|| {
            let Value::Object(value) = json!({
                "runtimeOS": if o.oh_runtime_os.is_empty() { "OpenHarmony" } else { &o.oh_runtime_os }, "originCompatibleSdkVersion": o.oh_origin_compatible_sdk_version,
                "compatibleSdkVersion": o.compatible_sdk_version, "compileSdkVersion": o.compile_sdk_version,
                "projectRootPath": o.oh_project_root_path, "projectPath": o.oh_project_path,
                "modulePath": o.oh_module_path, "etsLoaderPath": o.ets_loader_path,
                "externalApiPaths": o.oh_external_api_paths.as_deref().unwrap_or_default(),
                "requestPermissions": o.oh_request_permissions.as_deref().unwrap_or_default(),
                "syscapIntersectionSet": o.oh_syscap_intersection.as_deref().unwrap_or_default(),
                "syscapUnionSet": o.oh_syscap_union.as_deref().unwrap_or_default(),
                "deviceTypes": o.oh_device_types.as_deref().unwrap_or_default(),
                "isCrossplatform": o.oh_crossplatform == crate::Tristate::True,
                "ignoreCrossplatformCheck": o.oh_ignore_crossplatform_check == crate::Tristate::True,
                "compileMode": o.oh_compile_mode, "bundleType": o.oh_bundle_type,
                "strictMode": { "apiCompatibilityCheck": o.oh_api_compatibility_check }
            }) else { unreachable!() };
            Arc::new(value)
        }))
    }
}
