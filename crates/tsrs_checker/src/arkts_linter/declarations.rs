use super::*;

impl ArkTsLinter<'_> {
    pub(super) fn handle_parameter(&mut self, node: P<Node>) {
        self.handle_decorators_in_sendable_class(node);
        if node.name().is_some_and(ast::is_binding_pattern) { self.add(node, Fault::DestructuringParameter); }
        for modifier in node.modifier_nodes() {
            if matches!(modifier.kind(), Kind::PublicKeyword | Kind::ProtectedKeyword | Kind::ReadonlyKeyword | Kind::PrivateKeyword) {
                self.add(node, Fault::ParameterProperties); return;
            }
        }
        self.handle_inferred_declaration(node);
    }
    pub(super) fn decorators(&self, node: P<Node>) -> impl Iterator<Item = P<Node>> {
        node.modifier_nodes().iter().copied().filter(|modifier| ast::is_decorator(*modifier))
    }
    pub(super) fn has_decorator(&self, node: P<Node>, name: &str) -> bool {
        self.decorators(node).any(|decorator| self.decorator_name(decorator) == name)
    }
    pub(super) fn decorator_name(&self, node: P<Node>) -> &'static str {
        if !ast::is_decorator(node) { return ""; }
        let mut expression = node.expression().unwrap();
        if ast::is_call_expression(expression) { expression = expression.expression().unwrap(); }
        if ast::is_identifier(expression) { expression.text() } else { "" }
    }
    pub(super) fn filter_property_initialization_diagnostic(&mut self, node: P<Node>) {
        let Some(name) = node.name() else { return; };
        let property_decorator = self.decorators(node).any(|d| matches!(self.decorator_name(d), "Link" | "Consume" | "ObjectLink" | "Prop" | "BuilderParam" | "Param" | "Event" | "Require" | "Env"));
        let class_decorator = node.type_node().is_some_and(|t| self.node_text(t) == "CustomDialogController")
            && node.parent().is_some_and(|p| self.has_decorator(p, "CustomDialog"));
        if property_decorator || class_decorator {
            let start = get_token_pos_of_node(name, self.file, false);
            self.filter_strict_diagnostics(2564, start, start);
        }
    }
    pub(super) fn filter_method_return_diagnostic(&mut self, node: P<Node>) {
        if !self.decorators(node).any(|d| matches!(self.decorator_name(d), "AnimatableExtend" | "Builder" | "Extend" | "Styles")) { return; }
        let begin = node.parameter_list().unwrap().end().source_offset().unwrap_or(0);
        let end = node.body().map_or(begin, |body| get_token_pos_of_node(body, self.file, false));
        self.filter_strict_diagnostics(2366, begin, end);
    }
    pub(super) fn handle_import_declaration(&mut self, node: P<Node>) {
        for &statement in node.parent().unwrap().as_source_file().statements().nodes() {
            if statement == node { break; }
            if !ast::is_import_declaration(statement) { self.add(node, Fault::ImportAfterStatement); break; }
        }
        if let Some(attributes) = node.as_import_declaration().attributes() { self.add(attributes, Fault::ImportAssertion); }
        if self.in_shared_module() && node.as_import_declaration().import_clause.is_none() { self.add(node, Fault::SharedNoSideEffectImport); }
    }
    pub(super) fn handle_function_expression(&mut self, node: P<Node>) {
        self.add(node, Fault::FunctionExpression);
        if asterisk_token(node).is_some() { self.add(node, Fault::GeneratorFunction); }
        self.handle_standalone_this(node, node.body()); self.handle_missing_return_type(node);
    }
    pub(super) fn handle_function_declaration(&mut self, node: P<Node>) {
        if let Some(name) = node.name() { self.count_duplicate_name(name, node, node.kind()); }
        self.handle_missing_return_type(node); self.report_this_keywords(node.body());
        if node.parent().is_some_and(|p| !matches!(p.kind(), Kind::SourceFile | Kind::ModuleBlock)) { self.add(node, Fault::LocalFunction); }
        if asterisk_token(node).is_some() { self.add(node, Fault::GeneratorFunction); }
        if self.function_has_decorator_in_overloads(node, "Sendable") {
            if !self.sendable_function_or_alias_available(node) { return; }
            self.report_non_sendable_decorators(node, Fault::SendableFunctionDecorator);
            if !self.has_decorator(node, "Sendable") { self.add(node, Fault::SendableFunctionOverloadDecorator); }
            self.scan_captured_variables(node, node, Fault::SendableFunctionImportedVariables);
        }
    }
    pub(super) fn function_has_decorator_in_overloads(&mut self, node: P<Node>, name: &str) -> bool {
        let symbol = node.symbol().or_else(|| {
            let name = node.name()?;
            self.checker.get_symbol_at_location(name, false)
        });
        let Some(symbol) = symbol else { return self.has_decorator(node, name); };
        symbol.declarations().iter().any(|&d| ast::is_function_declaration(d) && self.has_decorator(d, name))
    }
    pub(super) fn handle_standalone_this(&mut self, node: P<Node>, body: Option<P<Node>>) {
        if node.parent().is_none_or(|p| {
            ast::find_ancestor(p, |n| ast::is_class_like(n) || ast::is_interface_declaration(n))
                .is_none()
        }) { self.report_this_keywords(body); }
    }
    pub(super) fn report_this_keywords(&mut self, scope: Option<P<Node>>) {
        fn visit(l: &mut ArkTsLinter<'_>, node: P<Node>, scope: P<Node>) {
            if node != scope && (ast::is_class_like(node) || matches!(node.kind(), Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::ModuleDeclaration)) { return; }
            if node.kind() == Kind::ThisKeyword { l.add(node, Fault::FunctionContainsThis); }
            node.for_each_child(&mut |child| { visit(l, child, scope); false });
        }
        if let Some(scope) = scope { visit(self, scope, scope); }
    }
    pub(super) fn handle_missing_return_type(&mut self, node: P<Node>) {
        if node.type_node().is_some() { return; }
        if node.kind() == Kind::MethodSignature || node.body().is_none() {
            if node.kind() == Kind::MethodSignature || node.flags().intersects(NodeFlags::Ambient) { self.add(node, Fault::LimitedReturnTypeInference); }
            return;
        }
        if self.has_call_with_omitted_return_type(node.body().unwrap()) { self.add(node, Fault::LimitedReturnTypeInference); return; }
        let signature = self.checker.get_signature_from_declaration(node);
        let return_type = self.checker.get_return_type_of_signature(signature);
        if return_type.flags().intersects(TypeFlags::Any | TypeFlags::Unknown | TypeFlags::Intersection) { self.add(node, Fault::LimitedReturnTypeInference); }
    }
    fn has_call_with_omitted_return_type(&mut self, body: P<Node>) -> bool {
        if !ast::is_block(body) {
            let expression = ast::skip_parentheses(body);
            if ast::is_call_expression(expression) { return self.call_omits_return_type(expression); }
        }
        fn visit(l: &mut ArkTsLinter<'_>, node: P<Node>, body: P<Node>) -> bool {
            if node != body && (ast::is_function_like(node) || ast::is_class_like(node)) { return false; }
            if ast::is_return_statement(node) {
                if let Some(expression) = node.expression().map(ast::skip_parentheses).filter(|e| ast::is_call_expression(*e)) {
                    if l.call_omits_return_type(expression) { return true; }
                }
            }
            node.for_each_child(&mut |child| visit(l, child, body))
        }
        visit(self, body, body)
    }
    fn call_omits_return_type(&mut self, expression: P<Node>) -> bool {
        self.checker.get_resolved_signature(expression, None, CheckMode::Normal).declaration().is_none_or(|d| d.type_node().is_none())
    }
    pub(super) fn handle_variable_declaration(&mut self, node: P<Node>) {
        if let Some(name) = node.name() {
            if ast::is_binding_pattern(name) { self.add(node, Fault::DestructuringDeclaration); }
            self.count_binding_name_duplicates(name);
        }
        self.handle_inferred_declaration(node); self.handle_definite_assignment(node);
        if let (Some(type_node), Some(initializer)) = (node.type_node(), node.initializer()) {
            let typ = self.checker.get_type_of_node(type_node); self.check_assignment_matching(node, typ, initializer, false);
        }
        if let Some(initializer) = node.initializer() { self.handle_es_object_assignment(node, node.type_node(), initializer, false); }
    }
    pub(super) fn handle_property_assignment_compatibility(&mut self, node: P<Node>) {
        let (Some(type_node), Some(initializer)) = (node.type_node(), node.initializer()) else { return; };
        let typ = self.checker.get_type_of_node(type_node); self.check_assignment_matching(node, typ, initializer, true);
        self.handle_es_object_assignment(node, Some(type_node), initializer, true);
    }
    pub(super) fn handle_inferred_declaration(&mut self, node: P<Node>) {
        if node.type_node().is_some() || node.parent().is_some_and(ast::is_catch_clause) || node.name().is_some_and(ast::is_binding_pattern) { return; }
        if node.initializer().is_none() && (ast::is_property_declaration(node) || node.parent().and_then(|p| p.parent()).is_some_and(ast::is_variable_statement)) {
            if ast::is_property_declaration(node) && self.file.is_declaration_file() && self.file.script_kind() == ScriptKind::ETS && node.modifier_flags().intersects(ModifierFlags::Private) { return; }
            self.add(node, Fault::AnyType); return;
        }
        let typ = self.checker.get_type_of_node(node);
        if self.contains_any_or_unknown(typ, &mut FxHashSet::default()) {
            self.add(node, if typ.flags().intersects(TypeFlags::Unknown) { Fault::UnknownType } else { Fault::AnyType });
        }
    }
    fn contains_any_or_unknown(&mut self, typ: P<Type>, visited: &mut FxHashSet<P<Type>>) -> bool {
        if typ.alias().is_some() || !visited.insert(typ) { return false; }
        if typ.flags().intersects(TypeFlags::Any | TypeFlags::Unknown) { return true; }
        if typ.flags().intersects(TypeFlags::Union | TypeFlags::Intersection) && typ.types().iter().any(|&t| self.contains_any_or_unknown(t, visited)) { return true; }
        typ.object_flags().intersects(ObjectFlags::Reference) && self.checker.get_type_arguments(typ).iter().any(|&t| self.contains_any_or_unknown(t, visited))
    }
    pub(super) fn handle_definite_assignment(&mut self, node: P<Node>) {
        let token = match node.kind() {
            Kind::VariableDeclaration => node.as_variable_declaration().exclamation_token(),
            Kind::PropertyDeclaration => node.as_property_declaration().postfix_token().filter(|t| t.kind() == Kind::ExclamationToken),
            _ => None,
        };
        if token.is_none() { return; }
        let sendable = ast::is_property_declaration(node) && node.parent().is_some_and(|p| ast::is_class_declaration(p) && self.has_decorator(p, "Sendable"));
        self.add(node, if sendable { Fault::SendableDefiniteAssignment } else { Fault::DefiniteAssignment });
    }
    pub(super) fn handle_class_declaration(&mut self, node: P<Node>) {
        if let Some(name) = node.name() { self.count_duplicate_name(name, node, node.kind()); }
        self.count_class_member_duplicate_names(node);
        let sendable = !ast::is_struct_declaration(node) && self.has_decorator(node, "Sendable");
        if sendable { self.report_non_sendable_decorators(node, Fault::SendableClassDecorator); }
        if self.checker.compiler_options.skip_ark_ts_static_blocks_check != Tristate::True {
            let mut seen = false;
            for &member in node.members() {
                if ast::is_class_static_block_declaration(member) { if seen { self.add(member, Fault::MultipleStaticBlocks); } seen = true; }
            }
        }
        if let Some(clauses) = node.as_class_declaration().heritage_clauses() {
            for clause in clauses.nodes() {
                let clause = clause.as_heritage_clause();
                for &heritage_type in clause.types().nodes() {
                    let typ = self.checker.get_type_of_node(heritage_type); let typ = reduce_reference(typ);
                    let base_sendable = self.is_sendable_class_or_interface(typ);
                    if clause.token == Kind::ImplementsKeyword && typ.is_class() { self.add(heritage_type, Fault::ImplementsClass); }
                    if !sendable { if base_sendable { self.add(heritage_type, Fault::SendableClassInheritance); } continue; }
                    if clause.token == Kind::ExtendsKeyword && (!base_sendable || !self.is_valid_sendable_class_extends(heritage_type)) { self.add(heritage_type, Fault::SendableClassInheritance); }
                }
            }
        }
        if sendable { for &member in node.members() { self.scan_captured_variables(member, node, Fault::SendableCapturedVars); } }
    }
    pub(super) fn handle_interface_declaration(&mut self, node: P<Node>) {
        let name = node.name().unwrap(); let symbol = self.checker.get_symbol_at_location(name, false);
        if count_declaration_kind(symbol, Kind::InterfaceDeclaration) > 1 { self.add(node, Fault::InterfaceMerging); }
        self.count_duplicate_name(name, node, node.kind());
        let Some(clauses) = node.as_interface_declaration().heritage_clauses else { return; };
        for clause in clauses.nodes() {
            let clause = clause.as_heritage_clause();
            if clause.token != Kind::ExtendsKeyword { continue; }
            let mut property_types = FxHashMap::default();
            for &heritage_type in clause.types().nodes() {
                let typ = self.checker.get_type_of_node(heritage_type);
                if typ.is_class() { self.add(heritage_type, Fault::InterfaceExtendsClass); }
                else if typ.object_flags().intersects(ObjectFlags::ClassOrInterface) { self.check_inherited_property_types(node, typ, &mut property_types); }
            }
        }
    }
    fn check_inherited_property_types(&mut self, interface: P<Node>, inherited: P<Type>, property_types: &mut FxHashMap<&'static str, &'static str>) {
        for &property in self.checker.get_properties_of_type(inherited) {
            let Some(&declaration) = property.declarations().first() else { continue; };
            if !matches!(declaration.kind(), Kind::MethodSignature | Kind::MethodDeclaration | Kind::PropertyDeclaration | Kind::PropertySignature) { continue; }
            let Some(typ) = declaration.type_node() else { continue; };
            let text = node_text(typ);
            if let Some(&previous) = property_types.get(property.name()) { if previous != text { self.add(interface, Fault::InterfaceExtendDifferentProperties); } }
            else { property_types.insert(property.name(), text); }
        }
    }
    pub(super) fn handle_enum_declaration(&mut self, node: P<Node>) {
        let name = node.name().unwrap(); let symbol = self.checker.get_symbol_at_location(name, false);
        self.count_duplicate_name(name, node, node.kind());
        if count_declaration_kind(symbol, Kind::EnumDeclaration) > 1 { self.add(node, Fault::EnumMerging); }
    }
    pub(super) fn handle_module_declaration(&mut self, node: P<Node>) {
        let declaration = node.as_module_declaration(); let name = node.name().unwrap();
        self.count_duplicate_name(name, node, node.kind());
        if let Some(body) = declaration.body().filter(|b| ast::is_module_block(*b)) {
            for &statement in body.as_module_block().statements.nodes() {
                if !matches!(statement.kind(), Kind::VariableStatement | Kind::FunctionDeclaration | Kind::ClassDeclaration | Kind::InterfaceDeclaration | Kind::TypeAliasDeclaration | Kind::EnumDeclaration | Kind::ExportDeclaration | Kind::ModuleDeclaration) { self.add(statement, Fault::NonDeclarationInNamespace); }
            }
        }
        if declaration.keyword != Kind::NamespaceKeyword && node.modifier_flags().intersects(ModifierFlags::Ambient) { self.add(node, Fault::ShorthandAmbientModuleDecl); }
        if ast::is_string_literal(name) && name.text().contains('*') { self.add(node, Fault::WildcardsInModuleName); }
    }
    pub(super) fn count_duplicate_name(&mut self, name: P<Node>, declaration: P<Node>, kind: Kind) {
        if self.checker.get_symbol_at_location(name, false).is_some_and(|symbol| self.symbol_has_duplicate_name(symbol, kind)) { self.add(declaration, Fault::DeclWithDuplicateName); }
    }
    fn count_binding_name_duplicates(&mut self, name: P<Node>) {
        if ast::is_identifier(name) { self.count_duplicate_name(name, name, name.parent().unwrap().kind()); return; }
        if !ast::is_binding_pattern(name) { return; }
        name.for_each_child(&mut |child| { if ast::is_binding_element(child) { if let Some(name) = child.name() { self.count_binding_name_duplicates(name); } } false });
    }
    fn count_class_member_duplicate_names(&mut self, class: P<Node>) {
        for &current in class.members() {
            let Some(name) = current.name().filter(|n| matches!(n.kind(), Kind::Identifier | Kind::PrivateIdentifier)) else { continue; };
            for &other in class.members() {
                if current == other { continue; }
                let Some(other_name) = other.name().filter(|n| matches!(n.kind(), Kind::Identifier | Kind::PrivateIdentifier)) else { continue; };
                if ast::is_private_identifier(name) == ast::is_private_identifier(other_name) { continue; }
                if name.text().trim_start_matches('#') == other_name.text().trim_start_matches('#') { self.add(current, Fault::DeclWithDuplicateName); break; }
            }
        }
    }
    pub(super) fn is_allowed_index_signature(&mut self, node: P<Node>) -> bool {
        if node.parameters().len() != 1 { return false; }
        if !self.checker.get_type_of_node(node.parameters()[0]).flags().intersects(TypeFlags::Number) { return false; }
        let Some(name) = node.parent().and_then(|p| p.name()) else { return false; };
        self.true_symbol_at_location(name).is_some_and(|s| self.is_arkts_collections_declaration(s.declarations()))
    }
}

fn count_declaration_kind(symbol: Option<P<Symbol>>, kind: Kind) -> usize {
    symbol.map_or(0, |s| s.declarations().iter().filter(|d| d.kind() == kind).count())
}
pub(super) fn heritage_expression(node: P<Node>) -> P<Node> {
    match node.kind() { Kind::ExpressionWithTypeArguments => node.expression().unwrap(), Kind::TypeReference => node.as_type_reference_node().type_name, _ => node }
}
pub(super) fn reduce_reference(typ: P<Type>) -> P<Type> {
    if typ.flags().intersects(TypeFlags::Object) && typ.object_flags().intersects(ObjectFlags::Reference) { typ.as_object_type().target.get().unwrap_or(typ) } else { typ }
}
pub(super) fn node_text(node: P<Node>) -> &'static str {
    let Some(file) = ast::get_source_file_of_node(node) else { return ""; };
    file
        .text()
        .get(
            get_token_pos_of_node(node, file, false) as usize
                ..node.end().source_offset().unwrap_or(0) as usize,
        )
        .unwrap_or_default()
}
pub(super) fn asterisk_token(node: P<Node>) -> Option<P<Node>> {
    match node.kind() {
        Kind::FunctionDeclaration => node.as_function_declaration().asterisk_token(),
        Kind::FunctionExpression => node.as_function_expression().asterisk_token(),
        Kind::MethodDeclaration => node.as_method_declaration().asterisk_token(),
        _ => None,
    }
}
