use super::*;

impl Checker {
    pub(super) fn arkts_ignored_directories(&self) -> Vec<String> {
        let paths = self.compiler_options.disable_strict_check_paths.as_deref().unwrap_or(&[]);
        let defaults = ["node_modules", "oh_modules", "build", ".preview"];
        let paths: Vec<&str> = if self.compiler_options.disable_strict_check_paths.is_none() { defaults.to_vec() } else { paths.iter().map(String::as_str).collect() };
        let mut result = Vec::new();
        for path in paths {
            if path.is_empty() || self.compiler_options.enable_strict_check_oh_module == Tristate::True && path == "oh_modules" || result.iter().any(|p| p == path) { continue; }
            result.push(path.to_string());
        }
        result
    }
}
impl ArkTsLinter<'_> {
    pub(super) fn true_symbol_at_location(&mut self, node: P<Node>) -> Option<P<Symbol>> {
        self.checker.get_symbol_at_location(node, false).map(|s| if s.flags().intersects(SymbolFlags::Alias) { self.checker.resolve_alias(s) } else { s })
    }
    pub(super) fn symbol_has_duplicate_name(&self, symbol: P<Symbol>, kind: Kind) -> bool {
        let is_type = |k| matches!(k, Kind::EnumDeclaration | Kind::ClassDeclaration | Kind::InterfaceDeclaration | Kind::TypeAliasDeclaration);
        symbol.declarations().iter().any(|d| d.kind() != Kind::Identifier && d.kind() != kind && !(is_type(d.kind()) && kind == Kind::ModuleDeclaration || is_type(kind) && d.kind() == Kind::ModuleDeclaration))
    }
    pub(super) fn is_library_type(&mut self, typ: P<Type>) -> bool {
        let typ = self.checker.get_non_nullable_type(typ);
        if typ.is_union() { return typ.types().iter().all(|&t| self.is_library_type(t)); }
        self.is_library_symbol(typ.alias().and_then(|a| a.symbol()).or_else(|| typ.symbol()))
    }
    pub(super) fn is_library_symbol(&mut self, symbol: Option<P<Symbol>>) -> bool {
        let Some(file) = symbol.and_then(|s| s.declarations().first().copied()).and_then(ast::get_source_file_of_node) else { return false; };
        if self.checker.program.is_source_file_default_library(file.path()) { return false; }
        let third_party = self.is_third_party_file(file);
        file.script_kind() != ScriptKind::ETS || third_party
    }
    fn is_third_party_file(&mut self, file: P<SourceFile>) -> bool {
        if tspath::get_base_file_name(file.file_name()) == "hvigorfile.ts" { return true; }
        let components = self.path_components.entry(file).or_insert_with(|| tspath::get_path_components(&tspath::normalize_path(file.file_name()), ""));
        self.ignored_directories.iter().any(|d| components.contains(d))
    }
    pub(super) fn source_file_path_contains_directory(&mut self, file: P<SourceFile>, directory: &str) -> bool {
        self.path_components.entry(file).or_insert_with(|| tspath::get_path_components(&tspath::normalize_path(file.file_name()), "")).iter().any(|p| p == directory)
    }
    fn is_static_source_type(&self, typ: P<Type>) -> bool {
        self.checker.compiler_options.mix_compile == Tristate::True && typ.symbol().and_then(|s| s.declarations().first().copied()).and_then(ast::get_source_file_of_node).is_some_and(|f| f.script_kind() == ScriptKind::ETS)
    }
    pub(super) fn is_dynamic_literal_initializer(&mut self, node: P<Node>) -> bool {
        if !matches!(node.kind(), Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression) { return false; }
        let mut current = Some(node);
        while let Some(literal) = current.filter(|n| matches!(n.kind(), Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression)) {
            if let Some(typ) = self.checker.get_contextual_type(literal, ContextFlags::None) {
                if !typ.object_flags().intersects(ObjectFlags::Anonymous) && self.is_library_type(typ) { return true; }
            }
            current = literal.parent();
            if current.is_some_and(ast::is_property_assignment) { current = current.unwrap().parent(); }
        }
        let Some(current) = current else { return false; };
        if ast::is_call_expression(current) {
            let expression = current.expression().unwrap(); let typ = self.checker.get_type_of_node(expression);
            if is_type_any(Some(typ)) || self.is_library_symbol(typ.symbol()) { return true; }
            if ast::is_property_access_expression(expression) { let symbol = self.true_symbol_at_location(expression.expression().unwrap()); return self.is_library_symbol(symbol); }
        }
        if ast::is_binary_expression(current) && ast::is_property_access_expression(current.as_binary_expression().left) {
            let typ = self.checker.get_type_of_node(current.as_binary_expression().left.expression().unwrap()); return self.is_library_type(typ);
        }
        false
    }
    pub(super) fn is_object_literal_assignable(&mut self, contextual_type: Option<P<Type>>, node: P<Node>) -> bool {
        let Some(typ) = contextual_type else { return false; }; let mut typ = self.checker.get_non_nullable_type(typ);
        if typ.is_union() { return typ.types().iter().any(|&t| self.is_object_literal_assignable(Some(t), node)); }
        if (is_type_any(Some(typ)) || self.is_library_type(typ)) && !self.is_static_source_type(typ) { return true; }
        if let Some(alias) = typ.alias() {
            if let Some(symbol) = alias.symbol() {
                if matches!(symbol.name(), "Partial" | "Required" | "Readonly") && self.checker.get_fully_qualified_name_exported(symbol) == symbol.name() {
                    if alias.type_arguments().len() != 1 { return false; } typ = alias.type_arguments()[0];
                }
            }
        }
        if self.is_std_record_type(typ) { return self.has_valid_record_keys(node); }
        let target = reduce_reference(typ);
        if !target.object_flags().intersects(ObjectFlags::ClassOrInterface) || !self.has_default_constructor(target) || self.has_readonly_fields(target) || self.is_abstract_class(target) || self.has_methods(target) { return false; }
        for &property in node.as_object_literal_expression().properties.nodes() {
            if !ast::is_property_assignment(property) { continue; }
            let name = self.property_name(property.name().unwrap());
            let symbol = self.checker.get_property_of_type(target, &name);
            if name.is_empty() { return false; }
            let Some(symbol) = symbol.filter(|s| !s.declarations().is_empty()) else { return false; };
            let property_type = self.checker.get_type_of_symbol_at_location(symbol, Some(symbol.declarations()[0])).unwrap();
            let initializer = ast::skip_parentheses(property.initializer().unwrap());
            if ast::is_object_literal_expression(initializer) {
                if !self.is_object_literal_assignable(Some(property_type), initializer) { return false; } continue;
            }
            let right_type = self.checker.get_type_of_node(initializer); let strict = self.needs_strict_structural_match(property_type, right_type);
            if self.needs_structural_identity(property_type, right_type, initializer, strict) || self.is_wrong_sendable_function_assignment(property_type, right_type) { return false; }
        }
        true
    }
    fn property_name(&mut self, name: P<Node>) -> String {
        if ast::is_identifier(name) || ast::is_private_identifier(name) || ast::is_string_or_numeric_literal_like(name) { return name.text().to_string(); }
        if ast::is_computed_property_name(name) {
            return match self.checker.evaluate(name.as_computed_property_name().expression, name).value {
                Some(LiteralValue::String(s)) => s.to_string(), Some(LiteralValue::Number(n)) => n.string(), _ => String::new(),
            };
        }
        String::new()
    }
    fn has_default_constructor(&self, typ: P<Type>) -> bool {
        let Some(members) = typ.symbol().and_then(|s| s.members()) else { return true; };
        let mut has_constructor = false;
        for symbol in members.values() {
            if !symbol.flags().intersects(SymbolFlags::Constructor) { continue; }
            has_constructor = true;
            if symbol.declarations().first().is_some_and(|d| ast::is_constructor_declaration(*d) && d.parameters().is_empty()) { return true; }
        }
        !has_constructor
    }
    fn has_readonly_fields(&self, typ: P<Type>) -> bool {
        typ.symbol().and_then(|s| s.members()).is_some_and(|members| members.values().into_iter().any(|s| s.declarations().first().is_some_and(|d| ast::is_property_declaration(*d) && ast::has_modifier(*d, ModifierFlags::Readonly))))
    }
    fn is_abstract_class(&self, typ: P<Type>) -> bool {
        typ.is_class() && typ.symbol().is_some_and(|s| s.declarations().first().is_some_and(|d| ast::has_modifier(*d, ModifierFlags::Abstract)))
    }
    fn has_methods(&mut self, typ: P<Type>) -> bool { self.checker.get_properties_of_type(typ).iter().any(|s| s.flags().intersects(SymbolFlags::Method)) }
    pub(super) fn is_std_record_type(&mut self, typ: P<Type>) -> bool {
        typ.alias().and_then(|a| a.symbol()).is_some_and(|s| s.name() == "Record" && self.checker.get_fully_qualified_name_exported(s) == "Record")
    }
    fn has_valid_record_keys(&mut self, node: P<Node>) -> bool {
        node.as_object_literal_expression().properties.nodes().iter().all(|p| p.name().is_some_and(|n| ast::is_string_literal(n) || ast::is_numeric_literal(n) || ast::is_computed_property_name(n) && self.is_valid_computed_property_name(n, true)))
    }
    pub(super) fn numeric_property_assignment_is_allowed(&mut self, node: P<Node>) -> bool {
        let parent = node.parent().unwrap(); let typ = self.checker.get_contextual_type(parent, ContextFlags::None);
        typ.is_some_and(|t| self.is_std_record_type(t) || self.is_library_type(t)) || self.is_dynamic_literal_initializer(parent)
    }
    pub(super) fn is_valid_computed_property_name(&mut self, node: P<Node>, record: bool) -> bool {
        let expression = node.as_computed_property_name().expression;
        if ast::is_string_literal_like(expression) { return true; }
        let symbol = self.true_symbol_at_location(expression);
        if symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::EnumMember)) {
            let typ = self.checker.get_type_of_node(expression);
            return typ.flags() & (TypeFlags::EnumLike | TypeFlags::StringLiteral) == TypeFlags::EnumLiteral | TypeFlags::StringLiteral;
        }
        !record && symbol.is_some_and(|s| s.name() == "iterator" && s.parent().is_some_and(|p| matches!(p.name(), "Symbol" | "SymbolConstructor")))
    }
    pub(super) fn needs_structural_identity(&mut self, left: P<Type>, right: P<Type>, expression: P<Node>, strict: bool) -> bool {
        let mut left = self.checker.get_non_nullable_type(left); let mut right = self.checker.get_non_nullable_type(right);
        if self.is_library_type(left) || self.is_dynamic_assigned_to_standard_type(left, expression) || !self.checker.get_signatures_of_type(left, SignatureKind::Call).is_empty() && !self.checker.get_signatures_of_type(right, SignatureKind::Call).is_empty() { return false; }
        if right.is_union() { return right.types().iter().any(|&t| self.needs_structural_identity(left, t, expression, strict)); }
        if left.is_union() { return left.types().iter().all(|&t| self.needs_structural_identity(t, right, expression, strict)); }
        if strict {
            if right.object_flags().contains(ObjectFlags::Reference | ObjectFlags::ArrayLiteral) { return false; }
            left = reduce_reference(left); right = reduce_reference(right);
        }
        left.object_flags().intersects(ObjectFlags::ClassOrInterface) && right.object_flags().intersects(ObjectFlags::ClassOrInterface) && !self.related_by_inheritance_or_identical(right, left, &mut FxHashSet::default())
    }
    fn related_by_inheritance_or_identical(&mut self, candidate: P<Type>, target: P<Type>, seen: &mut FxHashSet<(P<Type>, P<Type>)>) -> bool {
        let candidate = reduce_reference(candidate); let target = reduce_reference(target);
        if candidate == target || candidate.symbol().is_some() && candidate.symbol() == target.symbol() || self.is_named_global_type(target, "Object") || target.flags().intersects(TypeFlags::NonPrimitive) { return true; }
        if !seen.insert((candidate, target)) { return false; }
        let Some(symbol) = candidate.symbol() else { return false; }; let target_sendable = self.is_i_sendable_interface(target);
        for &declaration in symbol.declarations() {
            if target_sendable && ast::is_class_declaration(declaration) && self.has_decorator(declaration, "Sendable") { return true; }
            let clauses = match declaration.kind() { Kind::ClassDeclaration => declaration.as_class_declaration().heritage_clauses(), Kind::InterfaceDeclaration => declaration.as_interface_declaration().heritage_clauses, _ => None };
            let Some(clauses) = clauses else { continue; };
            for clause in clauses.nodes() {
                let clause = clause.as_heritage_clause(); let interfaces = !candidate.is_class() || clause.token != Kind::ExtendsKeyword;
                for &heritage in clause.types().nodes() {
                    let base = self.checker.get_type_of_node(heritage); let base = reduce_reference(base);
                    if base.is_class() != interfaces && self.related_by_inheritance_or_identical(base, target, seen) { return true; }
                }
            }
        }
        false
    }
    fn is_dynamic_assigned_to_standard_type(&mut self, left: P<Type>, expression: P<Node>) -> bool {
        if !self.is_standard_library_type(left) && !left.flags().intersects(TypeFlags::BooleanLike | TypeFlags::NumberLike | TypeFlags::StringLike | TypeFlags::BigIntLike | TypeFlags::ESSymbolLike) { return false; }
        let symbol = self.true_symbol_at_location(if ast::is_call_expression(expression) { expression.expression().unwrap() } else { expression });
        self.is_library_symbol(symbol)
    }
    fn is_standard_library_type(&self, typ: P<Type>) -> bool {
        typ.symbol().and_then(|s| s.declarations().first().copied()).and_then(ast::get_source_file_of_node).is_some_and(|f| self.checker.program.is_source_file_default_library(f.path()))
    }
    pub(super) fn needs_strict_structural_match(&mut self, left: P<Type>, right: P<Type>) -> bool {
        let mut strict_left = false;
        if left.is_union() {
            for &component in left.types() {
                let component = reduce_reference(component);
                if !component.object_flags().intersects(ObjectFlags::ClassOrInterface) { continue; }
                if !self.is_sendable_class_or_interface(component) { return false; } strict_left = true;
            }
        } else { strict_left = self.is_sendable_class_or_interface(left); }
        strict_left && self.contains_non_sendable_class_or_interface(right)
    }
    fn contains_non_sendable_class_or_interface(&mut self, typ: P<Type>) -> bool {
        if typ.is_union() { return typ.types().iter().any(|&t| self.contains_non_sendable_class_or_interface(t)); }
        let typ = reduce_reference(typ); typ.object_flags().intersects(ObjectFlags::ClassOrInterface) && !self.is_sendable_class_or_interface(typ)
    }
    pub(super) fn is_arkts_collections_declaration(&self, declarations: &[P<Node>]) -> bool {
        declarations.first().is_some_and(|d| matches!(d.kind(), Kind::ClassDeclaration | Kind::InterfaceDeclaration)
            && d.parent().filter(|p| ast::is_module_block(*p)).and_then(|p| p.parent()).and_then(|p| p.name()).is_some_and(|n| n.text() == "collections")
            && ast::get_source_file_of_node(*d).is_some_and(|f| tspath::get_base_file_name(f.file_name()).eq_ignore_ascii_case("@arkts.collections.d.ets")))
    }
    pub(super) fn is_indexable_array_type(&self, typ: P<Type>) -> bool {
        typ.symbol().is_some_and(|s| matches!(s.name(), "Array" | "ReadonlyArray" | "ConcatArray" | "ArrayLike" | "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array" | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array" | "BigUint64Array") || s.name() == "BitVector" && self.is_arkts_collections_declaration(s.declarations()))
    }
    pub(super) fn is_spread_array_type(&self, typ: P<Type>) -> bool {
        typ.symbol().is_some_and(|s| matches!(s.name(), "Array" | "ReadonlyArray" | "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array" | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array" | "BigUint64Array") || s.name() == "BitVector" && self.is_arkts_collections_declaration(s.declarations()))
    }
    pub(super) fn is_or_derived_from(&mut self, typ: P<Type>, predicate: impl Fn(&mut Self, P<Type>) -> bool) -> bool {
        // The recursive helper keeps the same linter borrow for each base type.
        fn walk<'a>(l: &mut ArkTsLinter<'a>, typ: P<Type>, predicate: &impl Fn(&mut ArkTsLinter<'a>, P<Type>) -> bool, seen: &mut FxHashSet<P<Type>>) -> bool {
            let typ = reduce_reference(typ); if !seen.insert(typ) { return false; }
            if predicate(l, typ) { return true; }
            l.checker.get_base_types(typ).iter().any(|&t| walk(l, t, predicate, seen))
        }
        walk(self, typ, &predicate, &mut FxHashSet::default())
    }
    pub(super) fn is_named_global_type(&mut self, typ: P<Type>, name: &str) -> bool {
        typ.symbol().is_some_and(|s| s.name() == name && self.checker.get_fully_qualified_name_exported(s) == name)
    }
    pub(super) fn type_or_constraint(&mut self, node: P<Node>) -> P<Type> {
        let typ = self.checker.get_type_of_node(node);
        if typ.flags().intersects(TypeFlags::TypeParameter) { self.checker.get_base_constraint_of_type(typ).unwrap_or(typ) } else { typ }
    }
}
