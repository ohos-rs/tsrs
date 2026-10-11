use super::*;
use tsrs_diagnostics::Category;

impl ArkTsLinter<'_> {
    pub(super) fn filter_library_call_diagnostics(&mut self, node: P<Node>, symbol: Option<P<Symbol>>) {
        let typ = self.checker.get_type_of_node(node.expression().unwrap()); let library = self.is_library_type(typ);
        let oh_modules_ets = symbol.and_then(|s| s.declarations().first().copied()).and_then(ast::get_source_file_of_node).is_some_and(|f| f.script_kind() == ScriptKind::ETS && self.source_file_path_contains_directory(f, "oh_modules"));
        let fallback = self.ignored_directories.iter().any(|d| d == "oh_modules");
        let mut deleted = FxHashSet::default();
        // OH keeps hasFiltered false in both passes, including after deleting a diagnostic.
        for possibly_undefined in [false, true] {
            for &diagnostic in &self.strict_diagnostics {
                if (self.strict_diagnostic_error_type(diagnostic) == StrictErrorType::PossiblyUndefined) != possibly_undefined { continue; }
                if self.filter_library_call_diagnostic(node, diagnostic, library, false, oh_modules_ets, fallback) { deleted.insert(diagnostic); }
            }
        }
        self.strict_diagnostics.retain(|d| !deleted.contains(d));
    }
    fn filter_library_call_diagnostic(&self, node: P<Node>, diagnostic: P<Diagnostic>, library: bool, filtered: bool, oh_modules_ets: bool, fallback: bool) -> bool {
        let error_type = self.strict_diagnostic_error_type(diagnostic);
        let filterable = match error_type { StrictErrorType::Unknown => true, StrictErrorType::Null => library, StrictErrorType::PossiblyUndefined => library && filtered, _ => false };
        if diagnostic.category() == Category::Warning || !filterable || !self.is_valid_library_call_diagnostic_range(node, diagnostic) { return false; }
        if fallback && oh_modules_ets && error_type != StrictErrorType::Unknown { diagnostic.set_category(Category::Warning); return false; }
        true
    }
    fn strict_diagnostic_error_type(&self, diagnostic: P<Diagnostic>) -> StrictErrorType {
        let source_type = diagnostic.message_args().first().map_or_else(|| diagnostic.message_text(), String::as_str);
        match diagnostic.code() {
            2322 => {
                if contains_type_word(source_type, "unknown") {
                    if self.checker.compiler_options.strict_checker_only == Tristate::True && !diagnostic.filter_flag() { return StrictErrorType::NoError; }
                    return StrictErrorType::Unknown;
                }
                if contains_type_word(source_type, "null") || contains_type_word(source_type, "undefined") { return StrictErrorType::Null; }
            }
            2345 => if contains_type_word(source_type, "null") || contains_type_word(source_type, "undefined") { return StrictErrorType::Null; },
            2532 => return StrictErrorType::PossiblyUndefined,
            _ => {}
        }
        for &child in diagnostic.message_chain() {
            let result = self.strict_diagnostic_error_type(child); if result != StrictErrorType::NoError { return result; }
        }
        StrictErrorType::NoError
    }
    fn is_valid_library_call_diagnostic_range(&self, node: P<Node>, diagnostic: P<Diagnostic>) -> bool {
        let start = get_token_pos_of_node(node, self.file, false);
        let full_call = diagnostic.len() != 0 && diagnostic.end() == node.end() && diagnostic.pos() >= start;
        let valid_argument = self.is_valid_library_call_argument_position(node, diagnostic.pos());
        match diagnostic.code() { 2322 | 2345 => valid_argument, 2769 => full_call || valid_argument, 2532 => full_call, _ => false }
    }
    fn is_valid_library_call_argument_position(&self, node: P<Node>, position: i32) -> bool {
        let Some(arguments) = node.argument_list() else { return false; };
        if position < arguments.pos() || position >= arguments.end() { return false; }
        for argument in arguments.nodes() {
            let (begin, end) = if ast::is_arrow_function(*argument) { let body = argument.body().unwrap(); (body.pos(), body.end()) }
            else if ast::is_call_expression(*argument) { let Some(nested) = argument.argument_list() else { continue; }; (nested.pos(), nested.end()) }
            else { continue; };
            if position >= begin && position < end { return false; }
        }
        true
    }
    pub(super) fn check_comment_directives(&mut self) {
        for pragma in self.file.pragmas.get() { if pragma.name == "ts-nocheck" { self.add_comment_range(pragma.comment_range.text_range); } }
        for directive in self.file.comment_directives.get() { self.add_comment_range(directive.loc); }
    }
    fn add_comment_range(&mut self, loc: TextRange) {
        let (code, category, message) = Fault::ErrorSuppression.attribute().unwrap();
        self.diagnostics.push(ast::new_diagnostic_from_text(Some(self.file), loc, code, category, message, &[], &[], false, false));
    }
}
fn contains_type_word(text: &str, word: &str) -> bool {
    let identifier = |b: u8| b == b'_' || b == b'$' || b.is_ascii_alphanumeric();
    text.match_indices(word).any(|(i, _)| (i == 0 || !identifier(text.as_bytes()[i - 1])) && (i + word.len() == text.len() || !identifier(text.as_bytes()[i + word.len()])))
}
