//! OH checker.ts throws diagnostics (checker/throws.go).
use crate::*;
use regress::Regex;
use tsrs_core::{CompilerOptions, ScriptKind, tspath};

pub(crate) struct ThrowsChecker {
    options: P<CompilerOptions>,
    sdk: Regex,
    since: Vec<Regex>,
    checked: FxHashMap<P<Node>, bool>,
}

impl Checker {
    pub(crate) fn check_throws_call(&mut self, node: P<Node>, declaration: Option<P<Node>>) {
        let Some(declaration) = declaration else { return; };
        if !ast::is_call_expression(node) || !matches!(declaration.kind(), Kind::MethodDeclaration | Kind::MethodSignature | Kind::FunctionDeclaration) { return; }
        let file = ast::get_source_file_of_node(node).unwrap();
        if file.script_kind() != ScriptKind::ETS && !self.compiler_options.uses_oh_module_resolution() { return; }
        let tags = throws_tags(declaration);
        if tags.is_empty() { return; }
        let checker = self.throws.get_or_insert_with(|| {
            let path = &self.compiler_options.ets_loader_path;
            let pattern = if path.is_empty() { String::new() } else {
                let parent = if tspath::normalize_path(path).ends_with("dynamic/build-tools/ets-loader") { "../../../../.." } else { "../../../.." };
                tspath::get_normalized_absolute_path(parent, path)
            };
            ThrowsChecker {
                options: self.compiler_options,
                // OH constructs an ECMAScript RegExp from the unescaped SDK path.
                sdk: Regex::new(&pattern).expect("invalid throws SDK regular expression"),
                since: Vec::new(), checked: FxHashMap::default(),
            }
        });
        if !checker.should_check(declaration, &tags) || throws_handled(node) { return; }
        let mut current = node.parent();
        while let Some(owner) = current {
            if ast::is_function_like_declaration(owner) {
                if !matches!(owner.kind(), Kind::FunctionDeclaration | Kind::MethodDeclaration) || !throws_tags(owner).is_empty() { return; }
                let expression = node.expression().unwrap();
                let error_node = match expression.kind() {
                    Kind::Identifier | Kind::ElementAccessExpression => expression,
                    Kind::PropertyAccessExpression => expression.name().unwrap(),
                    _ => node,
                };
                self.error(Some(error_node), &diagnostics::Function_may_throw_exceptions_Special_handling_is_required, &[]);
                return;
            }
            current = owner.parent();
        }
    }
}

fn throws_tags(node: P<Node>) -> Vec<P<Node>> {
    let mut result = Vec::new();
    let mut current = Some(node);
    while let Some(owner) = current.filter(|owner| owner.parent().is_some()) {
        if let Some(doc) = owner.jsdoc(None).last() {
            if let Some(tags) = doc.as_jsdoc().tags {
                result.extend(tags.nodes().iter().copied().filter(|tag| tag.tag_name().text() == "throws"));
            }
        }
        current = ast::get_next_jsdoc_comment_location(owner);
    }
    result
}

impl ThrowsChecker {
    fn should_check(&mut self, declaration: P<Node>, tags: &[P<Node>]) -> bool {
        if let Some(&result) = self.checked.get(&declaration) { return result; }
        let result = self.check_declaration(declaration, tags);
        self.checked.insert(declaration, result);
        result
    }
    fn check_declaration(&mut self, declaration: P<Node>, tags: &[P<Node>]) -> bool {
        let path = tspath::normalize_path(ast::get_source_file_of_node(declaration).unwrap().file_name());
        if ["/node_modules/", "/oh_modules/", "/js_util_module/"].iter().any(|excluded| path.contains(excluded)) { return false; }
        for &parameter in declaration.parameters() {
            if let Some(typ) = parameter.type_node().filter(|typ| ast::is_type_reference_node(*typ)) {
                let name = typ.as_type_reference_node().type_name;
                if ast::is_identifier(name) && matches!(name.text(), "AsyncCallback" | "ErrorCallback") { return false; }
            }
        }
        let sdk = self.sdk.find(&path).is_some();
        tags.iter().any(|&tag| !sdk || !self.skip_tag(tag))
    }
    fn skip_tag(&mut self, tag: P<Node>) -> bool {
        let comment = raw_throws_tag_comment(tag);
        if comment.contains("{ BusinessError } 401 -") { return true; }
        let Some(version) = self.options.compile_sdk_version else { return false; };
        if self.since.is_empty() {
            self.since = [
                r"\[since\s+(?:\d+\.\d+\.\d+\((\d+)\))(?:\s*-\s*(?:\d+\.\d+\.\d+\((\d+)\)))?\]",
                r"\[since\s+(?:\d+\.\d+\.\d+\((\d+)\))\s*-\s*(\d+)\.\d+\.\d+\]",
                r"\[since\s+(\d+)\s*-\s*(\d+)\.\d+\.\d+\]",
                r"\[since\s+(\d+)\.\d+\.\d+(?:\s*-\s*(\d+)\.\d+\.\d+)?\]",
                r"\[since\s+(\d+)(?:\s*-\s*(\d+))?\]",
            ].into_iter().map(|pattern| Regex::new(pattern).unwrap()).collect();
        }
        for pattern in &self.since {
            let Some(found) = pattern.find(&comment) else { continue; };
            let from = found.group(1).and_then(|range| comment[range].parse::<f64>().ok()).unwrap_or_default();
            let to = found.group(2).map(|range| &comment[range]).unwrap_or_default();
            if to.is_empty() { return version < from; }
            let to = to.parse::<f64>().unwrap_or_default();
            return !(version >= from.min(to) && version <= from.max(to));
        }
        false
    }
}

fn raw_throws_tag_comment(tag: P<Node>) -> String {
    if let Some(file) = ast::get_source_file_of_node(tag) {
        if let Some(text) = file.text().get(
            tag.pos().source_offset().unwrap_or(0) as usize..tag.end().source_offset().unwrap_or(0) as usize,
        ) {
            if let Some(index) = text.find("@throws") { return text[index + "@throws".len()..].trim().to_string(); }
        }
    }
    tag.comment_list().map(|comments| comments.nodes().iter().map(|comment| comment.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn throws_handled(node: P<Node>) -> bool {
    let mut current = node.parent();
    while let Some(owner) = current {
        if matches!(owner.kind(), Kind::MethodDeclaration | Kind::FunctionDeclaration) { return false; }
        if ast::is_try_statement(owner) {
            let statement = owner.as_try_statement();
            if statement.catch_clause.is_some() && node.pos() >= statement.try_block.pos() && node.end() <= statement.try_block.end() { return true; }
        }
        if ast::is_property_access_expression(owner) && owner.name().unwrap().text() == "catch" && owner.parent().is_some_and(ast::is_call_expression) { return true; }
        current = owner.parent();
    }
    false
}
