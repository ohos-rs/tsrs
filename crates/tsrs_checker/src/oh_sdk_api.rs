//! SDK apiAvailable and source-retention annotation validation.
use crate::*;
use tsrs_core::{ScriptKind, tspath};
use tsrs_diagnostics::Category;
use std::sync::LazyLock;
use regex::Regex;
const OH_RUNTIME_OS: &str = "OpenHarmony";
const OH_AVAILABLE_FILE_NAME: &str = "@ohos.annotation.d.ets";
const OH_DEVICE_INFO_FILE_NAME: &str = "@ohos.deviceInfo.d.ts";
const OH_API_AVAILABLE_NAME: &str = "apiAvailable";
const OH_API_AVAILABLE_ERROR: &str = "Invalid parameters for apiAvailable.";
const OH_API_AVAILABLE_OPEN_HARMONY_CONTENT_ERROR: &str = "The api version must be a decimal integer between 1 and 25.\n The M.S.F format must meet the following requirements: The value must be in the three decimal format, M must be greater than or equal to 26, and S and F must be decimal integers between 0 and 99.";
const OH_API_AVAILABLE_DISTRIBUTION_CONTENT_ERROR: &str = "The api version must be a decimal integer between 1 and 25.\n The M.S.F format must meet the following requirements: The value must be in the three decimal format, M must be decimal integers between 1 and 99, and S and F must be decimal integers between 0 and 99.";
const OH_AVAILABLE_VERSION_FORMAT_ERROR_PREFIX: &str = "The runtime OS for the current project is $RUNTIMEOS. The OS version number $VERSION is invalid.";
const OH_AVAILABLE_VERSION_FORMAT_ERROR: &str = "The OpenHarmony version must be an integer between 1 and 999,\n and when the OpenHarmony version is greater than or equal to 26, the version number format also supports the M.S.F format.";
const OH_AVAILABLE_OS_NAME_ERROR: &str = "The runtime OS for the current project is $RUNTIMEOS. @Available is not supported on the OS: $OSNAME.";
const OH_AVAILABLE_SCOPE_ERROR: &str = "Unnecessary. The outer annotation already indicates that the version is greater than or equal to $VERSION.";
static OH_DECIMAL_INTEGER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?[0-9]+$").unwrap());
static OH_CANONICAL_DECIMAL_INTEGER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?(0|[1-9][0-9]*)$").unwrap());
static OH_OPEN_HARMONY_STRING_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9.]+$").unwrap());
static OH_DISTRIBUTION_STRING_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9.()]+$").unwrap());
static OH_MSF_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([1-9][0-9]?)\.(0|[1-9][0-9]?)\.(0|[1-9][0-9]?)(?:\(([0-9]+)\))?$").unwrap());
static OH_AVAILABLE_FORMAT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[1-9][0-9]{0,2}|[1-9][0-9]?\.[0-9]{1,2}\.[0-9]{1,2})$").unwrap());

#[derive(Clone, Default)]
pub(crate) struct OhParsedVersion {
    pub os: String,
    pub version: String,
    pub format_version: String,
}
pub(crate) struct OhSdkCheckResult {
    pub valid: bool,
    pub message: String,
    pub category: Category,
}
impl Checker {
    pub(crate) fn add_oh_sdk_diagnostic(&mut self, node: P<Node>, result: &OhSdkCheckResult) {
        let file = ast::get_source_file_of_node(node).unwrap();
        let diagnostic = ast::new_diagnostic_from_text(Some(file), tsrs_scanner::get_error_range_for_node(file, node),
            28007, result.category, &result.message, &[], &[], false, false);
        self.add_diagnostic(diagnostic);
        self.add_suggestion_diagnostic(diagnostic);
    }
    pub(crate) fn check_api_available_version(&mut self, node: P<Node>) {
        if !ast::is_call_expression(node) { return; }
        let Some(expression) = node.expression() else { return; };
        let file = ast::get_source_file_of_node(node).unwrap();
        if file.script_kind() != ScriptKind::ETS && !self.compiler_options.uses_oh_module_resolution() { return; }
        if ast::is_property_access_expression(expression) {
            let name = expression.name().unwrap();
            if !ast::is_identifier(name) || name.text() != OH_API_AVAILABLE_NAME { return; }
        }
        if node.arguments().len() != 1 || !self.is_oh_api_available_expression(expression) { return; }
        let result = self.validate_oh_api_available_argument(node.arguments()[0]);
        if !result.valid { self.add_oh_sdk_diagnostic(node, &result); }
    }
    pub(crate) fn is_oh_api_available_expression(&mut self, node: P<Node>) -> bool {
        let mut typ = self.get_type_of_node(node);
        if typ.flags().intersects(TypeFlags::Union) {
            let mut non_nullable = None;
            for &constituent in typ.types() {
                if constituent.flags().intersects(TypeFlags::Nullable) { continue; }
                if non_nullable.is_some() { return false; }
                non_nullable = Some(constituent);
            }
            let Some(t) = non_nullable else { return false; };
            typ = t;
        }
        let Some(declaration) = typ.symbol().and_then(|s| s.value_declaration()) else { return false; };
        declaration.name().is_some_and(|name| ast::is_identifier(name) && name.text() == OH_API_AVAILABLE_NAME)
            && tspath::normalize_path(ast::get_source_file_of_node(declaration).unwrap().file_name()).ends_with(OH_DEVICE_INFO_FILE_NAME)
    }
    pub(crate) fn validate_oh_api_available_argument(&self, argument: P<Node>) -> OhSdkCheckResult {
        let mut result = OhSdkCheckResult { valid: true, message: OH_API_AVAILABLE_ERROR.to_string(), category: Category::Error };
        let is_number = ast::is_numeric_literal(argument) || ast::is_prefix_unary_expression(argument)
            && ast::is_numeric_literal(argument.as_prefix_unary_expression().operand)
            && matches!(argument.as_prefix_unary_expression().operator, Kind::PlusToken | Kind::MinusToken);
        let is_string = ast::is_string_literal(argument) || argument.kind() == Kind::NoSubstitutionTemplateLiteral;
        let nullish = argument.kind() == Kind::NullKeyword || ast::is_identifier(argument) && argument.text() == "undefined";
        if !is_number && !is_string && !nullish { return result; }
        let fail = |message: &str| OhSdkCheckResult { valid: false, message: format!("11706013#{OH_API_AVAILABLE_ERROR}{message}"), category: Category::Error };
        if nullish { return fail("Null and undefined are not allowed for parameters."); }
        if is_number {
            let text = tsrs_scanner::get_text_of_node(argument);
            let text = text.trim();
            if !OH_DECIMAL_INTEGER_PATTERN.is_match(text) { return fail("Only decimal digits are allowed."); }
            if !OH_CANONICAL_DECIMAL_INTEGER_PATTERN.is_match(text) || !text.parse::<f64>().is_ok_and(|v| (1.0..26.0).contains(&v)) {
                return fail(if self.oh_runtime_os() == OH_RUNTIME_OS { OH_API_AVAILABLE_OPEN_HARMONY_CONTENT_ERROR } else { OH_API_AVAILABLE_DISTRIBUTION_CONTENT_ERROR });
            }
            return result;
        }
        let content = argument.text();
        if self.oh_runtime_os() == OH_RUNTIME_OS {
            if !OH_OPEN_HARMONY_STRING_PATTERN.is_match(content) { return fail("Only digits and dots are allowed."); }
            if OH_MSF_PATTERN.captures(content).is_none_or(|m| parse_decimal(&m[1]) < 26) {
                return fail(OH_API_AVAILABLE_OPEN_HARMONY_CONTENT_ERROR);
            }
            return result;
        }
        if !OH_DISTRIBUTION_STRING_PATTERN.is_match(content) { return fail("Only digits, dots, and left and right parentheses are allowed."); }
        let Some(m) = OH_MSF_PATTERN.captures(content) else { return fail(OH_API_AVAILABLE_DISTRIBUTION_CONTENT_ERROR); };
        if parse_decimal(&m[1]) >= 26 && m.get(4).is_some_and(|v| !v.as_str().is_empty()) { return fail(OH_API_AVAILABLE_DISTRIBUTION_CONTENT_ERROR); }
        if parse_decimal(&m[1]) < 26 {
            let distribution = self.oh_sdk_check_distribution("since", content);
            if distribution.as_ref().is_some_and(|d| d.valid) { return result; }
            result.valid = false;
            result.message = format!("11706014#{}", distribution.map(|d| d.message).unwrap_or_default());
        }
        result
    }
    pub(crate) fn oh_runtime_os(&self) -> &str {
        if self.compiler_options.oh_runtime_os.is_empty() { OH_RUNTIME_OS } else { &self.compiler_options.oh_runtime_os }
    }
    pub(crate) fn validate_oh_available_version(&self, version: &OhParsedVersion) -> OhSdkCheckResult {
        let runtime = self.oh_runtime_os();
        let mut result = OhSdkCheckResult { valid: true, message: String::new(), category: Category::Error };
        if version.os != OH_RUNTIME_OS && version.os != runtime {
            result.valid = false;
            result.message = format!("11706017#{}", OH_AVAILABLE_OS_NAME_ERROR.replace("$RUNTIMEOS", runtime).replace("$OSNAME", &version.os));
            return result;
        }
        let format_valid = |value: &str| OH_AVAILABLE_FORMAT_PATTERN.is_match(value)
            && (!value.contains('.') || parse_decimal(value.split('.').next().unwrap()) >= 26);
        let (valid, plugin_message) = if version.os == OH_RUNTIME_OS { (format_valid(&version.version), String::new()) }
            else if let Some(plugin) = self.oh_sdk_check_format("available", &version.format_version) { (plugin.result, plugin.message) }
            else { (format_valid(&version.format_version), String::new()) };
        if valid { return result; }
        result.valid = false;
        let prefix = OH_AVAILABLE_VERSION_FORMAT_ERROR_PREFIX.replace("$RUNTIMEOS", runtime).replace("$VERSION", &version.version);
        let message = if plugin_message.is_empty() { OH_AVAILABLE_VERSION_FORMAT_ERROR } else { &plugin_message };
        result.message = format!("11706016#{prefix} {message}");
        result
    }
    pub(crate) fn check_source_retention_annotation_content(&mut self, node: P<Node>, declaration: P<Node>) {
        if declaration.name().is_none_or(|n| n.text() != "Available")
            || !tspath::normalize_path(ast::get_source_file_of_node(declaration).unwrap().file_name()).ends_with(OH_AVAILABLE_FILE_NAME) { return; }
        let expression = node.expression().unwrap();
        if !ast::is_call_expression(expression) || expression.arguments().is_empty()
            || !ast::is_object_literal_expression(expression.arguments()[0]) { return; }
        for &property in expression.arguments()[0].properties() {
            if !ast::is_property_assignment(property) || property.name().is_none_or(|n| n.text() != "minApiVersion")
                || !property.initializer().is_some_and(ast::is_string_literal) { continue; }
            let current = parse_oh_available_version(property.initializer().unwrap().text());
            let result = self.validate_oh_available_version(&current);
            if !result.valid { self.add_oh_sdk_diagnostic(node, &result); return; }
            if let Some(outer) = self.find_outer_oh_available_version(node.parent().unwrap()) {
                if !self.oh_available_versions_compatible(&outer, &current) {
                    self.add_oh_sdk_diagnostic(node, &OhSdkCheckResult { valid: false,
                        message: OH_AVAILABLE_SCOPE_ERROR.replacen("$VERSION", &outer.version, 1), category: Category::Warning });
                }
            }
            return;
        }
    }
    pub(crate) fn oh_available_versions_compatible(&self, required: &OhParsedVersion, target: &OhParsedVersion) -> bool {
        let scene = if target.os == OH_RUNTIME_OS { 1 } else { 2 };
        self.oh_sdk_check_value("available", &required.format_version, &target.format_version, scene)
            .map_or_else(|| compare_oh_point_versions(&target.version, &required.version) >= 0, |r| r.result)
    }
    fn find_outer_oh_available_version(&self, node: P<Node>) -> Option<OhParsedVersion> {
        let mut current = node.parent();
        while let Some(node) = current {
            for &modifier in node.modifier_nodes() {
                if !ast::is_decorator(modifier) { continue; }
                let expr = modifier.expression().unwrap();
                if !ast::is_call_expression(expr) || !expr.expression().is_some_and(|e| ast::is_identifier(e) && e.text() == "Available")
                    || expr.arguments().is_empty() || !ast::is_object_literal_expression(expr.arguments()[0]) { continue; }
                for &property in expr.arguments()[0].properties() {
                    if !ast::is_property_assignment(property) || property.name().is_none_or(|n| n.text() != "minApiVersion") { continue; }
                    if let Some(value) = property.initializer().filter(|v| ast::is_string_literal(*v) || ast::is_numeric_literal(*v)) {
                        let version = parse_oh_available_version(value.text());
                        if self.validate_oh_available_version(&version).valid { return Some(version); }
                    }
                }
            }
            current = node.parent();
        }
        None
    }
}
pub(crate) fn parse_decimal(value: &str) -> i32 { value.parse().unwrap_or_default() }
pub(crate) fn parse_oh_available_version(raw: &str) -> OhParsedVersion {
    let trimmed = raw.trim();
    let parts: Vec<_> = trimmed.split_whitespace().collect();
    let (os, version) = if parts.len() >= 2 { (parts[0], parts[1]) } else { (OH_RUNTIME_OS, trimmed) };
    OhParsedVersion { os: os.to_string(), version: version.to_string(), format_version: format!("{os} {version}") }
}
pub(crate) fn compare_oh_point_versions(first: &str, second: &str) -> i32 {
    let parse = |value: &str| {
        let mut result = [0; 3];
        for (i, part) in value.trim().split('.').take(3).enumerate() {
            let end = part.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(part.len());
            result[i] = parse_decimal(&part[..end]);
        }
        result
    };
    match parse(first).cmp(&parse(second)) { std::cmp::Ordering::Less => -1, std::cmp::Ordering::Equal => 0, std::cmp::Ordering::Greater => 1 }
}

pub(crate) fn oh_available_format_matches(value: &str) -> bool { OH_AVAILABLE_FORMAT_PATTERN.is_match(value) }
pub(crate) fn oh_msf_captures(value: &str) -> Option<i32> { OH_MSF_PATTERN.captures(value).map(|m| parse_decimal(&m[1])) }
