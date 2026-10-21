//! OH SDK declaration-use checks (`checker/oh_sdk_jsdoc.go`).
#![expect(clippy::unused_self, reason = "ported Go receiver methods keep their receiver")]
use crate::*;
use crate::oh_sdk_api::*;
use tsrs_core::tspath;
use tsrs_diagnostics::Category;
use std::sync::LazyLock;
use std::collections::VecDeque;
use tsrs_core::OhSdkPendingSyscap;
use regex::Regex;
const OH_SINCE_WARNING: &str = "The '{0}' API is supported since SDK version $SINCE1. However, the current compatible SDK version is $SINCE2.\n It is recommended to use apiAvailable to safeguard API compatibility.";
const OH_AVAILABLE_WARNING: &str = "The '{0}' API is available since SDK version $SINCE1. However, the current compatible SDK version is $SINCE2.\n It is recommended to use apiAvailable to safeguard API compatibility.";
const OH_PERMISSION_WARNING: &str = "To use this API, you need to apply for the permissions: $DT";
const OH_SYSTEM_API_WARNING: &str = "'{0}' is system api";
const OH_TEST_WARNING: &str = "'{0}' can only be used for testing directories ";
const OH_SYSCAP_WARNING: &str = "The system capacity of this api '{0}' is not supported on all devices";
const OH_DEPRECATED_WARNING: &str = "'{0}' has been deprecated.";
const OH_FORM_ERROR: &str = "11706006#'{0}' can't support form application.";
const OH_CROSSPLATFORM_ERROR: &str = "11706007#'{0}' can't support crossplatform application.";
const OH_FA_MODEL_ERROR: &str = "11706008#This API is used only in FA Mode, but the current Mode is Stage.";
const OH_STAGE_MODEL_ERROR: &str = "11706009#This API is used only in Stage Mode, but the current Mode is FA.";
const OH_ATOMIC_SERVICE_ERROR: &str = "11706010#'{0}' can't support atomicservice application.";
const OH_FIND_MODULE_WARNING: &str = "Cannot find name '{0}'.";
const OH_DEFAULT_SDK_MARKING: &str = "This API has been Special Markings. exercise caution when using this API.";
static OH_SINCE_FORMAT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[1-9][0-9]{0,2}|[1-9][0-9]{0,2}\.[0-9]{1,3}\.[0-9]{1,3}|[1-9][0-9]{0,2}\.[0-9]{1,3}\.[0-9]{1,3}\([1-9][0-9]{0,2}\)|[1-9][0-9]?\.[0-9]{1,2}\.[0-9]{1,2})$").unwrap());
static OH_VERSION_RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[since (.*?)\]").unwrap());
static OH_INVALID_SUPPRESS_COMMENT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^//[\t\n\f\r ]*@SuppressWarnings[\t\n\f\r ]*(/+)").unwrap());
static OH_SUPPRESS_COMMENT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"//[\t\n\f\r ]*@SuppressWarnings[\t\n\f\r ]").unwrap());

#[derive(Clone)]
pub struct OhSdkUseFact {
    pub file_name: String,
    pub api_module: String,
    pub function: String,
}
#[derive(Default)]
pub(crate) struct OhSdkState {
    pub enabled: bool,
    uses: FxHashMap<String, std::collections::BTreeMap<String, OhSdkUseFact>>,
    files: FxHashMap<ast::NodeId, OhSdkFileFacts>,
    available_checks: FxHashSet<String>,
    pending_syscap: Vec<PendingSyscapUse>,
    pipeline_syscap: bool,
    in_flight_syscap: VecDeque<PendingSyscapBatch>,
}
struct PendingSyscapUse { node: P<Node>, declaration: P<Node>, comment: String, suppressed: Option<bool> }
struct PendingSyscapBatch { uses: Vec<PendingSyscapUse>, reply: OhSdkPendingSyscap }
// One batch can execute in JS while this checker prepares the next file. Two
// batches per checker bound serialized transport and retained per-use state;
// enlarging the program does not enlarge this window.
const MAX_IN_FLIGHT_SYSCAP_BATCHES: usize = 2;
impl OhSdkState {
    fn file_facts(&mut self, file: P<SourceFile>, options: &CompilerOptions) -> &OhSdkFileFacts {
        self.files.entry(ast::get_node_id(file.as_node()))
            .or_insert_with(|| OhSdkFileFacts::new(file, options))
    }
}
// These are the original OH declaration-use predicates, evaluated once for
// each immutable SourceFile ID, never for a path that can acquire new contents.
struct OhSdkFileFacts {
    normalized: String,
    base: String,
    api_module: String,
    system: bool,
    project: bool,
    project_available: bool,
    sdk: bool,
    card: bool,
    ohos_test: bool,
    ts_implementation: bool,
    arkui: bool,
    device_info: bool,
    whitelist: &'static [&'static str],
}
impl OhSdkFileFacts {
    fn new(file: P<SourceFile>, o: &CompilerOptions) -> Self {
        let normalized = tspath::normalize_path(file.file_name());
        let base = tspath::get_base_file_name(file.file_name());
        let normalized_base = tspath::get_base_file_name(&normalized);
        let api_module = normalized_base.strip_suffix(".d.ets").unwrap_or(&normalized_base);
        let api_module = api_module.strip_suffix(".d.ts").unwrap_or(api_module).to_string();
        let root = if o.oh_project_root_path.is_empty() { &o.oh_project_path } else { &o.oh_project_root_path };
        let project = !root.is_empty() && normalized.starts_with(&tspath::normalize_path(root));
        let under = |root: &String| !root.is_empty() && normalized.starts_with(&(tspath::normalize_path(root) + "/"));
        let sdk = o.oh_all_module_paths.as_deref().unwrap_or_default().iter().any(|p| tspath::normalize_path(p) == normalized)
            || o.oh_sdk_configs.as_deref().unwrap_or_default().iter().any(|c| c.api_paths.iter().any(under))
            || o.oh_external_api_paths.as_deref().unwrap_or_default().iter().any(under)
            || o.oh_ark_ui_declaration_dirs.as_deref().unwrap_or_default().iter().any(under)
            || !o.ets_loader_path.is_empty() && under(&tspath::resolve_path(&o.ets_loader_path, &["declarations"]));
        let directory = tspath::normalize_path(&tspath::get_directory_path(&normalized));
        let arkui = !matches!(normalized_base.as_str(), "common_ts_ets_api.d.ts" | "global.d.ts")
            && (o.oh_ark_ui_declaration_dirs.as_deref().unwrap_or_default().iter().any(|d| directory == tspath::normalize_path(d))
                || !o.ets_loader_path.is_empty() && directory == tspath::normalize_path(&tspath::resolve_path(&o.ets_loader_path, &["declarations"])));
        let mut whitelist: &'static [&'static str] = &[];
        for root in o.oh_global_module_paths.as_deref().unwrap_or_default() {
            let root = tspath::normalize_path(root);
            match normalized.strip_prefix(&(root.trim_end_matches('/').to_string() + "/")) {
                Some("@arkts.lang.d.ets") => { whitelist = &["RetentionPolicy", "Retention", "SOURCE", "BYTECODE"]; break; }
                Some("@ohos.deviceInfo.d.ts") => { whitelist = &["apiAvailable"]; break; }
                _ => {}
            }
        }
        Self {
            system: o.oh_system_modules.as_deref().unwrap_or_default().contains(&base),
            project, project_available: project && file.text().contains("@Available"), sdk,
            card: o.oh_card_entry_files.as_deref().unwrap_or_default().iter().any(|p| tspath::normalize_path(p) == normalized),
            ohos_test: !o.oh_module_path.is_empty() && normalized.starts_with(&tspath::resolve_path(&o.oh_module_path, &["src", "ohosTest"])),
            ts_implementation: std::path::Path::new(&normalized)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
                && !normalized.ends_with(".d.ts"),
            arkui, device_info: file.text().contains("deviceInfo"), whitelist,
            normalized, base, api_module,
        }
    }
}
struct Tag { name: String, comment: String }
struct Contract { tags: Vec<Tag> }
impl Contract {
    fn new(declaration: P<Node>) -> Self {
        Self { tags: crate::jsdoc::get_all_jsdoc_tags(declaration).into_iter().map(|tag| Tag {
            name: tag.tag_name().text().to_string(), comment: tsrs_scanner::get_text_of_jsdoc_comment(tag.comment_list()),
        }).collect() }
    }
    fn first(&self, names: &[&str]) -> Option<&Tag> { self.tags.iter().find(|tag| names.contains(&tag.name.as_str())) }
}
impl OhSdkState { pub(crate) fn new() -> Self { Self { enabled: true, ..Default::default() } } }
impl Checker {
    pub fn oh_sdk_use_facts(&self, file_name: &str) -> Vec<OhSdkUseFact> {
        self.oh_sdk.uses.get(&tspath::normalize_path(file_name)).map(|facts| facts.values().cloned().collect()).unwrap_or_default()
    }
    pub fn reset_oh_sdk_jsdoc_check(&mut self) { self.oh_sdk.enabled = false; }
    pub(crate) fn check_oh_sdk_identifier_use(&mut self, node: P<Node>, symbol: P<Symbol>) {
        if !self.oh_sdk.enabled || node.is_virtual() || symbol.value_declaration().is_none() { return; }
        for &declaration in symbol.declarations() { self.check_oh_sdk_declaration_use(node, declaration); }
    }
    pub(crate) fn check_oh_sdk_property_use(&mut self, node: P<Node>, symbol: P<Symbol>) {
        if !self.oh_sdk.enabled || symbol.value_declaration().is_none() || symbol.declarations().len() >= 2 { return; }
        for &declaration in symbol.declarations() { self.check_oh_sdk_declaration_use(node, declaration); }
    }
    pub(crate) fn check_oh_sdk_overload_use(&mut self, node: P<Node>, declaration: Option<P<Node>>) {
        if !self.oh_sdk.enabled { return; }
        let Some(declaration) = declaration else { return; };
        let Some(expression) = node.expression().filter(|e| ast::is_property_access_expression(*e)) else { return; };
        let Some(symbol) = self.get_resolved_symbol_or_nil(expression) else { return; };
        if symbol.declarations().len() < 2 { return; }
        self.check_oh_sdk_declaration_use(expression.name().unwrap(), declaration);
    }
    fn check_oh_sdk_declaration_use(&mut self, node: P<Node>, declaration: P<Node>) {
        let use_file = ast::get_source_file_of_node(node).unwrap();
        let decl_file = ast::get_source_file_of_node(declaration).unwrap();
        if self.oh_sdk_file_facts(use_file).system { return; }
        let project_available = self.oh_sdk_file_facts(decl_file).project_available;
        let sdk = self.oh_sdk_file_facts(decl_file).sdk;
        if !project_available && !sdk { return; }
        if !project_available && !(sdk && (self.oh_sdk_file_facts(use_file).card || self.compiler_options.oh_crossplatform == Tristate::True
            || !self.compiler_options.oh_compile_mode.is_empty() || self.compiler_options.oh_bundle_type == "atomicService" && self.oh_compile_sdk_version() >= 11)) { return; }
        if sdk && self.compiler_options.oh_crossplatform == Tristate::True { self.record_oh_sdk_use(node, declaration); }
        if self.oh_sdk_file_facts(use_file).ts_implementation && self.oh_sdk_file_facts(decl_file).arkui {
            self.add_oh_sdk_use_diagnostic(node, OH_FIND_MODULE_WARNING, Category::Warning);
        }
        let contract = Contract::new(declaration);
        if project_available { self.check_oh_available_use(node, declaration, &contract); return; }
        self.check_oh_since_use(node, declaration, &contract);
        self.check_oh_syscap_use(node, declaration, &contract);
        self.check_oh_permission_use(node, &contract);
        self.check_oh_presence_tags(node, &contract);
    }
    fn record_oh_sdk_use(&mut self, node: P<Node>, declaration: P<Node>) {
        let function = sdk_api_path(declaration);
        if function.is_empty() { return; }
        let file_name = self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).normalized.clone();
        let api_module = self.oh_sdk_file_facts(ast::get_source_file_of_node(declaration).unwrap()).api_module.clone();
        let key = format!("{api_module}\0{function}");
        let fact = OhSdkUseFact { file_name: file_name.clone(), api_module, function };
        self.oh_sdk.uses.entry(file_name).or_default().insert(key, fact);
    }
    fn oh_sdk_file_facts(&mut self, file: P<SourceFile>) -> &OhSdkFileFacts {
        self.oh_sdk.file_facts(file, &self.compiler_options)
    }
    fn add_oh_sdk_use_diagnostic(&mut self, node: P<Node>, message: &str, category: Category) {
        self.add_oh_sdk_diagnostic(node, &OhSdkCheckResult { valid: false, message: message.replace("{0}", &tsrs_scanner::get_text_of_node(node)), category });
    }
    fn oh_compatible_sdk_version(&self) -> String {
        if !self.compiler_options.oh_origin_compatible_sdk_version.is_empty() { return self.compiler_options.oh_origin_compatible_sdk_version.clone(); }
        self.compiler_options.compatible_sdk_version.map(|v| v.to_string()).unwrap_or_default()
    }
    fn oh_compile_sdk_version(&self) -> i32 { self.compiler_options.compile_sdk_version.unwrap_or_default() as i32 }
    fn oh_has_configured_compatible_sdk(&self) -> bool { self.compiler_options.compatible_sdk_version.is_some_and(|v| v != 0.0) }
    fn oh_version_range_intersects_compile_sdk(&self, comment: &str) -> bool {
        let Some((start, end)) = version_range(comment) else { return false; };
        let start = self.parse_oh_range_version(start); let end = self.parse_oh_range_version(end);
        let sdk = self.parse_oh_range_version(&self.oh_compile_sdk_version().to_string());
        sdk >= start.min(end) && sdk <= start.max(end)
    }
    fn parse_oh_range_version(&self, version: &str) -> i32 {
        let version = version.trim();
        if let Some(m) = self.oh_sdk_match_build_version("since", version) {
            if m.matched && m.groups.len() > 4 { return parse_decimal(&m.groups[4]) * 10000; }
        }
        let parts: Vec<_> = version.split('.').collect();
        if !parts.iter().all(|v| !v.is_empty() && v.len() <= 2 && v.bytes().all(|c| c.is_ascii_digit())) { return 0; }
        match parts.as_slice() { [a] => parse_decimal(a) * 10000, [a,b,c] => parse_decimal(a)*10000 + parse_decimal(b)*100 + parse_decimal(c), _ => 0 }
    }
    fn oh_sdk_category(&self) -> Category { if self.compiler_options.oh_api_compatibility_check == "error" { Category::Error } else { Category::Warning } }
    fn check_oh_since_use(&mut self, node: P<Node>, declaration: P<Node>, contract: &Contract) {
        let Some(tag) = contract.first(&["since"]) else { return; };
        let compatible = self.oh_compatible_sdk_version(); let version = tag.comment.trim();
        if !self.oh_has_configured_compatible_sdk() || compatible.is_empty() || version.is_empty() || !OH_SINCE_FORMAT_PATTERN.is_match(version)
            || !self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).project { return; }
        let compatible_result = self.oh_sdk_check_value("since", version, &compatible, 0)
            .map_or_else(|| compare_oh_point_versions(&compatible, version) >= 0, |r| r.result);
        if compatible_result || self.oh_suppresses_sdk_warning(node, Some(declaration), "since", version, None) { return; }
        let mut message = OH_SINCE_WARNING.replace("$SINCE1", version).replace("$SINCE2", &compatible);
        if self.oh_sdk_category() == Category::Error { message = "11706011#".to_string() + &message; }
        self.add_oh_sdk_use_diagnostic(node, &message, self.oh_sdk_category());
    }
    fn check_oh_syscap_use(&mut self, node: P<Node>, declaration: P<Node>, contract: &Contract) {
        if self.compiler_options.oh_device_types.as_deref().unwrap_or_default().is_empty() { return; }
        let comment = contract.first(&["syscap"]).map_or("", |t| t.comment.as_str());
        if !self.compiler_options.oh_syscap_intersection.as_deref().unwrap_or_default().iter().any(|s| s == comment) {
            if !self.oh_suppresses_sdk_warning(node, None, "syscap", comment, None) { self.add_oh_sdk_use_diagnostic(node, OH_SYSCAP_WARNING, Category::Warning); }
            return;
        }
        if self.compiler_options.oh_sdk_plugin_executor.is_none() { return; }
        self.oh_sdk.pending_syscap.push(PendingSyscapUse { node, declaration, comment: comment.into(), suppressed: None });
        // Language-service queries outside a file check still need their result
        // before returning; normal checks flush in the existing file lifecycle.
        if self.checking_file.is_none() { self.flush_oh_syscap_checks(); }
    }
    pub(crate) fn flush_oh_syscap_checks(&mut self) {
        let mut pending = std::mem::take(&mut self.oh_sdk.pending_syscap);
        if pending.is_empty() { return; }
        let options = self.compiler_options;
        let plugins: Vec<_> = options.oh_sdk_class_check_plugins.as_deref().unwrap_or_default().iter()
            .filter(|plugin| plugin.tag_name == "syscap").cloned().collect();
        let nodes = pending.iter().map(|use_| (Self::oh_sdk_node_snapshot(use_.node), Self::oh_sdk_node_snapshot(use_.declaration))).collect();
        if self.oh_sdk.pipeline_syscap {
            // Freeze suppression while still in the originating file's checking
            // context, before any later file can affect checker query state.
            for use_ in &mut pending {
                use_.suppressed = Some(self.oh_suppresses_sdk_warning(use_.node, None, "syscap", &use_.comment, None));
            }
            if self.oh_sdk.in_flight_syscap.len() == MAX_IN_FLIGHT_SYSCAP_BATCHES { self.drain_oh_syscap_batch(); }
        }
        let reply = options.oh_sdk_plugin_executor.as_ref().unwrap().submit_syscap_batch(
            &plugins, nodes, options.oh_sdk_plugin_config_cache.get(&options));
        self.oh_sdk.in_flight_syscap.push_back(PendingSyscapBatch { uses: pending, reply });
        if !self.oh_sdk.pipeline_syscap { self.drain_oh_syscap_batch(); }
    }
    /// Used only inside an owned checker-group iteration. Call finish before
    /// reading per-file diagnostics, returning the checker, or retiring its region.
    pub fn begin_oh_syscap_pipeline(&mut self) { self.oh_sdk.pipeline_syscap = true; }
    pub fn finish_oh_syscap_pipeline(&mut self) {
        while !self.oh_sdk.in_flight_syscap.is_empty() { self.drain_oh_syscap_batch(); }
        self.oh_sdk.pipeline_syscap = false;
    }
    fn drain_oh_syscap_batch(&mut self) {
        let PendingSyscapBatch { uses: pending, reply } = self.oh_sdk.in_flight_syscap.pop_front().unwrap();
        let results = reply.wait();
        assert_eq!(results.len(), pending.len(), "SDK syscap batch response count");
        // api_check_utils::checkSyscapAbility only contributes warnings. Preserve
        // discovery order here; the existing DiagnosticsCollection sorts/dedups.
        for (use_, result) in pending.into_iter().zip(results) {
            let Some(result) = result.unwrap_or_else(|e| panic!("OH SDK syscap checker: {e}")) else { continue; };
            if !result.check_result || use_.suppressed.unwrap_or_else(|| self.oh_suppresses_sdk_warning(use_.node, None, "syscap", &use_.comment, None)) { continue; }
            let message = if result.check_message.is_empty() { OH_DEFAULT_SDK_MARKING } else { &result.check_message };
            self.add_oh_sdk_use_diagnostic(use_.node, message, Category::Warning);
        }
    }
    fn check_oh_permission_use(&mut self, node: P<Node>, contract: &Contract) {
        let mut missing = Vec::new();
        for tag in contract.tags.iter().filter(|t| t.name == "permission") {
            let mut comment = tag.comment.clone();
            if version_range(&comment).is_some() {
                if !self.oh_version_range_intersects_compile_sdk(&comment) { continue; }
                comment = OH_VERSION_RANGE_PATTERN.replace_all(&comment, "").trim().to_string();
            }
            if comment.is_empty() || (PermissionExpression { source: &comment, granted: self.compiler_options.oh_request_permissions.as_deref().unwrap_or_default() }).valid() { continue; }
            if self.oh_suppresses_sdk_warning(node, None, "permission", "", None) { continue; }
            missing.push(comment);
        }
        if !missing.is_empty() { self.add_oh_sdk_use_diagnostic(node, &OH_PERMISSION_WARNING.replacen("$DT", &missing.join(" and "), 1), Category::Warning); }
    }
    fn check_oh_presence_tags(&mut self, node: P<Node>, contract: &Contract) {
        if contract.first(&["deprecated"]).is_some() { self.add_oh_sdk_use_diagnostic(node, OH_DEPRECATED_WARNING, Category::Warning); }
        let applicable = |tag: &Tag| !tag.comment.contains("[since ") || self.oh_version_range_intersects_compile_sdk(&tag.comment);
        let system = contract.first(&["systemapi"]).is_some_and(applicable);
        let test = contract.first(&["test"]).is_some_and(applicable);
        if system { self.add_oh_sdk_use_diagnostic(node, OH_SYSTEM_API_WARNING, Category::Warning); }
        if test && !self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).ohos_test { self.add_oh_sdk_use_diagnostic(node, OH_TEST_WARNING, Category::Warning); }
        let card = self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).card;
        self.check_oh_required_tag(node, contract, "form", card, OH_FORM_ERROR, Category::Error);
        self.check_oh_required_tag(node, contract, "crossplatform", self.compiler_options.oh_crossplatform == Tristate::True, OH_CROSSPLATFORM_ERROR,
            if self.compiler_options.oh_ignore_crossplatform_check == Tristate::True { Category::Warning } else { Category::Error });
        self.check_oh_required_tag(node, contract, "atomicservice", self.compiler_options.oh_bundle_type == "atomicService" && self.oh_compile_sdk_version() >= 11, OH_ATOMIC_SERVICE_ERROR, Category::Error);
        let (tags, message): (&[&str], &str) = if self.compiler_options.oh_compile_mode == "moduleJson" { (&["famodelonly", "FAModelOnly"], OH_FA_MODEL_ERROR) }
            else if !self.compiler_options.oh_compile_mode.is_empty() { (&["stagemodelonly", "StageModelOnly"], OH_STAGE_MODEL_ERROR) } else { return; };
        if contract.first(tags).is_some_and(|t| !t.comment.contains("[since ") || self.oh_version_range_intersects_compile_sdk(&t.comment)) {
            self.add_oh_sdk_use_diagnostic(node, message, Category::Error);
        }
    }
    fn check_oh_required_tag(&mut self, node: P<Node>, contract: &Contract, tag: &str, enabled: bool, message: &str, category: Category) {
        if enabled && contract.first(&[tag]).is_none_or(|t| t.comment.contains("[since ") && !self.oh_version_range_intersects_compile_sdk(&t.comment)) {
            self.add_oh_sdk_use_diagnostic(node, message, category);
        }
    }
    fn check_oh_available_use(&mut self, node: P<Node>, declaration: P<Node>, contract: &Contract) {
        let compatible = self.oh_compatible_sdk_version();
        if !self.oh_has_configured_compatible_sdk() || !self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).project
            || !self.oh_sdk_file_facts(ast::get_source_file_of_node(declaration).unwrap()).project { return; }
        let key = format!("{}::{}::{}", self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).base, node.pos(), node.end());
        if !self.oh_sdk.available_checks.insert(key) { return; }
        let Some(version) = self.oh_available_decorator_version(declaration) else { return; };
        if compatible.is_empty() { return; }
        if self.oh_sdk_check_value("available", &version.format_version, &compatible, 0).map_or_else(|| compare_oh_point_versions(&compatible, &version.version) >= 0, |r| r.result) { return; }
        if self.oh_suppresses_sdk_warning(node, Some(declaration), "available", &version.version, Some(&version)) || contract.first(&["since"]).is_some() { return; }
        let mut message = OH_AVAILABLE_WARNING.replace("$SINCE1", &version.version).replace("$SINCE2", &compatible);
        if self.oh_sdk_category() == Category::Error { message = "11706012#".to_string() + &message; }
        self.add_oh_sdk_use_diagnostic(node, &message, self.oh_sdk_category());
    }
    fn oh_available_decorator_version(&self, node: P<Node>) -> Option<OhParsedVersion> {
        for current in ancestors_including(node) {
            for &modifier in current.modifier_nodes() {
                let Some(expr) = decorator_call(modifier, "Available") else { continue; };
                if expr.arguments().is_empty() || !ast::is_object_literal_expression(expr.arguments()[0]) { continue; }
                for &property in expr.arguments()[0].properties() {
                    if !ast::is_property_assignment(property) || property.name().and_then(ast::try_get_text_of_property_name).as_deref() != Some("minApiVersion") { continue; }
                    let Some(value) = property.initializer().filter(|v| ast::is_string_literal(*v) || ast::is_numeric_literal(*v)) else { continue; };
                    let version = parse_oh_available_version(value.text());
                    if self.validate_oh_available_version(&version).valid { return Some(version); }
                }
            }
        }
        None
    }
}
fn ancestors_including(node: P<Node>) -> impl Iterator<Item=P<Node>> { std::iter::successors(Some(node), |node| node.parent()) }
fn decorator_call(node: P<Node>, name: &str) -> Option<P<Node>> {
    if !ast::is_decorator(node) { return None; }
    node.expression().filter(|e| ast::is_call_expression(*e) && e.expression().is_some_and(|c| ast::is_identifier(c) && c.text() == name))
}
fn version_range(comment: &str) -> Option<(&str, &str)> {
    let m = OH_VERSION_RANGE_PATTERN.captures(comment)?;
    let mut parts = m.get(1)?.as_str().split('-');
    let first = parts.next()?.trim(); let second = parts.next()?.trim();
    if parts.next().is_some() { None } else { Some((first, second)) }
}
fn sdk_api_path(declaration: P<Node>) -> String {
    let mut parts = Vec::new();
    for node in ancestors_including(declaration) {
        if !matches!(node.kind(), Kind::VariableStatement | Kind::MethodDeclaration | Kind::MethodSignature | Kind::FunctionDeclaration
            | Kind::Constructor | Kind::ConstructSignature | Kind::CallSignature | Kind::PropertyDeclaration | Kind::PropertySignature
            | Kind::EnumMember | Kind::EnumDeclaration | Kind::TypeAliasDeclaration | Kind::ClassDeclaration | Kind::InterfaceDeclaration
            | Kind::ModuleDeclaration | Kind::GetAccessor | Kind::SetAccessor | Kind::IndexSignature) { break; }
        let name = match node.kind() {
            Kind::Constructor | Kind::ConstructSignature | Kind::CallSignature => "constructor".to_string(),
            Kind::VariableStatement => {
                let declarations = node.as_variable_statement().declaration_list.as_variable_declaration_list().declarations.nodes();
                if declarations.len() == 1 { declarations[0].name().map_or_else(|| "unnamed".into(), tsrs_scanner::get_text_of_node) } else { "unnamed".into() }
            }
            Kind::IndexSignature => "unnamed".into(),
            _ => node.name().map_or_else(|| "unnamed".into(), tsrs_scanner::get_text_of_node),
        };
        if !name.is_empty() { parts.push(name); }
    }
    parts.reverse(); parts.join("#")
}

impl Checker {
    fn oh_suppresses_sdk_warning(&mut self, node: P<Node>, declaration: Option<P<Node>>, warning: &str, value: &str, available: Option<&OhParsedVersion>) -> bool {
        if self.oh_suppress_warnings_annotation(node, warning) || self.oh_suppress_warnings_comment(node, warning) { return true; }
        match warning {
            "since" => self.oh_use_in_try(node) || self.oh_use_under_undefined_check(node) || declaration.is_some_and(|d| self.oh_sdk_whitelist(d))
                || self.oh_use_under_available(node, value, None) || self.oh_use_under_sdk_guard(node, value, None),
            "available" => self.oh_use_under_available(node, value, available) || self.oh_use_under_sdk_guard(node, value, available),
            "syscap" => self.oh_use_under_can_i_use(node, value),
            _ => false,
        }
    }
    fn oh_suppress_warnings_annotation(&self, node: P<Node>, warning: &str) -> bool {
        let rule = match warning { "since" | "available" => "SuppressWarningsType.COMPATIBILITY", "syscap" => "SuppressWarningsType.SYSCAP", "permission" => "SuppressWarningsType.PERMISSION", _ => return false };
        for current in ancestors_including(node) {
            let mut found = false;
            for &modifier in current.modifier_nodes() {
                let Some(expr) = decorator_call(modifier, "SuppressWarnings") else { continue; };
                if expr.arguments().is_empty() || !ast::is_object_literal_expression(expr.arguments()[0]) { continue; }
                found = true;
                let Some(&first) = expr.arguments()[0].properties().first() else { continue; };
                if !ast::is_property_assignment(first) { continue; }
                let Some(array) = first.initializer().filter(|v| ast::is_array_literal_expression(*v)) else { continue; };
                if array.elements().iter().any(|&e| rule.contains(&tsrs_scanner::get_text_of_node(e))) { return true; }
            }
            if found { return false; }
        }
        false
    }
    fn oh_suppress_warnings_comment(&self, node: P<Node>, warning: &str) -> bool {
        let warning = if matches!(warning, "since" | "available") { "compatibility" } else { warning };
        if !matches!(warning, "compatibility" | "syscap" | "permission") || !ast::is_identifier(node) { return false; }
        let file = ast::get_source_file_of_node(node).unwrap();
        let search = self.oh_sdk_find_comment_statement(node, warning);
        let search = self.oh_sdk_find_outer_chain(search, warning);
        let Some(found) = search.node else { return false; };
        let mut pos = found.pos();
        if search.is_chain {
            if let Some(parent) = search.chain_node.and_then(|n| n.parent()).filter(|p| ast::is_property_access_expression(*p)) {
                pos = parent.expression().unwrap().end();
            }
        }
        has_suppress_comment(file, pos.source_offset().unwrap_or(0), warning)
    }
    fn oh_sdk_find_comment_statement(&self, node: P<Node>, warning: &str) -> CommentSearch {
        let mut search = CommentSearch { node: Some(node), is_chain: false, chain_node: Some(node) };
        while let Some(current) = search.node {
            if ast::is_statement(current) || ast::is_property_declaration(current) || current.parent().is_none() { break; }
            if ast::is_property_access_expression(current) && current.expression().is_some_and(ast::is_call_expression) {
                search.is_chain = tsrs_scanner::get_text_of_node(current).contains("SuppressWarnings");
                if node_has_suppress_comment(current, warning) { search.chain_node = Some(current); break; }
            }
            search.node = current.parent();
        }
        search
    }
    fn oh_sdk_find_outer_chain(&self, mut search: CommentSearch, warning: &str) -> CommentSearch {
        let backup = search.node;
        if search.is_chain || search.node.and_then(|n| n.parent()).is_none_or(|p| !ast::is_block(p) || !p.parent().is_some_and(ast::is_arrow_function)) { return search; }
        while let Some(current) = search.node {
            let Some(parent) = current.parent() else { break; };
            if ast::is_arrow_function(current) && ast::is_call_expression(parent) && parent.expression().is_some_and(ast::is_property_access_expression) {
                if backup.is_some_and(|b| node_has_suppress_comment(b, warning)) { search.node = backup; break; }
                let expression = parent.expression().unwrap();
                search.is_chain = tsrs_scanner::get_text_of_node(expression).contains("SuppressWarnings");
                search.chain_node = expression.expression();
                search.node = expression.expression();
                break;
            }
            search.node = current.parent();
        }
        search
    }
    fn oh_use_in_try(&self, node: P<Node>) -> bool {
        ancestors_including(node).skip(1).any(|current| ast::is_try_statement(current) && node.pos() >= current.as_try_statement().try_block.pos())
    }
    fn oh_use_under_undefined_check(&self, node: P<Node>) -> bool {
        let target = primary_node_name(node);
        if target.is_empty() { return false; }
        for current in ancestors_including(node).skip(1) {
            if !ast::is_if_statement(current) { continue; }
            let Some(expression) = current.expression().filter(|e| ast::is_binary_expression(*e)) else { continue; };
            let expression = expression.as_binary_expression();
            if !matches!(expression.operator_token.kind(), Kind::ExclamationEqualsToken | Kind::ExclamationEqualsEqualsToken) { continue; }
            let left_undefined = ast::is_identifier(expression.left) && expression.left.text() == "undefined";
            let right_undefined = ast::is_identifier(expression.right.get()) && expression.right.get().text() == "undefined";
            if primary_node_name(expression.left) == target && right_undefined || left_undefined && primary_node_name(expression.right.get()) == target { return true; }
        }
        false
    }
    fn oh_sdk_whitelist(&mut self, declaration: P<Node>) -> bool {
        let Some(name) = declaration.name() else { return false; };
        self.oh_sdk_file_facts(ast::get_source_file_of_node(declaration).unwrap()).whitelist.contains(&name.text())
    }
    fn oh_use_under_available(&mut self, node: P<Node>, required: &str, available: Option<&OhParsedVersion>) -> bool {
        let file = ast::get_source_file_of_node(node).unwrap();
        if !self.oh_sdk_file_facts(file).project_available { return false; }
        let Some(version) = self.oh_available_decorator_version(node) else { return false; };
        let scene = if version.os == "OpenHarmony" { 1 } else { 2 };
        self.oh_sdk_check_value("available", available.map_or(required, |a| &a.format_version), &version.format_version, scene)
            .map_or_else(|| compare_oh_point_versions(&version.version, required) >= 0, |r| r.result)
    }
    fn oh_use_under_sdk_guard(&mut self, node: P<Node>, required: &str, available: Option<&OhParsedVersion>) -> bool {
        if !self.oh_sdk_file_facts(ast::get_source_file_of_node(node).unwrap()).device_info { return false; }
        for current in ancestors_including(node).skip(1) {
            if !ast::is_if_statement(current) { continue; }
            let then = current.as_if_statement().then_statement;
            if node.pos() < then.pos() || node.end() > then.end() { continue; }
            let condition = current.expression().unwrap();
            if self.oh_api_available_guard(condition, required) || self.oh_sdk_version_guard(condition, required, available) { return true; }
        }
        false
    }
    fn oh_api_available_guard(&self, condition: P<Node>, required: &str) -> bool {
        if !ast::is_call_expression(condition) || condition.arguments().len() != 1 { return false; }
        let text = tsrs_scanner::get_text_of_node(condition);
        let Some(api) = ["distributionOSApiVersion", "sdkApiVersion", "apiAvailable"].into_iter().find(|c| text.contains(c)) else { return false; };
        if self.oh_runtime_os() == "OpenHarmony" && api == "distributionOSApiVersion" { return false; }
        let argument = condition.arguments()[0];
        if !self.validate_oh_api_available_argument(argument).valid { return false; }
        let text = tsrs_scanner::get_text_of_node(argument).trim().replace(['\'', '"', '`', '|'], "");
        if self.oh_runtime_os() == "OpenHarmony" { return oh_available_format_matches(&text) && compare_oh_point_versions(&text, required) >= 0; }
        let mut distribution_required = required.to_string();
        if oh_msf_captures(required).is_some() && required.contains('(') {
            if let Some(converted) = self.oh_sdk_check_distribution("since", required).filter(|c| c.valid) { distribution_required = converted.version; }
        }
        if !text.contains('.') { return oh_available_format_matches(&text) && compare_oh_point_versions(&text, &distribution_required) >= 0; }
        let Some(major) = oh_msf_captures(&text) else { return false; };
        if major < 26 {
            if !self.oh_sdk_check_distribution("since", &text).is_some_and(|d| d.valid) { return false; }
            let scene = if api == "apiAvailable" { 1 } else { 2 };
            return self.oh_sdk_check_value("since", required, &text, scene).map_or_else(|| compare_oh_point_versions(&text, required) >= 0, |r| r.result);
        }
        compare_oh_point_versions(&text, &distribution_required) >= 0
    }
    fn oh_sdk_version_guard(&mut self, condition: P<Node>, required: &str, available: Option<&OhParsedVersion>) -> bool {
        let parts: Vec<_> = required.split('.').collect();
        if parts.len() >= 3 && parse_decimal(parts[0]) > 26 || !ast::is_binary_expression(condition) { return false; }
        let expression = condition.as_binary_expression();
        let (mut api_side, mut value_side, mut api_on_left) = (expression.left, expression.right.get(), true);
        let mut api_name = primary_node_name(api_side);
        if !matches!(api_name, "sdkApiVersion" | "distributionOSApiVersion") {
            (api_side, value_side, api_on_left) = (expression.right.get(), expression.left, false); api_name = primary_node_name(api_side);
        }
        if !matches!(api_name, "sdkApiVersion" | "distributionOSApiVersion") || api_name == "distributionOSApiVersion" && self.oh_runtime_os() == "OpenHarmony"
            || !ast::is_property_access_expression(api_side) || !self.is_oh_device_info_root(api_side.expression().unwrap()) { return false; }
        let Some(value) = self.oh_sdk_guard_value(value_side) else { return false; };
        let mut operator = expression.operator_token.kind();
        if !api_on_left {
            operator = match operator { Kind::GreaterThanToken => Kind::LessThanToken, Kind::LessThanToken => Kind::GreaterThanToken,
                Kind::GreaterThanEqualsToken => Kind::LessThanEqualsToken, Kind::LessThanEqualsToken => Kind::GreaterThanEqualsToken, _ => operator };
        }
        let assigned = match operator { Kind::GreaterThanToken => value + 1, Kind::GreaterThanEqualsToken | Kind::EqualsEqualsToken | Kind::EqualsEqualsEqualsToken => value, _ => return false };
        let assigned = assigned.to_string();
        if self.oh_runtime_os() != "OpenHarmony" {
            let scene = if api_name == "sdkApiVersion" { 1 } else { 2 };
            if let Some(result) = self.oh_sdk_check_value("since", available.map_or(required, |a| &a.format_version), &assigned, scene) { return result.result; }
        }
        compare_oh_point_versions(&assigned, required) >= 0
    }
    fn oh_sdk_guard_value(&mut self, node: P<Node>) -> Option<i32> {
        if ast::is_numeric_literal(node) { return node.text().parse::<f64>().ok().map(|v| (v + 0.999_999_999) as i32); }
        if ast::is_identifier(node) {
            if let Some(declaration) = self.get_resolved_symbol_or_nil(node).and_then(|s| s.value_declaration()).filter(|d| ast::is_variable_declaration(*d) || ast::is_binding_element(*d)) {
                let initializer = declaration.initializer()?;
                if ast::is_string_literal(initializer) {
                    if let Ok(value) = initializer.text().parse::<f64>() { return Some((value + 0.999_999_999) as i32); }
                }
                return self.oh_sdk_guard_value(initializer);
            }
        }
        None
    }
    fn is_oh_device_info_root(&mut self, mut node: P<Node>) -> bool {
        while ast::is_property_access_expression(node) { node = node.expression().unwrap(); }
        if !ast::is_identifier(node) { return false; }
        let Some(mut symbol) = self.get_resolved_symbol_or_nil(node) else { return false; };
        if symbol.flags().intersects(SymbolFlags::Alias) { symbol = self.resolve_alias(symbol); }
        symbol.declarations().iter().any(|&d| tspath::normalize_path(ast::get_source_file_of_node(d).unwrap().file_name()).to_lowercase().contains("@ohos.deviceinfo.d.ts"))
    }
    fn oh_use_under_can_i_use(&mut self, node: P<Node>, syscap: &str) -> bool {
        if !ast::get_source_file_of_node(node).unwrap().text().contains("canIUse(") { return false; }
        for current in ancestors_including(node).skip(1) {
            if !ast::is_if_statement(current) { continue; }
            let expr = current.expression().unwrap();
            if !ast::is_call_expression(expr) || !expr.expression().is_some_and(|e| ast::is_identifier(e) && e.text() == "canIUse") || expr.arguments().len() != 1 { continue; }
            let argument = expr.arguments()[0];
            if ast::is_string_literal(argument) && argument.text() == syscap { return true; }
            if ast::is_identifier(argument) {
                if self.get_resolved_symbol_or_nil(argument).and_then(|s| s.value_declaration()).filter(|d| ast::is_variable_declaration(*d))
                    .and_then(|d| d.initializer()).is_some_and(|v| ast::is_string_literal(v) && v.text() == syscap) { return true; }
            }
        }
        false
    }
}
struct CommentSearch { node: Option<P<Node>>, is_chain: bool, chain_node: Option<P<Node>> }
fn node_has_suppress_comment(node: P<Node>, warning: &str) -> bool {
    has_suppress_comment(
        ast::get_source_file_of_node(node).unwrap(),
        node.pos().source_offset().unwrap_or(0),
        warning,
    )
}
fn has_suppress_comment(file: P<SourceFile>, pos: u32, warning: &str) -> bool {
    let comments = tsrs_scanner::get_leading_comment_ranges(file.text(), pos);
    let texts: Vec<_> = comments.map(|c| &file.text()[c.pos() as usize..c.end() as usize]).collect();
    if texts.iter().any(|text| text.starts_with("/*") || OH_INVALID_SUPPRESS_COMMENT_PATTERN.is_match(text)) { return false; }
    texts.iter().any(|text| OH_SUPPRESS_COMMENT_PATTERN.is_match(text) && text.split([' ', '\t', '\r', '\n', ',']).any(|field| field == warning))
}
fn primary_node_name(node: P<Node>) -> &'static str {
    match node.kind() { Kind::Identifier => node.text(), Kind::CallExpression => node.expression().map_or("", primary_node_name), Kind::PropertyAccessExpression => node.name().unwrap().text(), _ => "" }
}

struct PermissionExpression<'a> { source: &'a str, granted: &'a [String] }
#[derive(Clone, Copy)]
enum PermissionToken { Initial, And, Or }
#[derive(Clone, Copy)]
struct PermissionCalculation { valid: bool, token: PermissionToken, finish: bool, matches: bool }
#[derive(Default)]
struct PermissionGroup<'a> { queue: Vec<&'a str>, parentheses: bool }
impl PermissionExpression<'_> {
    fn valid(&self) -> bool {
        let queue = self.tokens();
        let mut result = PermissionCalculation { valid: false, token: PermissionToken::Initial, finish: false, matches: true };
        self.validate(&queue, &mut result); result.valid
    }
    fn tokens(&self) -> Vec<&str> {
        let mut items: Vec<_> = self.source.split(' ').map(str::trim).filter(|s| !s.is_empty()).collect();
        for separator in ["(", ")", "\n"] {
            let mut next = Vec::new();
            for item in items {
                if !item.contains(separator) { next.push(item); continue; }
                for part in item.split(separator) { let part = part.trim(); next.push(if part.is_empty() { separator } else { part }); }
            }
            items = next;
        }
        items
    }
    fn validate(&self, queue: &[&str], result: &mut PermissionCalculation) {
        if queue.contains(&"(") || queue.contains(&")") {
            let mut flattened = Vec::new();
            for group in self.groups(queue) {
                if !group.parentheses { flattened.extend(group.queue); continue; }
                let inner = if group.queue.len() >= 2 { &group.queue[1..group.queue.len()-1] } else { &[] };
                let mut nested = *result;
                self.validate(inner, &mut nested);
                flattened.push(if nested.valid { "" } else { "NA" });
            }
            self.validate_atoms(&flattened, result);
        } else { self.validate_atoms(queue, result); }
    }
    fn groups<'a>(&self, queue: &[&'a str]) -> Vec<PermissionGroup<'a>> {
        let mut depth = 0;
        let mut groups = Vec::new(); let mut current = PermissionGroup::default();
        for (index, &item) in queue.iter().enumerate() {
            match item {
                "(" => {
                    if depth == 0 { groups.push(current); current = PermissionGroup { queue: vec![item], parentheses: true }; }
                    else { current.queue.push(item); }
                    depth += 1;
                }
                ")" => { depth -= 1; current.queue.push(item); if depth == 0 { groups.push(std::mem::take(&mut current)); } }
                _ => { current.queue.push(item); if index == queue.len()-1 { groups.push(std::mem::take(&mut current)); } }
            }
        }
        groups
    }
    fn validate_atoms(&self, queue: &[&str], result: &mut PermissionCalculation) {
        if result.finish { return; }
        if queue.is_empty() { result.matches = false; result.valid = false; result.finish = true; return; }
        for &item in queue {
            match item {
                "and" => result.token = PermissionToken::And,
                "or" => result.token = PermissionToken::Or,
                _ => { let matches = item.is_empty() || self.granted.iter().any(|s| s == item);
                    result.matches = match result.token { PermissionToken::Or => result.matches || matches, PermissionToken::And => result.matches && matches, PermissionToken::Initial => matches }; }
            }
        }
        result.valid = result.matches; result.finish = true;
    }
}

#[cfg(test)]
#[path = "oh_sdk_jsdoc_test.rs"]
mod tests;
