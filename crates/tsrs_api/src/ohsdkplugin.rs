//! Bidirectional OH SDK callbacks (`api/ohsdkplugin.go`).
use crate::ClientConn;
use rustc_hash::FxHashMap;
use serde_json::{Value, json};
use std::hash::Hash;
use std::sync::{Arc, Mutex, OnceLock, RwLock, atomic::{AtomicU64, Ordering}};
use tsrs_core::{OhSdkCheckPlugin, OhSdkClassCheckPlugin, OhSdkPendingSyscap, OhSdkPluginExecutor, OhSdkPluginNodeSnapshot, OhSdkPluginResponse};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
type ProjectConfig = serde_json::Map<String, Value>;
type PluginKey = (String, String, String);
type PluginResult = Result<OhSdkPluginResponse, String>;

// Preserve the former serialized-key distinctions without serializing: object
// entry order, integer versus float, and the sign of floating-point zero.
// Value's Eq alone merges +0.0 and -0.0, which JS plugins can distinguish.
#[derive(Clone, Hash, PartialEq, Eq)]
enum ArgumentKey {
    Null, Bool(bool), Signed(i64), Unsigned(u64), Float(u64), String(String),
    Array(Vec<ArgumentKey>), Object(Vec<(String, ArgumentKey)>),
}
impl ArgumentKey {
    fn new(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() { Self::Signed(value) }
                else if let Some(value) = value.as_u64() { Self::Unsigned(value) }
                else { Self::Float(value.as_f64().unwrap().to_bits()) }
            }
            Value::String(value) => Self::String(value.clone()),
            Value::Array(values) => Self::Array(values.iter().map(Self::new).collect()),
            Value::Object(values) => Self::Object(values.iter().map(|(key, value)| (key.clone(), Self::new(value))).collect()),
        }
    }
}
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum FunctionOperation { Value, Format, Distribution, Regex }
impl FunctionOperation {
    fn parse(operation: &str) -> Option<Self> {
        match operation { "value" => Some(Self::Value), "format" => Some(Self::Format),
            "distribution" => Some(Self::Distribution), "regex" => Some(Self::Regex), _ => None }
    }
}
#[derive(Clone, Hash, PartialEq, Eq)]
struct FunctionKey { plugin: u32, operation: FunctionOperation, args: Vec<ArgumentKey> }
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct NodeKey { file: u64, pos: i32, end: i32, start: i32 }
impl NodeKey {
    fn new(node: &OhSdkPluginNodeSnapshot) -> Self { Self { file: node.file_id, pos: node.pos, end: node.end, start: node.start } }
}
#[derive(Clone, Hash, PartialEq, Eq)]
struct SyscapKey { plugin: u32, node: NodeKey, declaration: NodeKey, config: usize }

struct ResultCache<K>(RwLock<FxHashMap<K, Arc<OnceLock<PluginResult>>>>);
impl<K> Default for ResultCache<K> {
    fn default() -> Self { Self(RwLock::default()) }
}
impl<K: Clone + Eq + Hash> ResultCache<K> {
    fn invoke(&self, key: &K, invoke: impl FnOnce() -> PluginResult) -> PluginResult {
        let cached = self.0.read().unwrap().get(key).cloned();
        let slot = cached.unwrap_or_else(|| Arc::clone(self.0.write().unwrap().entry(key.clone()).or_default()));
        // Only callers for this exact key wait on each other. Neither the map
        // lock nor the registration mutex is held during the host round trip.
        let result = slot.get_or_init(invoke);
        if !result.as_ref().is_ok_and(|response| response.error.is_empty()) {
            let mut cache = self.0.write().unwrap();
            if cache.get(key).is_some_and(|value| Arc::ptr_eq(value, &slot)) { cache.remove(key); }
        }
        result.clone()
    }
}

#[derive(Default)]
struct ConfigTable {
    // Retain every distinct Arc, including equal configs, so an address cannot
    // be recycled while the executor still uses it as a fast identity key.
    identities: FxHashMap<usize, (Arc<ProjectConfig>, usize)>,
    values: Vec<Arc<ProjectConfig>>,
}
impl ConfigTable {
    fn intern(&mut self, config: Arc<ProjectConfig>) -> usize {
        let address = Arc::as_ptr(&config) as usize;
        if let Some((_, id)) = self.identities.get(&address) { return *id; }
        let index = self.values.iter().position(|value| **value == *config).unwrap_or_else(|| {
            self.values.push(Arc::clone(&config));
            self.values.len() - 1
        });
        self.identities.insert(address, (config, index));
        index
    }
}

// Claim all misses of a file together. Waiting before claiming any prevents
// cycles between concurrent files with overlapping exact callback keys.
#[derive(Default)]
struct SyscapResults {
    values: RwLock<FxHashMap<SyscapKey, OhSdkPluginResponse>>,
    pending: Mutex<FxHashMap<SyscapKey, Arc<OnceLock<()>>>>,
}
struct SyscapReservation { cache: Arc<SyscapResults>, keys: Vec<SyscapKey>, done: Arc<OnceLock<()>> }
impl SyscapResults {
    fn reserve(self: &Arc<Self>, keys: &[SyscapKey]) -> SyscapReservation {
        let done = Arc::new(OnceLock::new());
        loop {
            let complete = { let values = self.values.read().unwrap(); keys.iter().all(|key| values.contains_key(key)) };
            if complete {
                return SyscapReservation { cache: Arc::clone(self), keys: Vec::new(), done };
            }
            let mut pending = self.pending.lock().unwrap();
            let values = self.values.read().unwrap();
            if let Some(wait) = keys.iter().filter(|key| !values.contains_key(*key)).find_map(|key| pending.get(key).cloned()) {
                drop(values); drop(pending); wait.wait(); continue;
            }
            let mut claimed = Vec::new();
            for key in keys {
                if !values.contains_key(key) && !pending.contains_key(key) {
                    pending.insert(key.clone(), Arc::clone(&done));
                    claimed.push(key.clone());
                }
            }
            return SyscapReservation { cache: Arc::clone(self), keys: claimed, done };
        }
    }
}
impl Drop for SyscapReservation {
    fn drop(&mut self) {
        if self.keys.is_empty() { return; }
        let mut pending = self.cache.pending.lock().unwrap();
        for key in &self.keys { pending.remove(key); }
        let _ = self.done.set(());
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum ResourceKey { Source(u64), Config(usize), Plugin(u32) }
struct Registration { id: usize, publication: Arc<OnceLock<()>> }
#[derive(Default)]
struct SourceState { next_source_id: usize, registrations: FxHashMap<ResourceKey, Registration> }
struct WireReference { id: usize, full: bool, sent: bool, publication: Arc<OnceLock<()>> }
struct PreparedBatch { references: FxHashMap<ResourceKey, WireReference> }
impl SourceState {
    fn prepare(&mut self, keys: &[ResourceKey]) -> PreparedBatch {
        for &key in keys {
            self.registrations.entry(key).or_insert_with(|| {
                let id = match key {
                    ResourceKey::Source(_) => { self.next_source_id += 1; self.next_source_id }
                    ResourceKey::Config(index) => index + 1,
                    ResourceKey::Plugin(id) => id as usize,
                };
                Registration { id, publication: Arc::default() }
            });
        }
        // In-flight publications are not references yet. Resend their full
        // payload until a reply acknowledges registration. Batches may arrive
        // out of order; this protocol does not rely on N-API TSFN FIFO ordering.
        let references: FxHashMap<_, _> = keys.iter().map(|&key| {
            let registration = &self.registrations[&key];
            let full = registration.publication.get().is_none();
            (key, WireReference { id: registration.id, full, sent: false, publication: Arc::clone(&registration.publication) })
        }).collect();
        PreparedBatch { references }
    }
}
impl PreparedBatch {
    fn reference(&mut self, key: ResourceKey) -> (usize, bool) {
        let reference = self.references.get_mut(&key).unwrap();
        let full = reference.full && !reference.sent;
        reference.sent = true;
        (reference.id, full)
    }
    fn source(&mut self, node: &OhSdkPluginNodeSnapshot) -> Value {
        let (id, full) = self.reference(ResourceKey::Source(node.file_id));
        let mut value = json!({ "fileName": node.file_name, "sourceId": id, "pos": node.pos, "end": node.end, "start": node.start });
        if let Some(text) = &node.text { value["text"] = json!(text); }
        if full { value["source"] = json!(node.source); }
        value
    }
    fn args(&mut self, node: &OhSdkPluginNodeSnapshot, declaration: &OhSdkPluginNodeSnapshot, index: usize, config: &ProjectConfig) -> Vec<Value> {
        let (id, full) = self.reference(ResourceKey::Config(index));
        let mut request = json!({ "node": self.source(node), "declaration": self.source(declaration), "projectConfigId": id });
        if full { request["projectConfig"] = json!(config); }
        vec![request]
    }
    fn complete(&self, result: &Result<Vec<OhSdkPluginResponse>, String>) {
        let contents = result.as_ref().is_ok_and(|responses| responses.iter().all(|r| r.error.is_empty()));
        #[expect(clippy::iter_over_hash_type, reason = "independent immutable-ID acknowledgements; no output depends on their order")]
        for (&key, reference) in &self.references {
            let registered = if matches!(key, ResourceKey::Plugin(_)) { result.is_ok() } else { contents };
            // Any successful full publication acknowledges this immutable ID.
            // A later in-flight resend must not hide an earlier acknowledgement.
            if reference.full && registered { let _ = reference.publication.set(()); }
        }
    }
}

#[derive(Default)]
struct PluginState { next_id: u32, descriptors: FxHashMap<PluginKey, u32> }

/// The only JavaScript-facing boundary. Descriptor fields appear only on registration.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPluginRequest {
    pub session_id: u64,
    pub plugin_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Prior uncached check of this node; stop after its false/error response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<usize>,
    pub operation: String,
    pub args: Vec<Value>,
}

/// A batch registers all full descriptors/sources/configs before evaluating callbacks.
///
/// This includes registrations on short-circuited requests. Responses retain request order; an `after`
/// dependency stops on false/invoke error.
pub type SdkPluginCompletion = Box<dyn FnOnce(Result<Vec<OhSdkPluginResponse>, String>) + Send>;

pub trait SdkPluginHost: Send + Sync {
    fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String>;
    /// Event-loop hosts enqueue immediately and complete exactly once, including
    /// transport errors. Synchronous RPC hosts can execute on the calling thread.
    fn submit_batch(&self, requests: Vec<SdkPluginRequest>, complete: SdkPluginCompletion) {
        complete(self.invoke_batch(requests));
    }
}
struct ClientPluginHost(Arc<dyn ClientConn>);
impl SdkPluginHost for ClientPluginHost {
    fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
        let raw = self.0.call("ohSdkPluginBatch", &serde_json::to_string(&requests).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("invalid OH SDK plugin batch response: {e}"))
    }
}

pub struct SdkPluginExecutor {
    host: Arc<dyn SdkPluginHost>,
    session_id: u64,
    state: Mutex<SourceState>,
    configs: RwLock<ConfigTable>,
    plugins: Arc<RwLock<PluginState>>,
    function_results: ResultCache<FunctionKey>,
    syscap_results: Arc<SyscapResults>,
}
impl std::fmt::Debug for SdkPluginExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkPluginExecutor").field("session_id", &self.session_id).finish_non_exhaustive()
    }
}
impl SdkPluginExecutor {
    pub fn new(host: Arc<dyn SdkPluginHost>) -> Self {
        Self { host, session_id: SESSION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1,
            state: Mutex::default(), configs: RwLock::default(), plugins: Arc::default(),
            function_results: ResultCache::default(), syscap_results: Arc::default() }
    }
    pub(crate) fn from_connection(conn: Arc<dyn ClientConn>) -> Self { Self::new(Arc::new(ClientPluginHost(conn))) }

    fn config_index(&self, config: &Arc<ProjectConfig>) -> usize {
        if let Some((_, index)) = self.configs.read().unwrap().identities.get(&(Arc::as_ptr(config) as usize)) { return *index; }
        self.configs.write().unwrap().intern(Arc::clone(config))
    }
    fn plugin(&self, plugin: &OhSdkCheckPlugin) -> u32 {
        let key = (plugin.path.clone(), plugin.function_name.clone(), plugin.function_name.clone());
        if let Some(&id) = self.plugins.read().unwrap().descriptors.get(&key) { return id; }
        let mut state = self.plugins.write().unwrap();
        let PluginState { next_id, descriptors } = &mut *state;
        *descriptors.entry(key).or_insert_with(|| { *next_id += 1; *next_id })
    }
    fn prepare(&self, keys: &[ResourceKey]) -> PreparedBatch {
        self.state.lock().unwrap().prepare(keys)
    }

    fn request(&self, plugin: &OhSdkCheckPlugin, id: u32, prepared: &mut PreparedBatch, operation: &str, args: Vec<Value>, after: Option<usize>) -> SdkPluginRequest {
        let (_, full) = prepared.reference(ResourceKey::Plugin(id));
        SdkPluginRequest {
            session_id: self.session_id, plugin_id: id,
            path: full.then(|| plugin.path.clone()), function_name: full.then(|| plugin.function_name.clone()),
            class_name: full.then(|| plugin.function_name.clone()), operation: operation.into(), args, after,
        }
    }
    fn invoke(&self, requests: Vec<SdkPluginRequest>, prepared: &PreparedBatch, plugins: &[(OhSdkCheckPlugin, u32)]) -> Result<Vec<OhSdkPluginResponse>, String> {
        let count = requests.len();
        let result = self.host.invoke_batch(requests);
        Self::complete(&self.plugins, prepared, plugins, count, result)
    }
    fn complete(state: &RwLock<PluginState>, prepared: &PreparedBatch, plugins: &[(OhSdkCheckPlugin, u32)],
        count: usize, result: Result<Vec<OhSdkPluginResponse>, String>) -> Result<Vec<OhSdkPluginResponse>, String> {
        let result = result.and_then(|responses| {
            if responses.len() != count { return Err(format!("invalid OH SDK plugin batch response: received {} responses for {count} requests", responses.len())); }
            Ok(responses)
        });
        if result.is_err() {
            let mut state = state.write().unwrap();
            for (plugin, id) in plugins {
                if !prepared.references.get(&ResourceKey::Plugin(*id)).is_some_and(|r| r.full) { continue; }
                let key = (plugin.path.clone(), plugin.function_name.clone(), plugin.function_name.clone());
                if state.descriptors.get(&key) == Some(id) { state.descriptors.remove(&key); }
            }
        }
        // A completed publication is acknowledged before any other batch can
        // reference its IDs. No N-API TSFN FIFO assumption is required; neither
        // the host round trip nor publication waits hold the registration lock.
        prepared.complete(&result);
        result
    }
    fn dispatch(&self, plugin: &OhSdkCheckPlugin, id: u32, operation: &str, args: Vec<Value>) -> PluginResult {
        let mut prepared = self.prepare(&[ResourceKey::Plugin(id)]);
        let request = self.request(plugin, id, &mut prepared, operation, args, None);
        let mut responses = self.invoke(vec![request], &prepared, &[(plugin.clone(), id)])?;
        Ok(responses.pop().unwrap())
    }
    fn interpret(plugin: &OhSdkCheckPlugin, operation: &str, result: PluginResult) -> PluginResult {
        let response = match result {
            Ok(response) => response,
            Err(_) if matches!(operation, "value" | "format" | "prepareClass") => return Ok(OhSdkPluginResponse::default()),
            Err(error) => return Err(error),
        };
        if !response.error.is_empty() {
            if matches!(operation, "value" | "format") || operation == "syscap" && response.phase == "load"
                || operation == "prepareClass" && !response.found { return Ok(OhSdkPluginResponse::default()); }
            return Err(format!("SDK plugin {}#{} {} failed: {}", plugin.path, plugin.function_name, response.phase, response.error));
        }
        Ok(response)
    }
}
impl OhSdkPluginExecutor for SdkPluginExecutor {
    fn call(&self, plugin: &OhSdkCheckPlugin, operation: &str, args: Vec<Value>) -> PluginResult {
        let registration = self.plugin(plugin);
        let result = if let Some(kind) = FunctionOperation::parse(operation) {
            let key = FunctionKey { plugin: registration, operation: kind, args: args.iter().map(ArgumentKey::new).collect() };
            self.function_results.invoke(&key, || self.dispatch(plugin, registration, operation, args))
        } else { self.dispatch(plugin, registration, operation, args) };
        Self::interpret(plugin, operation, result)
    }
    fn submit_syscap_batch(&self, plugins: &[OhSdkClassCheckPlugin], nodes: Vec<(OhSdkPluginNodeSnapshot, OhSdkPluginNodeSnapshot)>,
        config: Arc<ProjectConfig>) -> OhSdkPendingSyscap {
        let (sender, reply) = OhSdkPendingSyscap::channel();
        let node_count = nodes.len();
        if plugins.is_empty() { let _ = sender.send(vec![Ok(None); node_count]); return reply; }
        let config_index = self.config_index(&config);
        let plugins: Vec<_> = plugins.iter().map(|plugin| {
            let descriptor = OhSdkCheckPlugin { path: plugin.path.clone(), function_name: plugin.class_name.clone(), ..Default::default() };
            let id = self.plugin(&descriptor);
            (descriptor, id)
        }).collect();
        let keys: Vec<_> = nodes.iter().flat_map(|(node, declaration)| plugins.iter().map(move |(_, id)| SyscapKey {
            plugin: *id, node: NodeKey::new(node), declaration: NodeKey::new(declaration), config: config_index,
        })).collect();
        let reservation = self.syscap_results.reserve(&keys);
        let cached: Vec<_> = {
            let values = self.syscap_results.values.read().unwrap();
            keys.iter().map(|key| values.get(key).cloned()).collect()
        };
        let mut indices = FxHashMap::default();
        let mut pending = Vec::new();
        for node in 0..nodes.len() {
            let mut after = None;
            for plugin in 0..plugins.len() {
                let index = node * plugins.len() + plugin;
                if let Some(result) = &cached[index] {
                    if result.found && !result.check_result { break; }
                } else {
                    let key = &keys[index];
                    let next = *indices.entry(key.clone()).or_insert_with(|| {
                        let next = pending.len();
                        pending.push((index, after));
                        next
                    });
                    after = Some(next);
                }
            }
        }
        let (prepared, requests) = if !pending.is_empty() {
            let mut resources = vec![ResourceKey::Config(config_index)];
            for &(index, _) in &pending {
                let key = &keys[index];
                resources.extend([ResourceKey::Plugin(key.plugin), ResourceKey::Source(key.node.file), ResourceKey::Source(key.declaration.file)]);
            }
            let mut prepared = self.prepare(&resources);
            let requests = pending.iter().map(|&(index, after)| {
                let (node, declaration) = &nodes[index / plugins.len()];
                let (plugin, id) = &plugins[index % plugins.len()];
                let args = prepared.args(node, declaration, config_index, &config);
                self.request(plugin, *id, &mut prepared, "syscap", args, after)
            }).collect();
            (Some(prepared), requests)
        } else { (None, Vec::new()) };
        // No arena-backed snapshots cross this boundary. Completion publishes
        // caches and releases reservations on the host reply, not on waiter drain
        // (a later identical call must not wait for its own checker's drain).
        let plugin_state = Arc::clone(&self.plugins);
        let count = requests.len();
        let complete: SdkPluginCompletion = Box::new(move |results| {
            let results = match prepared {
                Some(prepared) => Self::complete(&plugin_state, &prepared, &plugins, count, results),
                None => results,
            };
            if let Ok(responses) = &results {
                let mut values = reservation.cache.values.write().unwrap();
                for (&(index, _), response) in pending.iter().zip(responses) {
                    if response.error.is_empty() && !response.skipped { values.insert(keys[index].clone(), response.clone()); }
                }
            }
            // Fold the original plugin chain locally, mixing cache hits and batch
            // responses. Cached false results also stop later uncached requests.
            let folded = (0..node_count).map(|node| {
                let mut result = None;
                for (plugin, (descriptor, _)) in plugins.iter().enumerate() {
                    let index = node * plugins.len() + plugin;
                    let current = if let Some(cached) = &cached[index] { cached.clone() }
                    else { Self::interpret(descriptor, "syscap", results.as_ref().map(|responses| responses[indices[&keys[index]]].clone()).map_err(Clone::clone))? };
                    if current.skipped { return Err("SDK syscap host skipped a reachable callback".into()); }
                    if !current.found { continue; }
                    let stop = !current.check_result;
                    result = Some(current);
                    if stop { break; }
                }
                Ok(result)
            }).collect();
            drop(reservation);
            let _ = sender.send(folded);
        });
        if requests.is_empty() { complete(Ok(Vec::new())); }
        else { self.host.submit_batch(requests, complete); }
        reply
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
