//! Bidirectional OH SDK callbacks (`api/ohsdkplugin.go`).
use crate::ClientConn;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tsrs_core::{
    OhSdkCheckPlugin, OhSdkClassCheckPlugin, OhSdkPluginExecutor, OhSdkPluginNodeSnapshot,
    OhSdkPluginResponse,
};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct State {
    next_source_id: u32,
    sources: std::collections::BTreeMap<String, (u32, String)>,
    project_configs: Vec<serde_json::Map<String, Value>>,
}

impl State {
    fn intern_source(&mut self, node: OhSdkPluginNodeSnapshot) -> Value {
        let mut value = json!({ "fileName": node.file_name, "pos": node.pos, "end": node.end, "text": node.text });
        if let Some((id, _)) = self
            .sources
            .get(&node.file_name)
            .filter(|(_, source)| *source == node.source)
        {
            value["sourceId"] = json!(id);
        } else {
            self.next_source_id += 1;
            value["sourceId"] = json!(self.next_source_id);
            value["source"] = json!(&node.source);
            self.sources
                .insert(node.file_name, (self.next_source_id, node.source));
        }
        value
    }
}

/// The only JavaScript-facing boundary. Compiler requests never pass through it.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPluginRequest {
    pub session_id: u64,
    pub path: String,
    pub function_name: String,
    pub class_name: String,
    pub operation: String,
    pub args: Vec<Value>,
}

pub trait SdkPluginHost: Send + Sync {
    fn invoke_batch(
        &self,
        requests: Vec<SdkPluginRequest>,
    ) -> Result<Vec<OhSdkPluginResponse>, String>;
}

struct ClientPluginHost(Arc<dyn ClientConn>);
impl SdkPluginHost for ClientPluginHost {
    fn invoke_batch(
        &self,
        requests: Vec<SdkPluginRequest>,
    ) -> Result<Vec<OhSdkPluginResponse>, String> {
        let raw = self
            .0
            .call(
                "ohSdkPluginBatch",
                &serde_json::to_string(&requests).map_err(|e| e.to_string())?,
            )
            .map_err(|error| error.to_string())?;
        serde_json::from_str(&raw)
            .map_err(|error| format!("invalid OH SDK plugin batch response: {error}"))
    }
}

pub struct SdkPluginExecutor {
    host: Arc<dyn SdkPluginHost>,
    session_id: u64,
    state: Mutex<State>,
}
impl std::fmt::Debug for SdkPluginExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkPluginExecutor")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}
impl SdkPluginExecutor {
    pub fn new(host: Arc<dyn SdkPluginHost>) -> Self {
        Self {
            host,
            session_id: SESSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1,
            state: Mutex::default(),
        }
    }
}
impl SdkPluginExecutor {
    pub(crate) fn from_connection(conn: Arc<dyn ClientConn>) -> Self {
        Self::new(Arc::new(ClientPluginHost(conn)))
    }
}
impl OhSdkPluginExecutor for SdkPluginExecutor {
    fn call(
        &self,
        plugin: &OhSdkCheckPlugin,
        operation: &str,
        args: Vec<Value>,
    ) -> Result<OhSdkPluginResponse, String> {
        let request = SdkPluginRequest {
            session_id: self.session_id,
            path: plugin.path.clone(),
            function_name: plugin.function_name.clone(),
            class_name: plugin.function_name.clone(),
            operation: operation.into(),
            args,
        };
        let responses = (|| {
            let responses = self.host.invoke_batch(vec![request])?;
            if responses.len() != 1 {
                return Err(format!(
                    "invalid OH SDK plugin batch response: received {} responses for 1 requests",
                    responses.len()
                ));
            }
            Ok(responses)
        })();
        let mut responses = match responses {
            Ok(responses) => responses,
            Err(_) if matches!(operation, "value" | "format" | "prepareClass") => {
                return Ok(OhSdkPluginResponse::default());
            }
            Err(error) => return Err(error),
        };
        let response = responses.pop().unwrap();
        // Source-defined policies catch loading errors for class callbacks and
        // loading/invocation errors for value and format callbacks.
        if !response.error.is_empty() {
            if matches!(operation, "value" | "format")
                || operation == "syscap" && response.phase == "load"
                || operation == "prepareClass" && !response.found
            {
                return Ok(OhSdkPluginResponse::default());
            }
            return Err(format!(
                "SDK plugin {}#{} {} failed: {}",
                plugin.path, plugin.function_name, response.phase, response.error
            ));
        }
        Ok(response)
    }
    fn check_syscap(
        &self,
        plugin: &OhSdkClassCheckPlugin,
        node: OhSdkPluginNodeSnapshot,
        declaration: OhSdkPluginNodeSnapshot,
        config: serde_json::Map<String, Value>,
    ) -> Result<OhSdkPluginResponse, String> {
        // SyscapChecker.check consumes the original nodes and project config
        // synchronously. Keep their interning publication ordered with host
        // dispatch: another checker must not send an ID-only reference before
        // JavaScript has consumed the request that registers that ID.
        let mut state = self.state.lock().unwrap();
        let request = {
            let mut request = json!({ "node": state.intern_source(node), "declaration": state.intern_source(declaration) });
            if let Some(index) = state
                .project_configs
                .iter()
                .position(|item| *item == config)
            {
                request["projectConfigId"] = json!(index + 1);
            } else {
                request["projectConfigId"] = json!(state.project_configs.len() + 1);
                request["projectConfig"] = json!(&config);
                state.project_configs.push(config);
            }
            request
        };
        let response = self.call(
            &OhSdkCheckPlugin {
                path: plugin.path.clone(),
                function_name: plugin.class_name.clone(),
                ..Default::default()
            },
            "syscap",
            vec![request],
        );
        drop(state);
        response
    }
}

/// Shared by transport and embedded program creation. OH skips missing class exports.
pub(crate) fn prepare_class_plugins(
    options: &mut tsrs_core::CompilerOptions,
) -> crate::ApiResult<()> {
    if let Some(executor) = &options.oh_sdk_plugin_executor {
        let mut loaded = Vec::new();
        for plugin in options
            .oh_sdk_class_check_plugins
            .take()
            .unwrap_or_default()
        {
            let descriptor = tsrs_core::OhSdkCheckPlugin {
                path: plugin.path.clone(),
                function_name: plugin.class_name.clone(),
                ..Default::default()
            };
            let response = executor
                .call(&descriptor, "prepareClass", Vec::new())
                .map_err(|error| {
                    crate::ApiError::internal(format!(
                        "failed to initialize OH SDK checker {}#{}: {error}",
                        plugin.path, plugin.class_name
                    ))
                })?;
            if response.found {
                loaded.push(plugin);
            }
        }
        options.oh_sdk_class_check_plugins = Some(loaded);
    }
    Ok(())
}
