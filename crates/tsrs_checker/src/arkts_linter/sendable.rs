use super::*;

impl ArkTsLinter<'_> {
    pub(super) fn report_non_sendable_decorators(&mut self, node: P<Node>, fault: Fault) {
        let disable_class_rule = fault == Fault::SendableClassDecorator && self.checker.compiler_options.disable_sendable_check_rules.as_ref().is_some_and(|rules| rules.iter().any(|r| r == "arkts-sendable-class-decorator"));
        for &decorator in node.modifier_nodes() {
            if !ast::is_decorator(decorator) { continue; }
            let name = self.decorator_name(decorator);
            if name == "Sendable" || disable_class_rule && !is_arkui_decorator(name) { continue; }
            self.add(decorator, fault);
        }
    }
    pub(super) fn handle_decorators_in_sendable_class(&mut self, node: P<Node>) {
        let class = if ast::is_parameter_declaration(node) { node.parent().and_then(|p| p.parent()) } else { node.parent() };
        if !class.is_some_and(|c| ast::is_class_declaration(c) && !ast::is_struct_declaration(c) && self.has_decorator(c, "Sendable")) { return; }
        for &decorator in node.modifier_nodes() { if ast::is_decorator(decorator) { self.add(decorator, Fault::SendableClassDecorator); } }
    }
    pub(super) fn handle_sendable_class_property(&mut self, node: P<Node>) {
        if !node.parent().is_some_and(|c| ast::is_class_declaration(c) && !ast::is_struct_declaration(c) && self.has_decorator(c, "Sendable")) { return; }
        let Some(typ) = node.type_node() else { self.add(node, Fault::SendableExplicitFieldType); return; };
        self.handle_decorators_in_sendable_class(node);
        if !self.is_sendable_type_node(typ, false, &mut FxHashSet::default()) { self.add(node, Fault::SendablePropType); }
        else { self.check_type_alias_in_sendable_scope(node); }
    }
    pub(super) fn handle_sendable_interface_property(&mut self, node: P<Node>) {
        let Some(typ) = node.type_node() else { return; };
        let Some(parent) = node.parent().filter(|p| ast::is_interface_declaration(*p)) else { return; };
        let parent_type = self.checker.get_type_of_node(parent);
        if !self.is_sendable_class_or_interface(parent_type) { return; }
        if !self.is_sendable_type_node(typ, false, &mut FxHashSet::default()) { self.add(node, Fault::SendablePropType); }
        else { self.check_type_alias_in_sendable_scope(node); }
    }
    fn check_type_alias_in_sendable_scope(&mut self, node: P<Node>) {
        let mut typ = node.type_node().unwrap();
        while ast::is_parenthesized_type_node(typ) { typ = typ.as_parenthesized_type_node().type_; }
        if ast::is_union_type_node(typ) {
            for &component in typ.as_union_type_node().types().nodes() {
                if self.is_non_sendable_type_alias_reference(component) { self.add(typ, Fault::SendablePropTypeWarning); return; }
            }
        } else if self.is_non_sendable_type_alias_reference(typ) { self.add(typ, Fault::SendablePropTypeWarning); }
    }
    fn is_non_sendable_type_alias_reference(&mut self, node: P<Node>) -> bool {
        if !ast::is_type_reference_node(node) { return false; }
        let reference = node.as_type_reference_node();
        let Some(symbol) = self.true_symbol_at_location(reference.type_name) else { return false; };
        if !symbol.flags().intersects(SymbolFlags::TypeAlias) || !symbol.declarations().first().is_some_and(|d| ast::is_type_alias_declaration(*d)) { return false; }
        reference.type_arguments().is_some_and(|args| args.nodes().iter().any(|&a| !self.is_sendable_type_node(a, false, &mut FxHashSet::default())))
    }
    pub(super) fn is_sendable_type_node(&mut self, mut node: P<Node>, shared: bool, seen: &mut FxHashSet<P<Symbol>>) -> bool {
        while ast::is_parenthesized_type_node(node) { node = node.as_parenthesized_type_node().type_; }
        if ast::is_union_type_node(node) { return node.as_union_type_node().types().nodes().iter().all(|&t| self.is_sendable_type_node(t, shared, seen)); }
        let symbol = if ast::is_type_reference_node(node) { self.true_symbol_at_location(node.as_type_reference_node().type_name) } else { None };
        if let Some(symbol) = symbol {
            if symbol.flags().intersects(SymbolFlags::TypeAlias) {
                if let Some(&declaration) = symbol.declarations().first().filter(|d| ast::is_type_alias_declaration(**d)) {
                    if !seen.insert(symbol) { return false; }
                    return self.is_sendable_type_node(declaration.type_node().unwrap(), shared, seen);
                }
            }
            if symbol.flags() == SymbolFlags::ConstEnum { return true; }
        }
        let typ = self.checker.get_type_from_type_node(node);
        if shared && typ.flags().intersects(TypeFlags::BooleanLiteral | TypeFlags::NumberLiteral | TypeFlags::StringLiteral | TypeFlags::BigIntLiteral) && !typ.flags().intersects(TypeFlags::EnumLiteral) { return true; }
        self.is_sendable_type(typ)
    }
    pub(super) fn handle_sendable_type_parameter(&mut self, node: P<Node>) {
        if !node.parent().is_some_and(|p| ast::is_class_declaration(p) && self.has_decorator(p, "Sendable")) { return; }
        if let Some(default_type) = node.as_type_parameter_declaration().default_type {
            if !self.is_sendable_type_node(default_type, false, &mut FxHashSet::default()) { self.add(default_type, Fault::SendableGenericTypes); }
        }
    }
    #[expect(
        clippy::float_cmp,
        reason = "SDK versions are exact configuration values, compared as in the Go source"
    )]
    pub(super) fn sendable_function_or_alias_available(&mut self, node: P<Node>) -> bool {
        let version = self.checker.compiler_options.compatible_sdk_version.unwrap_or(12.0);
        let stage = &self.checker.compiler_options.compatible_sdk_version_stage;
        let stage = if stage.is_empty() { "beta1" } else { stage };
        if version > 12.0 || version == 12.0 && stage != "beta1" && stage != "beta2" { return true; }
        for &decorator in node.modifier_nodes() {
            if self.decorator_name(decorator) == "Sendable" { self.add(decorator, Fault::SendableBetaCompatible); break; }
        }
        false
    }
    pub(super) fn handle_sendable_type_alias(&mut self, node: P<Node>) {
        if !self.has_decorator(node, "Sendable") || !self.sendable_function_or_alias_available(node) { return; }
        self.report_non_sendable_decorators(node, Fault::SendableTypeAliasDecorator);
        let typ = node.type_node().unwrap(); if !ast::is_function_type_node(typ) { self.add(typ, Fault::SendableTypeAliasDeclaration); }
    }
    pub(super) fn is_valid_sendable_class_extends(&mut self, node: P<Node>) -> bool {
        let Some(symbol) = self.checker.get_symbol_at_location(heritage_expression(node), false) else { return true; };
        if symbol.flags().intersects(SymbolFlags::Class) { return true; }
        symbol.flags().intersects(SymbolFlags::Alias) && self.checker.resolve_alias(symbol).flags().intersects(SymbolFlags::Class)
    }
    pub(super) fn scan_captured_variables(&mut self, start: P<Node>, scope: P<Node>, fault: Fault) {
        fn visit(l: &mut ArkTsLinter<'_>, node: P<Node>, start: P<Node>, scope: P<Node>, fault: Fault) {
            if ast::is_identifier(node) {
                if l.checker.get_symbol_at_location(node, false).is_some_and(|s| s.declarations().first().is_some_and(|d| ast::is_namespace_import(*d))) { l.add(node, fault); return; }
                if !node.parent().is_some_and(|p| ast::is_property_access_expression(p) && p.name() == Some(node)) && (!ast::is_function_declaration(start) || start.name() != Some(node)) { l.check_captured_identifier(node, scope, fault); }
            }
            if ast::is_type_reference_node(node) {
                if node.parent().is_some_and(ast::is_heritage_clause) { visit(l, node.as_type_reference_node().type_name, start, scope, fault); }
                return;
            }
            if ast::is_decorator(node) && node.parent() == Some(start) { return; }
            node.for_each_child(&mut |child| { visit(l, child, start, scope, fault); false });
        }
        visit(self, start, start, scope, fault);
    }
    fn check_captured_identifier(&mut self, node: P<Node>, scope: P<Node>, fault: Fault) {
        let Some(symbol) = self.true_symbol_at_location(node) else { return; };
        if symbol.flags() == SymbolFlags::ConstEnum { return; }
        let Some(&declaration) = symbol.declarations().first() else { return; };
        let Some(file) = ast::get_source_file_of_node(declaration).filter(|f| f.file_name() == self.file.file_name()) else { return; };
        let position = get_token_pos_of_node(declaration, file, false);
        let scope_start = get_token_pos_of_node(scope, self.file, false);
        if position >= scope_start
            && position < scope.end().source_offset().unwrap_or(u32::MAX)
            || self.is_exported_declaration(declaration) || self.is_allowed_top_level_sendable_closure(declaration) { return; }
        if matches!(declaration.kind(), Kind::VariableDeclaration | Kind::FunctionDeclaration | Kind::ClassDeclaration | Kind::InterfaceDeclaration | Kind::EnumDeclaration | Kind::ModuleDeclaration | Kind::Parameter) { self.add(node, fault); }
    }
    fn is_allowed_top_level_sendable_closure(&mut self, node: P<Node>) -> bool {
        if !node.parent().is_some_and(ast::is_source_file) { return false; }
        if ast::is_class_declaration(node) {
            let typ = self.checker.get_type_of_node(node); if self.is_sendable_class_or_interface(typ) { return true; }
        }
        ast::is_function_declaration(node) && self.function_has_decorator_in_overloads(node, "Sendable")
    }
    fn is_exported_declaration(&mut self, node: P<Node>) -> bool {
        if self.exported.is_none() { self.build_exported_declarations(); }
        self.exported.as_ref().unwrap().contains(&node)
    }
    fn build_exported_declarations(&mut self) {
        let mut exported = FxHashSet::default();
        for &statement in self.file.statements().nodes() {
            match statement.kind() {
                Kind::ExportAssignment => if !statement.as_export_assignment().is_export_equals {
                    if let Some(symbol) = self.true_symbol_at_location(statement.expression().unwrap()) { exported.extend(symbol.declarations()); }
                },
                Kind::ExportDeclaration => if let Some(clause) = statement.as_export_declaration().export_clause.filter(|c| ast::is_named_exports(*c)) {
                    for specifier in clause.as_named_exports().elements.nodes() {
                        let name = specifier.as_export_specifier().property_name.or_else(|| specifier.name()).unwrap();
                        if let Some(symbol) = self.true_symbol_at_location(name) { exported.extend(symbol.declarations()); }
                    }
                },
                _ => {
                    if !statement.modifier_flags().intersects(ModifierFlags::Export) { continue; }
                    if ast::is_variable_statement(statement) { exported.extend(statement.as_variable_statement().declaration_list.as_variable_declaration_list().declarations.nodes()); }
                    else { exported.insert(statement); }
                }
            }
        }
        self.exported = Some(exported);
    }
    pub(super) fn is_literal_initializer_for_sendable_type(&mut self, contextual_type: P<Type>, node: P<Node>) -> bool {
        if !contextual_type.is_union() { return self.is_sendable_class_or_interface(contextual_type); }
        let literal_type = self.checker.get_type_of_node(node); let mut matched = false;
        for &component in contextual_type.types() {
            if self.checker.is_type_assignable_to(literal_type, component) {
                matched = true; if !self.is_sendable_class_or_interface(component) { return false; }
            }
        }
        matched
    }
    pub(super) fn is_sendable_class_or_interface(&mut self, typ: P<Type>) -> bool {
        let typ = reduce_reference(typ);
        if typ.is_class() && typ.symbol().is_some_and(|s| s.declarations().first().is_some_and(|d| self.has_decorator(*d, "Sendable"))) { return true; }
        fn visit(l: &mut ArkTsLinter<'_>, typ: P<Type>, seen: &mut FxHashSet<P<Type>>) -> bool {
            if !seen.insert(typ) { return false; }
            if typ.symbol().is_some_and(|s| s.declarations().first().is_some_and(|d| is_i_sendable_declaration(*d))) { return true; }
            l.checker.get_base_types(typ).iter().any(|&base| visit(l, base, seen))
        }
        visit(self, typ, &mut FxHashSet::default())
    }
    pub(super) fn is_i_sendable_interface(&self, typ: P<Type>) -> bool {
        typ.alias().and_then(|a| a.symbol()).or_else(|| typ.symbol()).is_some_and(|s| s.declarations().first().is_some_and(|d| is_i_sendable_declaration(*d)))
    }
    pub(super) fn is_sendable_type(&mut self, typ: P<Type>) -> bool {
        typ.flags().intersects(TypeFlags::Boolean | TypeFlags::Number | TypeFlags::String | TypeFlags::BigInt | TypeFlags::Null | TypeFlags::Undefined | TypeFlags::TypeParameter)
            || self.is_sendable_type_alias(typ) || self.is_sendable_function(typ) || self.is_sendable_class_or_interface(typ)
    }
    pub(super) fn is_sendable_type_alias(&mut self, typ: P<Type>) -> bool {
        self.original_type_alias_declaration(typ, &mut FxHashSet::default()).is_some_and(|d| self.has_decorator(d, "Sendable"))
    }
    fn original_type_alias_declaration(&mut self, typ: P<Type>, seen: &mut FxHashSet<P<Symbol>>) -> Option<P<Node>> {
        let symbol = typ.alias()?.symbol()?; if !seen.insert(symbol) { return None; }
        let &declaration = symbol.declarations().first().filter(|d| ast::is_type_alias_declaration(**d))?;
        let node = declaration.type_node().unwrap();
        if ast::is_type_reference_node(node) {
            let target = self.checker.get_type_of_node(node.as_type_reference_node().type_name);
            if target.alias().and_then(|a| a.symbol()).is_some_and(|s| s.flags().intersects(SymbolFlags::TypeAlias)) {
                if let Some(original) = self.original_type_alias_declaration(target, seen) { return Some(original); }
            }
        }
        Some(declaration)
    }
    pub(super) fn is_sendable_function(&mut self, typ: P<Type>) -> bool {
        let Some(declaration) = self.checker.get_signatures_of_type(typ, SignatureKind::Call).first().and_then(|s| s.declaration()).filter(|d| ast::is_function_declaration(*d)) else { return false; };
        self.function_has_decorator_in_overloads(declaration, "Sendable")
    }
    pub(super) fn has_sendable_type_alias(&mut self, typ: P<Type>) -> bool {
        if typ.is_union() { typ.types().iter().any(|&t| self.has_sendable_type_alias(t)) } else { self.is_sendable_type_alias(typ) }
    }
    pub(super) fn is_wrong_sendable_function_assignment(&mut self, left: P<Type>, right: P<Type>) -> bool {
        let left = self.checker.get_non_nullable_type(left); let right = self.checker.get_non_nullable_type(right);
        if !self.has_sendable_type_alias(left) { return false; }
        if right.is_union() { right.types().iter().any(|&t| self.is_invalid_sendable_function_assignment(t)) }
        else { self.is_invalid_sendable_function_assignment(right) }
    }
    fn is_invalid_sendable_function_assignment(&mut self, typ: P<Type>) -> bool {
        if typ.alias().and_then(|a| a.symbol()).is_some() {
            return self.original_type_alias_declaration(typ, &mut FxHashSet::default()).is_some_and(|d| d.type_node().is_some_and(ast::is_function_type_node) && !self.has_decorator(d, "Sendable"));
        }
        !self.checker.get_signatures_of_type(typ, SignatureKind::Call).is_empty() && !self.is_sendable_function(typ)
    }
    pub(super) fn handle_sendable_as_expression(&mut self, node: P<Node>, target: P<Type>, expression: P<Type>) {
        if !self.is_sendable_class_or_interface(expression) && !is_object_type(expression) && !is_type_any(Some(expression)) && self.is_sendable_class_or_interface(target) { self.add(node, Fault::SendableAsExpression); }
        if self.is_wrong_sendable_function_assignment(target, expression) { self.add(node, Fault::SendableFunctionAsExpression); }
    }
    pub(super) fn handle_decorator(&mut self, node: P<Node>) {
        if self.decorator_name(node) != "Sendable" { return; }
        if node.parent().is_none_or(|p| ast::is_struct_declaration(p) || !matches!(p.kind(), Kind::ClassDeclaration | Kind::FunctionDeclaration | Kind::TypeAliasDeclaration)) { self.add(node, Fault::SendableDecoratorLimited); }
    }
    pub(super) fn handle_expression_with_type_arguments(&mut self, node: P<Node>) {
        let Some(symbol) = self.true_symbol_at_location(node.expression().unwrap()) else { return; };
        if symbol.declarations().first().is_some_and(|d| ast::is_type_alias_declaration(*d) && d.name().is_some_and(|n| n.text() == "ESObject") && d.type_node().is_some_and(|t| t.kind() == Kind::AnyKeyword)) { self.add(node, Fault::ESObjectType); }
    }
    pub(super) fn is_taskpool_api(&mut self, symbol: Option<P<Symbol>>, node: P<Node>) -> bool {
        let Some(symbol) = symbol else { return false; };
        if !symbol.declarations().first().is_some_and(|d| ast::get_source_file_of_node(*d).unwrap().is_declaration_file()) || !ast::is_property_access_expression(node) { return false; }
        if !matches!(symbol.name(), "Task" | "LongTask" | "GenericsTask" | "execute" | "addTask") { return false; }
        let Some(base) = self.true_symbol_at_location(node.expression().unwrap()) else { return false; };
        if matches!(base.name(), "taskpool" | "TaskGroup") { return true; }
        let typ = self.checker.get_type_of_symbol(base); self.checker.type_to_string(typ, None) == "TaskGroup"
    }
    pub(super) fn handle_taskpool_call(&mut self, node: P<Node>, symbol: Option<P<Symbol>>) {
        if !self.is_taskpool_api(symbol, node.expression().unwrap()) { return; }
        let Some(&argument) = node.arguments().first() else { return; };
        let typ = self.checker.get_type_of_node(argument);
        let Some(symbol) = typ.symbol().filter(|s| !matches!(s.name(), "Task" | "LongTask" | "GenericsTask" | "TaskGroup")) else { return; };
        if self.invalid_taskpool_function(argument, typ, Some(symbol)) { self.add(argument, Fault::TaskpoolFunctionArg); }
    }
    pub(super) fn handle_taskpool_new(&mut self, node: P<Node>) {
        for &argument in node.arguments().iter().take(2) {
            let typ = self.checker.get_type_of_node(argument);
            if typ.flags().intersects(TypeFlags::StringLike) { continue; }
            if self.invalid_taskpool_function(argument, typ, typ.symbol()) { self.add(argument, Fault::TaskpoolFunctionArg); }
            break;
        }
    }
    fn invalid_taskpool_function(&mut self, argument: P<Node>, typ: P<Type>, symbol: Option<P<Symbol>>) -> bool {
        let declaration_symbol = symbol.is_some_and(|s| s.declarations().first().is_some_and(|d| ast::get_source_file_of_node(*d).unwrap().is_declaration_file()));
        let declaration = self.true_symbol_at_location(argument).and_then(|s| s.declarations().first().copied());
        if declaration_symbol {
            return declaration.is_some_and(|d| d.kind() == Kind::PropertySignature && d.name().is_some_and(|n| n.text() == "constructor") && d.parent().is_some_and(|p| ast::is_interface_declaration(p) && p.name().unwrap().text() == "Object") && tspath::get_base_file_name(ast::get_source_file_of_node(d).unwrap().file_name()).eq_ignore_ascii_case("lib.es5.d.ts"));
        }
        if symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Function)) { return !self.is_concurrent_function(typ); }
        declaration.is_some_and(|d| ast::is_method_declaration(d) || ast::is_class_declaration(d))
    }
    fn is_concurrent_function(&mut self, typ: P<Type>) -> bool {
        let Some(declaration) = self.checker.get_signatures_of_type(typ, SignatureKind::Call).first().and_then(|s| s.declaration()).filter(|d| ast::is_function_declaration(*d)) else { return false; };
        if declaration.symbol().is_some_and(|s| s.declarations().iter().any(|&d| ast::is_function_declaration(d) && self.has_decorator(d, "Concurrent"))) { return true; }
        declaration.body().filter(|b| ast::is_block(*b)).and_then(|b| b.statements().first().copied()).is_some_and(|s| ast::is_expression_statement(s) && s.expression().is_some_and(|e| ast::is_string_literal(e) && e.text().trim() == "use concurrent"))
    }
    pub(super) fn in_shared_module(&self) -> bool {
        self.file.statements().nodes().iter().find(|s| !ast::is_import_declaration(**s)).is_some_and(|s| ast::is_expression_statement(*s) && s.expression().is_some_and(|e| ast::is_string_literal(e) && e.text() == "use shared"))
    }
    pub(super) fn handle_shared_export_assignment(&mut self, node: P<Node>) {
        if self.in_shared_module() { let expression = node.expression().unwrap(); if !self.is_shareable_entity(expression) { self.add(expression, Fault::SharedModuleExports); } }
    }
    pub(super) fn handle_shared_export_keyword(&mut self, node: P<Node>) {
        let Some(declaration) = node.parent() else { return; };
        if !self.in_shared_module() || declaration.parent().is_some_and(ast::is_module_block) { return; }
        match declaration.kind() {
            Kind::EnumDeclaration => {
                let symbol = declaration.symbol().or_else(|| {
                let name = declaration.name()?;
                self.true_symbol_at_location(name)
            });
                if !symbol.is_some_and(is_const_enum_symbol) { self.add(declaration.name().unwrap(), Fault::SharedModuleExports); }
            }
            Kind::InterfaceDeclaration | Kind::FunctionDeclaration | Kind::ClassDeclaration => {
                let typ = self.checker.get_type_of_node(declaration);
                if !self.is_shareable_type(typ) { self.add(declaration.name().unwrap_or(declaration), Fault::SharedModuleExports); }
            }
            Kind::VariableStatement => for variable in declaration.as_variable_statement().declaration_list.as_variable_declaration_list().declarations.nodes() {
                let name = variable.name().unwrap(); if !self.is_shareable_entity(name) { self.add(name, Fault::SharedModuleExports); }
            },
            Kind::TypeAliasDeclaration => if !self.is_shareable_entity(declaration) { self.add(declaration, Fault::SharedModuleExportsWarning); },
            _ => self.add(declaration, Fault::SharedModuleExports),
        }
    }
    pub(super) fn handle_shared_export_declaration(&mut self, node: P<Node>) {
        if !self.in_shared_module() || node.parent().is_some_and(ast::is_module_block) { return; }
        let Some(clause) = node.as_export_declaration().export_clause else { self.add(node, Fault::SharedModuleNoWildcardExport); return; };
        if ast::is_namespace_export(clause) {
            let name = clause.name().unwrap(); let typ = self.checker.get_type_of_node(name);
            if !self.is_shareable_type(typ) { self.add(name, Fault::SharedModuleExports); } return;
        }
        for specifier in clause.as_named_exports().elements.nodes() {
            let name = specifier.name().unwrap(); if !self.is_shareable_entity(name) { self.add(name, Fault::SharedModuleExports); }
        }
    }
    fn is_shareable_entity(&mut self, node: P<Node>) -> bool {
        let mut declaration = node;
        if !ast::is_declaration(node) { if let Some(d) = self.true_symbol_at_location(node).and_then(|s| s.declarations().first().copied()) { declaration = d; } }
        if let Some(typ) = declaration.type_node().filter(|_| !ast::is_function_like_declaration(declaration)) { return self.is_sendable_type_node(typ, true, &mut FxHashSet::default()); }
        let typ = self.checker.get_type_of_node(declaration); self.is_shareable_type(typ)
    }
    fn is_shareable_type(&mut self, typ: P<Type>) -> bool {
        if typ.symbol().is_some_and(|s| is_const_enum_symbol(s) || s.flags().intersects(SymbolFlags::EnumMember) && s.parent().is_some_and(is_const_enum_symbol)) { return true; }
        if typ.is_union() { return typ.types().iter().all(|&t| self.is_shareable_type(t)); }
        if typ.flags().intersects(TypeFlags::BooleanLiteral | TypeFlags::NumberLiteral | TypeFlags::StringLiteral | TypeFlags::BigIntLiteral) && !typ.flags().intersects(TypeFlags::EnumLiteral) { return true; }
        self.is_sendable_type(typ)
    }
}

fn is_arkui_decorator(name: &str) -> bool {
    matches!(name, "Entry" | "Component" | "Reusable" | "CustomDialog" | "Consume" | "Link" | "LocalStorageLink" | "LocalStorageProp" | "ObjectLink" | "Prop" | "Provide" | "State" | "StorageLink" | "StorageProp" | "Builder" | "LocalBuilder" | "BuilderParam" | "Observed" | "Require" | "Sendable" | "Track" | "ComponentV2" | "ObservedV2" | "Trace" | "Local" | "Param" | "Once" | "Event" | "Monitor" | "Provider" | "Consumer" | "Computed" | "Type" | "Env")
}
fn is_i_sendable_declaration(node: P<Node>) -> bool {
    ast::is_interface_declaration(node) && node.name().is_some_and(|n| n.text() == "ISendable")
        && node.parent().filter(|p| ast::is_module_block(*p)).and_then(|p| p.parent()).and_then(|p| p.name()).is_some_and(|n| n.text() == "lang")
        && ast::get_source_file_of_node(node).is_some_and(|f| tspath::get_base_file_name(f.file_name()).eq_ignore_ascii_case("@arkts.lang.d.ets"))
}
pub(super) fn is_object_type(typ: P<Type>) -> bool {
    typ.flags().intersects(TypeFlags::NonPrimitive) || typ.object_flags().intersects(ObjectFlags::ClassOrInterface) && typ.symbol().is_some_and(|s| s.name() == "Object")
}
