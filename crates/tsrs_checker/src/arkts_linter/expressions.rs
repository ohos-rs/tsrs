use super::*;

impl ArkTsLinter<'_> {
    pub(super) fn handle_object_literal_expression(&mut self, node: P<Node>) {
        if is_destructuring_assignment_lhs(node) { return; }
        let typ = self.checker.get_contextual_type(node, ContextFlags::None);
        if typ.is_some_and(|t| self.is_literal_initializer_for_sendable_type(t, node)) { self.add(node, Fault::SendableObjectInitialization); return; }
        if !self.is_struct_object_initializer(node) && !self.is_dynamic_literal_initializer(node) && !self.is_object_literal_assignable(typ, node) { self.add(node, Fault::ObjectLiteralNoContextType); }
    }
    pub(super) fn handle_array_literal_expression(&mut self, node: P<Node>) {
        if is_destructuring_assignment_lhs(node) { return; }
        let typ = self.checker.get_contextual_type(node, ContextFlags::None);
        if typ.is_some_and(|t| self.is_literal_initializer_for_sendable_type(t, node)) { self.add(node, Fault::SendableObjectInitialization); return; }
        for &element in node.as_array_literal_expression().elements.nodes() {
            if ast::is_omitted_expression(element) { continue; }
            let typ = self.checker.get_contextual_type(element, ContextFlags::None);
            if ast::is_object_literal_expression(element) && !self.is_dynamic_literal_initializer(node) && !self.is_object_literal_assignable(typ, element) { self.add(node, Fault::ArrayLiteralNoContextType); return; }
            if let Some(typ) = typ { self.check_assignment_matching(element, typ, element, true); }
        }
    }
    fn is_struct_object_initializer(&mut self, node: P<Node>) -> bool {
        let Some(parent) = node.parent().filter(|p| ast::is_call_like_expression(*p)) else { return false; };
        self.checker.get_resolved_signature(parent, None, CheckMode::Normal).declaration().is_some_and(|d| ast::is_constructor_declaration(d) && d.parent().is_some_and(ast::is_struct_declaration))
    }
    pub(super) fn handle_prefix_unary_expression(&mut self, node: P<Node>) {
        let expression = node.as_prefix_unary_expression();
        if !matches!(expression.operator, Kind::PlusToken | Kind::MinusToken | Kind::TildeToken) { return; }
        if !self.checker.get_type_of_node(expression.operand).flags().intersects(TypeFlags::NumberLike | TypeFlags::BigIntLike) { self.add(node, Fault::UnaryArithmNotNumber); }
    }
    pub(super) fn handle_binary_expression(&mut self, node: P<Node>) {
        let expression = node.as_binary_expression(); let operator = expression.operator_token.kind();
        if is_assignment_operator(operator) {
            if matches!(expression.left.kind(), Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression) { self.add(node, Fault::DestructuringAssignment); }
            if ast::is_property_access_expression(expression.left) {
                let symbol = self.true_symbol_at_location(expression.left); let assignment = is_method_assignment(symbol);
                if symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Method) || assignment) { self.add(expression.left, Fault::MethodReassignment); }
                let base = self.true_symbol_at_location(expression.left.expression().unwrap());
                if assignment && base.is_some_and(|s| s.flags().intersects(SymbolFlags::Function)) { self.add(expression.left, Fault::PropertyDeclOnFunction); }
            }
        }
        if operator == Kind::EqualsToken {
            let left = self.checker.get_type_of_node(expression.left); self.check_assignment_matching(node, left, expression.right(), false);
            let declared_type = self.variable_declaration_type_node(expression.left); self.handle_es_object_assignment(node, declared_type, expression.right(), false);
        }
        match operator {
            Kind::CommaToken => {
                let mut root = node; while let Some(parent) = root.parent().filter(|p| ast::is_binary_expression(*p)) { root = parent; }
                if let Some(parent) = root.parent().filter(|p| ast::is_for_statement(*p)) {
                    let statement = parent.as_for_statement(); if statement.initializer == Some(root) || statement.incrementor == Some(root) { return; }
                }
                self.add(node, Fault::CommaOperator);
            }
            Kind::InKeyword => self.add(expression.operator_token, Fault::InOperator),
            Kind::InstanceOfKeyword => {
                if expression.left.kind() == Kind::ThisKeyword { return; }
                let left = ast::skip_parentheses(expression.left); let typ = self.checker.get_type_of_node(expression.left); let symbol = self.true_symbol_at_location(left);
                if typ.flags().intersects(TypeFlags::BooleanLike | TypeFlags::NumberLike) || ast::is_type_node(left) || symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Class | SymbolFlags::Interface)) { self.add(node, Fault::InstanceofUnsupported); }
            }
            _ => {}
        }
    }
    pub(super) fn handle_identifier(&mut self, node: P<Node>) {
        let Some(symbol) = self.true_symbol_at_location(node) else { return; };
        if node.text() == "globalThis" && symbol.flags().intersects(SymbolFlags::Module) && symbol.flags().intersects(SymbolFlags::Transient) { self.add(node, Fault::GlobalThis); return; }
        if !symbol.flags().intersects(SymbolFlags::ConstEnum | SymbolFlags::RegularEnum | SymbolFlags::ValueModule | SymbolFlags::Class) || symbol.declarations().iter().any(|&d| ast::is_struct_declaration(d)) || !identifier_used_in_value_context(node, symbol) { return; }
        if symbol.flags().intersects(SymbolFlags::ValueModule) && self.symbol_has_duplicate_name(symbol, Kind::ModuleDeclaration) { return; }
        if symbol.flags().intersects(SymbolFlags::Class) && self.is_allowed_class_value_context(node) { return; }
        if symbol.flags().intersects(SymbolFlags::Annotation) { return; }
        self.add(node, if symbol.flags().intersects(SymbolFlags::ValueModule) { Fault::NamespaceAsObject } else { Fault::ClassAsObject });
    }
    fn is_allowed_class_value_context(&mut self, identifier: P<Node>) -> bool {
        let mut root = access_root(identifier);
        if let Some(parent) = root.parent().filter(|p| ast::is_property_assignment(*p)) {
            if let Some(object) = parent.parent().filter(|p| ast::is_object_literal_expression(*p)) { root = object; }
        }
        if let Some(parent) = root.parent().filter(|p| ast::is_arrow_function(*p) && p.body() == Some(root)) { root = parent; }
        let Some(parent) = root.parent().filter(|p| matches!(p.kind(), Kind::CallExpression | Kind::NewExpression)) else { return false; };
        let callee = parent.expression().unwrap(); if callee == root { return false; }
        let typ = self.checker.get_type_of_node(callee); is_type_any(Some(typ)) || self.is_library_type(typ)
    }
    pub(super) fn handle_property_access(&mut self, node: P<Node>) {
        if node.parent().is_some_and(|p| ast::is_call_expression(p) && p.expression() == Some(node)) { return; }
        if node.name().unwrap().text() == "prototype" {
            let expression = node.expression().unwrap(); let typ = self.checker.get_type_of_node(expression); let symbol = self.checker.get_symbol_at_location(expression, false);
            if symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Class | SymbolFlags::Function)) || is_type_any(Some(typ)) { self.add(node.name().unwrap(), Fault::Prototype); }
        }
        let symbol = self.true_symbol_at_location(node);
        if is_symbol_api(symbol) && symbol.unwrap().name() != "iterator" { self.add(node, Fault::SymbolType); }
        let typ = self.checker.get_type_of_node(node.expression().unwrap());
        if self.is_sendable_function(typ) || self.has_sendable_type_alias(typ) { self.add(node, Fault::SendableFunctionProperty); }
        if let Some(parent) = node.parent().filter(|p| ast::is_new_expression(*p)) { if self.is_taskpool_api(symbol, node) { self.handle_taskpool_new(parent); } }
    }
    pub(super) fn handle_element_access(&mut self, node: P<Node>) {
        let expression = node.as_element_access_expression(); let symbol = self.true_symbol_at_location(expression.expression);
        let typ = self.type_or_constraint(expression.expression); let typ = self.checker.get_non_nullable_type(typ);
        let argument = self.checker.get_type_of_node(expression.argument_expression);
        if self.is_library_symbol(symbol) || ast::is_array_literal_expression(expression.expression) || self.is_element_access_allowed(typ, argument) { return; }
        self.add(node, Fault::PropertyAccessByIndex);
    }
    fn is_element_access_allowed(&mut self, typ: P<Type>, argument: P<Type>) -> bool {
        if typ.is_union() { return typ.types().iter().all(|&t| self.is_element_access_allowed(t, argument)); }
        if self.is_or_derived_from(typ, |l, t| t.symbol().is_some_and(|s| s.name() == "BitVector" && l.is_arkts_collections_declaration(s.declarations()))) { return argument.flags().intersects(TypeFlags::NumberLike); }
        is_type_any(Some(typ)) || self.is_library_type(typ) || self.is_or_derived_from(typ, |l, t| l.is_indexable_array_type(t)) || self.is_or_derived_from(typ, |_, t| t.is_tuple_type()) || self.is_or_derived_from(typ, Self::is_std_record_type) || typ.flags().intersects(TypeFlags::String | TypeFlags::EnumLike | TypeFlags::NonPrimitive) || self.is_named_global_type(typ, "Map") || is_es_object_type(typ)
    }
    fn handle_call_argument_assignments(&mut self, call: P<Node>, signature: P<Signature>) {
        let parameters = signature.parameters(); if parameters.is_empty() { return; }
        for (index, &argument) in call.arguments().iter().enumerate() {
            let parameter = parameters[index.min(parameters.len() - 1)];
            let Some(declaration) = parameter.value_declaration().filter(|d| ast::is_parameter_declaration(*d)) else { continue; };
            let Some(mut typ) = self.checker.get_type_of_symbol_at_location(parameter, Some(declaration)) else { continue; };
            if declaration.as_parameter_declaration().dot_dot_dot_token().is_some() { typ = self.checker.get_element_type_of_array_type(typ).unwrap_or(typ); }
            self.check_assignment_matching(argument, typ, argument, false);
        }
    }
    pub(super) fn handle_return_statement(&mut self, node: P<Node>) {
        if let Some(expression) = node.expression() {
            if let Some(typ) = self.checker.get_contextual_type(expression, ContextFlags::None) { self.check_assignment_matching(node, typ, expression, true); }
        }
    }
    pub(super) fn check_assignment_matching(&mut self, field: P<Node>, left: P<Type>, expression: P<Node>, missed_structural: bool) {
        let right = self.checker.get_type_of_node(expression);
        if self.is_wrong_sendable_function_assignment(left, right) { self.add(field, Fault::SendableFunctionAssignment); }
        let strict = self.needs_strict_structural_match(left, right); if missed_structural && !strict { return; }
        if self.needs_structural_identity(left, right, expression, strict) { self.add(field, Fault::StructuralIdentity); }
    }
    fn is_es_object_possibly_allowed(&self, node: P<Node>) -> bool {
        let Some(parent) = node.parent() else { return false; };
        match parent.kind() {
            Kind::VariableDeclaration | Kind::PropertyDeclaration | Kind::Parameter | Kind::FunctionType | Kind::PropertySignature | Kind::ArrayType | Kind::NewExpression => true,
            Kind::ArrowFunction | Kind::FunctionDeclaration | Kind::MethodDeclaration => !function_returns_object_literal(parent, false),
            Kind::TypeReference => entity_name_to_string(parent.as_type_reference_node().type_name) != "Promise" || parent.parent().is_none_or(|p| !function_returns_object_literal(p, true)),
            Kind::AsExpression => !ast::is_object_literal_expression(parent.expression().unwrap()),
            _ => false,
        }
    }
    fn variable_declaration_type_node(&mut self, node: P<Node>) -> Option<P<Node>> {
        let symbol = self.true_symbol_at_location(node)?;
        let declaration = symbol
            .declarations()
            .first()
            .copied()
            .filter(|d| ast::is_variable_declaration(*d))?;
        declaration.type_node()
    }
    pub(super) fn handle_es_object_assignment(&mut self, node: P<Node>, declared_type: Option<P<Node>>, initializer: P<Node>, property: bool) {
        let Some(declared_type) = declared_type else { return; };
        if ast::is_type_reference_node(declared_type) && entity_name_to_string(declared_type.as_type_reference_node().type_name) == "ESObject" {
            if ast::is_object_literal_expression(initializer) { self.add(node, Fault::ESObjectType); } return;
        }
        if property { return; }
        if self.variable_declaration_type_node(initializer).is_some_and(|t| ast::is_type_reference_node(t) && entity_name_to_string(t.as_type_reference_node().type_name) == "ESObject") { self.add(node, Fault::ESObjectType); }
    }
    pub(super) fn handle_enum_member(&mut self, node: P<Node>) {
        let count = self.diagnostics.len();
        if let Some(initializer) = node.initializer() {
            if !matches!(initializer.kind(), Kind::StringLiteral | Kind::NumericLiteral | Kind::PrefixUnaryExpression) && self.checker.evaluate(initializer, node).value.is_none() { self.add(node, Fault::EnumMemberNonConstInit); }
        }
        let Some(parent) = node.parent().filter(|p| ast::is_enum_declaration(*p)) else { return; };
        let Some(&first) = parent.as_enum_declaration().members.nodes().first() else { return; };
        let current_value = self.checker.get_constant_value(node); let first_value = self.checker.get_constant_value(first);
        if matches!((current_value, first_value), (Some(LiteralValue::String(_)), Some(LiteralValue::String(_))) | (Some(LiteralValue::Number(_)), Some(LiteralValue::Number(_)))) { return; }
        if self.diagnostics.len() == count && self.checker.get_type_of_node(first) != self.checker.get_type_of_node(node) { self.add(node, Fault::EnumMemberNonConstInit); }
    }
    pub(super) fn handle_type_reference(&mut self, node: P<Node>) {
        let reference = node.as_type_reference_node(); let name = entity_name_to_string(reference.type_name);
        if name == "ESObject" { if !self.is_es_object_possibly_allowed(node) { self.add(node, Fault::ESObjectType); } return; }
        if matches!(name.as_str(), "Awaited" | "Pick" | "Omit" | "Exclude" | "Extract" | "NonNullable" | "Parameters" | "ConstructorParameters" | "ReturnType" | "InstanceType" | "ThisParameterType" | "OmitThisParameter" | "ThisType" | "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize") { self.add(node, Fault::UtilityType); return; }
        if name == "Partial" {
            if let Some(arguments) = reference.type_arguments().filter(|a| a.nodes().len() == 1) {
                if !self.checker.get_type_from_type_node(arguments.nodes()[0]).object_flags().intersects(ObjectFlags::ClassOrInterface) { self.add(node, Fault::UtilityType); }
            }
        }
        let typ = self.checker.get_type_from_type_node(node);
        if self.is_sendable_class_or_interface(typ) {
            if let Some(arguments) = reference.type_arguments() {
                for &argument in arguments.nodes() { if !self.is_sendable_type_node(argument, false, &mut FxHashSet::default()) { self.add(argument, Fault::SendableGenericTypes); } }
            }
        }
    }
    pub(super) fn handle_call_expression(&mut self, node: P<Node>) {
        let expression = node.expression().unwrap(); let symbol = self.true_symbol_at_location(expression);
        if expression.kind() == Kind::ImportKeyword && node.arguments().len() > 1 && ast::is_object_literal_expression(node.arguments()[1]) {
            for &property in node.arguments()[1].as_object_literal_expression().properties.nodes() { if property.name().is_some_and(|n| n.text() == "assert") { self.add(property, Fault::ImportAssertion); } }
        }
        if ast::is_identifier(expression) && expression.text() == "require" && node.parent().is_some_and(ast::is_variable_declaration) {
            let typ = self.checker.get_type_of_node(expression);
            if typ.symbol().is_some_and(|s| s.flags().intersects(SymbolFlags::Interface) && s.name() == "NodeRequire") { self.add(node.parent().unwrap(), Fault::ImportAssignment); }
        }
        if let Some(symbol) = symbol {
            match self.checker.get_fully_qualified_name_exported(symbol).as_str() {
                "Function.apply" | "Function.call" | "CallableFunction.apply" | "CallableFunction.call" => self.add(node, Fault::FunctionApplyCall),
                "Function.bind" | "CallableFunction.bind" => self.add(node, Fault::FunctionBind), _ => {}
            }
            self.handle_stdlib_api_call(node, symbol);
        }
        let signature = self.checker.get_resolved_signature(node, None, CheckMode::Normal);
        if !self.is_library_symbol(symbol) { self.handle_generic_call_with_signature(node, signature); self.handle_call_argument_assignments(node, signature); }
        self.handle_taskpool_call(node, symbol); self.filter_library_call_diagnostics(node, symbol);
    }
    fn handle_stdlib_api_call(&mut self, node: P<Node>, symbol: P<Symbol>) {
        let name = symbol.name(); let parent = symbol.parent().map_or("", |p| p.name());
        let fault = match parent {
            "" => if name == "eval" { Some(Fault::LimitedStdLibAPI) } else if matches!(name, "Symbol" | "SymbolConstructor") { Some(Fault::SymbolType) } else { None },
            "Object" | "ObjectConstructor" if matches!(name, "__proto__" | "__defineGetter__" | "__defineSetter__" | "__lookupGetter__" | "__lookupSetter__" | "assign" | "create" | "defineProperties" | "defineProperty" | "freeze" | "fromEntries" | "getOwnPropertyDescriptor" | "getOwnPropertyDescriptors" | "getOwnPropertySymbols" | "getPrototypeOf" | "hasOwnProperty" | "is" | "isExtensible" | "isFrozen" | "isPrototypeOf" | "isSealed" | "preventExtensions" | "propertyIsEnumerable" | "seal" | "setPrototypeOf") => Some(Fault::LimitedStdLibAPI),
            "Reflect" if matches!(name, "apply" | "construct" | "defineProperty" | "deleteProperty" | "getOwnPropertyDescriptor" | "getPrototypeOf" | "isExtensible" | "preventExtensions" | "setPrototypeOf") => Some(Fault::LimitedStdLibAPI),
            "ProxyHandler" if matches!(name, "apply" | "construct" | "defineProperty" | "deleteProperty" | "get" | "getOwnPropertyDescriptor" | "getPrototypeOf" | "has" | "isExtensible" | "ownKeys" | "preventExtensions" | "set" | "setPrototypeOf") => Some(Fault::LimitedStdLibAPI),
            "Symbol" | "SymbolConstructor" => Some(Fault::SymbolType), _ => None,
        };
        if let Some(fault) = fault { self.add(node, fault); }
    }
    pub(super) fn handle_generic_call(&mut self, node: P<Node>) {
        let signature = self.checker.get_resolved_signature(node, None, CheckMode::Normal);
        self.handle_generic_call_with_signature(node, signature); self.handle_call_argument_assignments(node, signature);
        if ast::is_new_expression(node) {
            let typ = self.checker.get_type_of_node(node);
            if self.is_sendable_class_or_interface(typ) {
                for &argument in node.type_arguments() { if !self.is_sendable_type_node(argument, false, &mut FxHashSet::default()) { self.add(argument, Fault::SendableGenericTypes); } }
            }
        }
    }
    fn handle_generic_call_with_signature(&mut self, node: P<Node>, signature: P<Signature>) {
        let Some(mapper) = signature.mapper.get() else { return; };
        let parameters = signature.target().unwrap_or(signature).type_parameters();
        let inferred = self.checker.instantiate_types(parameters, Some(mapper));
        if inferred.iter().skip(node.type_arguments().len()).any(|t| t.flags().intersects(TypeFlags::Unknown)) { self.add(node, Fault::GenericCallNoTypeArgs); }
    }
    pub(super) fn handle_spread(&mut self, node: P<Node>) {
        if ast::is_spread_element(node) && node.parent().is_some_and(|p| ast::is_call_like_expression(p) || ast::is_array_literal_expression(p)) {
            let typ = self.type_or_constraint(node.expression().unwrap());
            if !typ.is_tuple_type() && self.is_or_derived_from(typ, |l, t| l.is_spread_array_type(t)) { return; }
        }
        self.add(node, Fault::SpreadOperator);
    }
    pub(super) fn handle_computed_property_name(&mut self, node: P<Node>) {
        if let Some(declaration) = node.parent().and_then(|p| p.parent()) {
            let sendable = if ast::is_class_declaration(declaration) { self.has_decorator(declaration, "Sendable") } else if ast::is_interface_declaration(declaration) { let typ = self.checker.get_type_of_node(declaration); self.is_sendable_class_or_interface(typ) } else { false };
            if sendable {
                if self.is_symbol_iterator_computed_name(node) && self.is_arkts_collections_declaration(&[declaration]) { return; }
                self.add(node, Fault::SendableComputedPropName); return;
            }
        }
        if !self.is_valid_computed_property_name(node, false) { self.add(node, Fault::ComputedPropertyName); }
    }
    fn is_symbol_iterator_computed_name(&mut self, node: P<Node>) -> bool {
        if !ast::is_computed_property_name(node) { return false; }
        let expression = node.as_computed_property_name().expression;
        ast::is_property_access_expression(expression) && expression.name().unwrap().text() == "iterator" && self.true_symbol_at_location(expression).is_some_and(|s| s.name() == "iterator" && s.parent().is_some_and(|p| matches!(p.name(), "Symbol" | "SymbolConstructor")))
    }
    pub(super) fn handle_throw(&mut self, node: P<Node>) {
        let typ = self.checker.get_type_of_node(node.expression().unwrap());
        // This traversal intentionally retains instantiated types, matching OH's global check.
        fn visit(l: &mut ArkTsLinter<'_>, typ: P<Type>, seen: &mut FxHashSet<P<Type>>) -> bool {
            if !seen.insert(typ) { return false; }
            if typ.symbol().is_some_and(|s| s.name() == "Error") { return true; }
            l.checker.get_base_types(typ).iter().any(|&base| visit(l, base, seen))
        }
        if !typ.object_flags().intersects(ObjectFlags::ClassOrInterface) || !visit(self, typ, &mut FxHashSet::default()) { self.add(node, Fault::ThrowStatement); }
    }
}

fn is_assignment_operator(kind: Kind) -> bool { kind >= Kind::FirstAssignment && kind <= Kind::LastAssignment }
fn is_destructuring_assignment_lhs(node: P<Node>) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if ast::is_binary_expression(parent) && is_assignment_operator(parent.as_binary_expression().operator_token.kind()) && parent.as_binary_expression().left == current { return true; }
        if ast::is_for_statement(parent) && parent.as_for_statement().initializer == Some(current) { return true; }
        if matches!(parent.kind(), Kind::ForInStatement | Kind::ForOfStatement) && parent.as_for_in_or_of_statement().initializer == current { return true; }
        current = parent;
    }
    false
}
fn is_method_assignment(symbol: Option<P<Symbol>>) -> bool {
    symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Assignment) && (s.flags().intersects(SymbolFlags::Method) || s.value_declaration().or_else(|| s.declarations().first().copied()).is_some_and(|d| ast::is_binary_expression(d) && ast::is_function_like_declaration(d.as_binary_expression().right()))))
}
fn access_root(mut node: P<Node>) -> P<Node> {
    while let Some(parent) = node.parent().filter(|p| matches!(p.kind(), Kind::PropertyAccessExpression | Kind::QualifiedName)) { node = parent; } node
}
fn identifier_used_in_value_context(identifier: P<Node>, symbol: P<Symbol>) -> bool {
    let root = access_root(identifier); let Some(parent) = root.parent() else { return false; };
    if ast::is_type_node(parent) && !ast::is_type_of_expression(parent) || matches!(parent.kind(), Kind::ExpressionWithTypeArguments | Kind::ExportAssignment | Kind::ExportSpecifier | Kind::MetaProperty | Kind::ImportClause | Kind::ClassDeclaration | Kind::ClassExpression | Kind::InterfaceDeclaration | Kind::ModuleDeclaration | Kind::EnumDeclaration | Kind::NamespaceImport | Kind::ImportSpecifier | Kind::ImportEqualsDeclaration)
        || ast::is_qualified_name(root) && identifier != root.as_qualified_name().right || ast::is_property_access_expression(root) && root.name() != Some(identifier) || ast::is_new_expression(parent) && parent.expression() == Some(root) || ast::is_binary_expression(parent) && parent.as_binary_expression().operator_token.kind() == Kind::InstanceOfKeyword { return false; }
    if ast::is_element_access_expression(parent) && symbol.flags().intersects(SymbolFlags::Enum) {
        let expression = parent.expression().unwrap(); if expression == identifier || ast::is_property_access_expression(expression) && expression.name() == Some(identifier) { return false; }
    }
    true
}
fn function_returns_object_literal(node: P<Node>, promise: bool) -> bool {
    if !matches!(node.kind(), Kind::FunctionDeclaration | Kind::MethodDeclaration | Kind::ArrowFunction) || promise && !node.modifier_flags().intersects(ModifierFlags::Async) { return false; }
    let Some(body) = node.body().filter(|b| ast::is_block(*b)) else { return false; };
    fn visit(node: P<Node>, body: P<Node>) -> bool {
        if node != body && matches!(node.kind(), Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor | Kind::ArrowFunction) { return false; }
        if ast::is_return_statement(node) && node.expression().is_some_and(ast::is_object_literal_expression) { return true; }
        node.for_each_child(&mut |child| visit(child, body))
    }
    visit(body, body)
}
fn is_symbol_api(symbol: Option<P<Symbol>>) -> bool {
    symbol.is_some_and(|s| matches!(s.parent().unwrap_or(s).name(), "Symbol" | "SymbolConstructor"))
}
fn is_es_object_type(typ: P<Type>) -> bool { typ.alias().and_then(|a| a.symbol()).is_some_and(|s| s.name() == "ESObject") }
