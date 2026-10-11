//! Bidirectional OH SDK callbacks (`api/ohsdkplugin.go`).
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};
use serde_json::{json, Value};
use tsrs_core::{OhSdkCheckPlugin, OhSdkClassCheckPlugin, OhSdkPluginExecutor, OhSdkPluginNodeSnapshot, OhSdkPluginResponse};
use crate::ClientConn;

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
        if let Some((id, _)) = self.sources.get(&node.file_name).filter(|(_, source)| *source == node.source) {
            value["sourceId"] = json!(id);
        } else {
            self.next_source_id += 1;
            value["sourceId"] = json!(self.next_source_id);
            value["source"] = json!(&node.source);
            self.sources.insert(node.file_name, (self.next_source_id, node.source));
        }
        value
    }
}

pub(crate) struct ClientOhSdkPluginExecutor {
    conn: Arc<dyn ClientConn>,
    session_id: u64,
    state: Mutex<State>,
}
impl std::fmt::Debug for ClientOhSdkPluginExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientOhSdkPluginExecutor").field("session_id", &self.session_id).finish_non_exhaustive()
    }
}
impl ClientOhSdkPluginExecutor {
    pub(crate) fn new(conn: Arc<dyn ClientConn>) -> Self {
        Self { conn, session_id: SESSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1, state: Mutex::default() }
    }
}
impl OhSdkPluginExecutor for ClientOhSdkPluginExecutor {
    fn call(&self, plugin: &OhSdkCheckPlugin, operation: &str, args: Vec<Value>) -> Result<OhSdkPluginResponse, String> {
        let request = json!([{ "sessionId": self.session_id, "path": plugin.path,
            "functionName": plugin.function_name, "className": plugin.function_name,
            "operation": operation, "args": args }]);
        let responses = (|| {
            let raw = self.conn.call("ohSdkPluginBatch", &request.to_string()).map_err(|error| error.to_string())?;
            let responses: Vec<OhSdkPluginResponse> = serde_json::from_str(&raw)
                .map_err(|error| format!("invalid OH SDK plugin batch response: {error}"))?;
            if responses.len() != 1 {
                return Err(format!("invalid OH SDK plugin batch response: received {} responses for 1 requests", responses.len()));
            }
            Ok(responses)
        })();
        let mut responses = match responses {
            Ok(responses) => responses,
            Err(_) if matches!(operation, "value" | "format" | "prepareClass") => return Ok(OhSdkPluginResponse::default()),
            Err(error) => return Err(error),
        };
        let response = responses.pop().unwrap();
        // Source-defined policies catch loading errors for class callbacks and
        // loading/invocation errors for value and format callbacks.
        if !response.error.is_empty() {
            if matches!(operation, "value" | "format") || operation == "syscap" && response.phase == "load"
                || operation == "prepareClass" && !response.found {
                return Ok(OhSdkPluginResponse::default());
            }
            return Err(format!("SDK plugin {}#{} {} failed: {}", plugin.path, plugin.function_name, response.phase, response.error));
        }
        Ok(response)
    }
    fn check_syscap(&self, plugin: &OhSdkClassCheckPlugin, node: OhSdkPluginNodeSnapshot,
        declaration: OhSdkPluginNodeSnapshot, config: serde_json::Map<String, Value>) -> Result<OhSdkPluginResponse, String> {
        let request = {
            let mut state = self.state.lock().unwrap();
            let mut request = json!({ "node": state.intern_source(node), "declaration": state.intern_source(declaration) });
            if let Some(index) = state.project_configs.iter().position(|item| *item == config) {
                request["projectConfigId"] = json!(index + 1);
            } else {
                request["projectConfigId"] = json!(state.project_configs.len() + 1);
                request["projectConfig"] = json!(&config);
                state.project_configs.push(config);
            }
            request
        };
        self.call(&OhSdkCheckPlugin { path: plugin.path.clone(), function_name: plugin.class_name.clone(), ..Default::default() }, "syscap", vec![request])
    }
}
