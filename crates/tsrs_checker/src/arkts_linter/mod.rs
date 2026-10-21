//! Dynamic `ArkTS` 1.1 linter, ported from OH `checker/arkts_linter.go`.
#![expect(clippy::unused_self, reason = "ported Go receiver methods keep their receiver")]
use crate::*;
use tsrs_core::{ScriptKind, TextRange, tspath};
use tsrs_scanner::{get_error_range_for_node, get_token_pos_of_node};
mod faults;
mod diagnostics;
use faults::Fault;
mod declarations;
use declarations::{asterisk_token, heritage_expression, reduce_reference};
mod expressions;
mod types;
mod sendable;
mod interop;

struct ArkTsLinter<'a> {
    checker: &'a mut Checker,
    file: P<SourceFile>,
    strict_diagnostics: Vec<P<Diagnostic>>,
    diagnostics: Vec<P<Diagnostic>>,
    exported: Option<FxHashSet<P<Node>>>,
    kit_infos: FxHashMap<String, KitInfo>,
    ignored_directories: Vec<String>,
    path_components: FxHashMap<P<SourceFile>, Vec<String>>,
}
#[derive(Default, serde::Deserialize)]
struct KitInfo { symbols: FxHashMap<String, KitSymbol> }
#[derive(serde::Deserialize)]
struct KitSymbol { source: String }
#[derive(Clone, Copy, PartialEq, Eq)]
enum StrictErrorType { NoError, Unknown, Null, PossiblyUndefined }

impl Checker {
    pub fn get_arkts_linter_diagnostics(&mut self, file: P<SourceFile>, strict_diagnostics: Vec<P<Diagnostic>>) -> Vec<P<Diagnostic>> {
        let (strict_diagnostics, non_linter_diagnostics): (Vec<_>, Vec<_>) = if self.compiler_options.strict_checker_only == Tristate::True && file.script_kind() == ScriptKind::ETS {
            strict_diagnostics.into_iter().partition(|d| matches!(d.code(), 2322 | 2345 | 2769 | 2532 | 2564))
        } else { (strict_diagnostics, Vec::new()) };
        let ignored_directories = self.arkts_ignored_directories();
        let mut linter = ArkTsLinter { checker: self, file, strict_diagnostics, diagnostics: Vec::new(), exported: None, kit_infos: FxHashMap::default(), ignored_directories, path_components: FxHashMap::default() };
        if file.script_kind() == ScriptKind::ETS {
            linter.visit(file.as_node());
            linter.check_comment_directives();
        } else { linter.visit_interop(file.as_node()); }
        linter.strict_diagnostics.extend(non_linter_diagnostics);
        linter.strict_diagnostics.extend(linter.diagnostics);
        linter.strict_diagnostics
    }
}

impl ArkTsLinter<'_> {
    fn add(&mut self, node: P<Node>, fault: Fault) {
        let Some((code, category, message)) = fault.attribute() else { return; };
        let mut start = get_token_pos_of_node(node, self.file, false);
        let mut end = node.end().source_offset().unwrap_or(start);
        match fault {
            Fault::VarDeclaration => end = (start + 3).min(end),
            Fault::DeleteOperator | Fault::TypeQuery => end = (start + 6).min(end),
            Fault::WithStatement => {
                end = get_error_range_for_node(self.file, node.as_with_statement().statement)
                    .pos()
                    .source_offset()
                    .unwrap_or(0)
                    .saturating_sub(1);
            }
            Fault::ForInStatement => {
                let s = node.as_for_in_or_of_statement();
                start = s.initializer.end().source_offset().unwrap_or(0) + 1;
                end = s.expression.pos().source_offset().unwrap_or(0).saturating_sub(1);
            }
            Fault::CatchWithUnsupportedType => if let Some(declaration) = node.as_catch_clause().variable_declaration {
                let loc = get_error_range_for_node(self.file, declaration);
                start = loc.pos().source_offset().unwrap_or(0);
                end = loc.end().source_offset().unwrap_or(0);
            },
            Fault::DeclWithDuplicateName => if let Some(name) = node.name() {
                let loc = get_error_range_for_node(self.file, name);
                start = loc.pos().source_offset().unwrap_or(0);
                end = loc.end().source_offset().unwrap_or(0);
            },
            Fault::ObjectLiteralNoContextType => end = (start + 1).min(end),
            Fault::InstanceofUnsupported => {
                start = get_error_range_for_node(self.file, node.as_binary_expression().operator_token)
                    .pos()
                    .source_offset()
                    .unwrap_or(0);
                end = start + 10;
            }
            Fault::ConstAssertion => {
                if ast::is_as_expression(node) {
                    start = node.expression().unwrap().end().source_offset().unwrap_or(0) + 1;
                    end = get_error_range_for_node(self.file, node.type_node().unwrap())
                        .pos()
                        .source_offset()
                        .unwrap_or(0)
                        .saturating_sub(1);
                } else if node.kind() == Kind::TypeAssertionExpression {
                    start = node.expression().unwrap().end().source_offset().unwrap_or(0) + 1;
                    end = node.type_node().unwrap().end().source_offset().unwrap_or(0) + 1;
                }
            }
            Fault::LimitedReturnTypeInference => {
                let target = match node.kind() {
                    Kind::FunctionExpression => node.type_node(),
                    Kind::FunctionDeclaration | Kind::MethodDeclaration => node.name(),
                    _ => None,
                };
                if let Some(target) = target {
                    let loc = get_error_range_for_node(self.file, target);
                    start = loc.pos().source_offset().unwrap_or(0);
                    end = loc.end().source_offset().unwrap_or(0);
                }
            }
            Fault::LocalFunction => end = (start + 8).min(end),
            Fault::FunctionApplyCall | Fault::FunctionBind => {
                if let Some(point) = self.file.text()[start as usize..end as usize].rfind('.') {
                    start += point as u32 + 1;
                }
            }
            Fault::ClassExpression => end = (start + 5).min(end),
            Fault::MultipleStaticBlocks => end = (start + 6).min(end),
            Fault::SendableDefiniteAssignment => if let Some(name) = node.name() {
                end = name.end().source_offset().unwrap_or(start);
                if let Some(token) = node.as_property_declaration().postfix_token().filter(|t| t.kind() == Kind::ExclamationToken) {
                    end = token.end().source_offset().unwrap_or(end);
                }
                start = get_error_range_for_node(self.file, name)
                    .pos()
                    .source_offset()
                    .unwrap_or(0);
            },
            _ => {}
        }
        self.diagnostics.push(ast::new_diagnostic_from_text(Some(self.file), TextRange::new(start, end), code, category, message, &[], &[], false, false));
    }
    fn visit(&mut self, node: P<Node>) {
        if node.parent().is_some_and(ast::is_struct_declaration) && ast::is_constructor_declaration(node) && node.is_virtual() { return; }
        let fault = match node.kind() {
            Kind::AnyKeyword => Some(Fault::AnyType),
            Kind::SymbolKeyword => Some(Fault::SymbolType),
            Kind::ThisType => Some(Fault::ThisType),
            Kind::TypeQuery => Some(Fault::TypeQuery),
            Kind::DeleteExpression => Some(Fault::DeleteOperator),
            Kind::TypePredicate => Some(Fault::IsOperator),
            Kind::YieldExpression => Some(Fault::YieldExpression),
            Kind::WithStatement => Some(Fault::WithStatement),
            Kind::IndexedAccessType => Some(Fault::IndexedAccessType),
            Kind::UnknownKeyword => Some(Fault::UnknownType),
            Kind::CallSignature => Some(Fault::CallSignature),
            Kind::IntersectionType => Some(Fault::IntersectionType),
            Kind::TypeLiteral => Some(Fault::ObjectTypeLiteral),
            Kind::ConstructorType => Some(Fault::ConstructorFuncs),
            Kind::PrivateIdentifier => Some(Fault::PrivateIdentifier),
            Kind::ConditionalType => Some(Fault::ConditionalType),
            Kind::MappedType => Some(Fault::MappedType),
            Kind::JsxElement | Kind::JsxSelfClosingElement => Some(Fault::JsxElement),
            Kind::ImportEqualsDeclaration => Some(Fault::ImportAssignment),
            Kind::NamespaceExportDeclaration => Some(Fault::UMDModuleDefinition),
            Kind::ClassExpression => Some(Fault::ClassExpression),
            _ => None,
        };
        if let Some(fault) = fault { self.add(node, fault); } else { self.handle(node); }
        node.for_each_child(&mut |child| { self.visit(child); false });
    }
    fn handle(&mut self, node: P<Node>) {
        match node.kind() {
            Kind::ObjectLiteralExpression => self.handle_object_literal_expression(node),
            Kind::ArrayLiteralExpression => self.handle_array_literal_expression(node),
            Kind::Parameter => self.handle_parameter(node),
            Kind::ForStatement => {
                if let Some(initializer) = node.as_for_statement().initializer.filter(|n| matches!(n.kind(), Kind::ArrayLiteralExpression | Kind::ObjectLiteralExpression)) { self.add(initializer, Fault::DestructuringAssignment); }
            }
            Kind::ForInStatement | Kind::ForOfStatement => {
                let initializer = node.as_for_in_or_of_statement().initializer;
                if matches!(initializer.kind(), Kind::ArrayLiteralExpression | Kind::ObjectLiteralExpression) { self.add(initializer, Fault::DestructuringAssignment); }
                if node.kind() == Kind::ForInStatement { self.add(node, Fault::ForInStatement); }
            }
            Kind::ImportDeclaration => self.handle_import_declaration(node),
            Kind::ImportClause | Kind::ImportSpecifier | Kind::NamespaceImport => if let Some(name) = node.name() { self.count_duplicate_name(name, node, node.kind()); },
            Kind::PropertyDeclaration | Kind::PropertySignature => {
                if let Some(name) = node.name().filter(|n| ast::is_numeric_literal(*n)) { self.add(name, Fault::LiteralAsPropertyName); }
                if ast::is_property_declaration(node) { self.filter_property_initialization_diagnostic(node); }
                self.handle_inferred_declaration(node); self.handle_definite_assignment(node);
                if ast::is_property_declaration(node) { self.handle_property_assignment_compatibility(node); self.handle_sendable_class_property(node); }
                else { self.handle_sendable_interface_property(node); }
            }
            Kind::PropertyAssignment => if let Some(name) = node.name().filter(|n| ast::is_numeric_literal(*n)) {
                if !self.numeric_property_assignment_is_allowed(node) { self.add(name, Fault::LiteralAsPropertyName); }
            },
            Kind::FunctionExpression => self.handle_function_expression(node),
            Kind::ArrowFunction => { self.handle_standalone_this(node, node.body()); self.handle_missing_return_type(node); }
            Kind::FunctionDeclaration => self.handle_function_declaration(node),
            Kind::TypeAliasDeclaration => {
                if let Some(name) = node.name() { self.count_duplicate_name(name, node, node.kind()); }
                self.handle_sendable_type_alias(node);
            }
            Kind::PrefixUnaryExpression => self.handle_prefix_unary_expression(node),
            Kind::BinaryExpression => self.handle_binary_expression(node),
            Kind::VariableDeclarationList => if !node.flags().intersects(NodeFlags::Let | NodeFlags::Const) { self.add(node, Fault::VarDeclaration); },
            Kind::VariableDeclaration => self.handle_variable_declaration(node),
            Kind::CatchClause => if node.as_catch_clause().variable_declaration.is_some_and(|n| n.type_node().is_some()) { self.add(node, Fault::CatchWithUnsupportedType); },
            Kind::ClassDeclaration => self.handle_class_declaration(node),
            Kind::InterfaceDeclaration => self.handle_interface_declaration(node),
            Kind::EnumDeclaration => self.handle_enum_declaration(node),
            Kind::ModuleDeclaration => self.handle_module_declaration(node),
            Kind::TypeAssertionExpression => {
                let start = get_token_pos_of_node(node, self.file, false) as usize;
                if self.file.text()[start..node.end().source_offset().unwrap_or(start as u32) as usize]
                    .starts_with("</")
                {
                    let line_start = self.file.text()[..start].rfind('\n').map_or(0, |i| i + 1);
                    if self.file.text()[line_start..start].trim().is_empty() { return; }
                }
                self.add(node, if ast::is_const_type_reference(node.type_node().unwrap()) { Fault::ConstAssertion } else { Fault::TypeAssertion });
            }
            Kind::MethodDeclaration | Kind::MethodSignature => {
                if ast::is_method_declaration(node) { self.handle_decorators_in_sendable_class(node); self.filter_method_return_diagnostic(node); }
                self.handle_missing_return_type(node);
                if ast::is_method_declaration(node) && node.modifier_flags().intersects(ModifierFlags::Static) { self.report_this_keywords(node.body()); }
                if asterisk_token(node).is_some() { self.add(node, Fault::GeneratorFunction); }
            }
            Kind::Identifier => self.handle_identifier(node),
            Kind::PropertyAccessExpression => self.handle_property_access(node),
            Kind::ElementAccessExpression => self.handle_element_access(node),
            Kind::EnumMember => self.handle_enum_member(node),
            Kind::TypeReference => self.handle_type_reference(node),
            Kind::ExportAssignment => {
                if node.as_export_assignment().is_export_equals { self.add(node, Fault::ExportAssignment); }
                self.handle_shared_export_assignment(node);
            }
            Kind::ExportKeyword => self.handle_shared_export_keyword(node),
            Kind::ExportDeclaration => self.handle_shared_export_declaration(node),
            Kind::CallExpression => self.handle_call_expression(node),
            Kind::NewExpression => self.handle_generic_call(node),
            Kind::MetaProperty => if node.as_meta_property().keyword_token == Kind::NewKeyword && node.name().unwrap().text() == "target" { self.add(node, Fault::NewTarget); },
            Kind::AsExpression => {
                let type_node = node.type_node().unwrap(); let expression = node.expression().unwrap();
                if ast::is_const_type_reference(type_node) { self.add(node, Fault::ConstAssertion); }
                let target_type = self.checker.get_type_of_node(type_node); let target_type = self.checker.get_non_nullable_type(target_type);
                let expression_type = self.checker.get_type_of_node(expression); let expression_type = self.checker.get_non_nullable_type(expression_type);
                if expression_type.flags().intersects(TypeFlags::NumberLike) && target_type == self.checker.global_number_type
                    || expression_type.flags().intersects(TypeFlags::BooleanLike) && target_type == self.checker.global_boolean_type { self.add(node, Fault::TypeAssertion); }
                self.handle_sendable_as_expression(node, target_type, expression_type);
            }
            Kind::SpreadElement | Kind::SpreadAssignment => self.handle_spread(node),
            Kind::ConstructSignature => {
                if node.parent().is_some_and(ast::is_type_literal_node) { self.add(node, Fault::ConstructorType); }
                else if node.parent().is_some_and(ast::is_interface_declaration) { self.add(node, Fault::ConstructorIface); }
            }
            Kind::ComputedPropertyName => self.handle_computed_property_name(node),
            Kind::ClassStaticBlockDeclaration => if self.checker.compiler_options.skip_ark_ts_static_blocks_check != Tristate::True { self.report_this_keywords(Some(node.as_class_static_block_declaration().body)); },
            Kind::IndexSignature => if !self.is_allowed_index_signature(node) { self.add(node, Fault::IndexMember); },
            Kind::ThrowStatement => self.handle_throw(node),
            Kind::ReturnStatement => self.handle_return_statement(node),
            Kind::ExpressionWithTypeArguments => self.handle_expression_with_type_arguments(node),
            Kind::Decorator => self.handle_decorator(node),
            Kind::GetAccessor | Kind::SetAccessor => self.handle_decorators_in_sendable_class(node),
            Kind::TypeParameter => self.handle_sendable_type_parameter(node),
            _ => {}
        }
    }
    fn filter_strict_diagnostics(&mut self, code: i32, begin: u32, end: u32) {
        self.strict_diagnostics.retain(|d| {
            !(d.code() == code && d.pos().is_some_and(|pos| pos >= begin && pos <= end))
        });
    }
    fn node_text(&self, node: P<Node>) -> &'static str {
        let start = get_token_pos_of_node(node, self.file, false);
        self.file
            .text()
            .get(start as usize..node.end().source_offset().unwrap_or(start) as usize)
            .unwrap_or_default()
    }
}
