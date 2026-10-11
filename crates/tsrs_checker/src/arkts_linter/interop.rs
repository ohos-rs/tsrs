use super::*;

impl ArkTsLinter<'_> {
    pub(super) fn visit_interop(&mut self, node: P<Node>) {
        match node.kind() {
            Kind::ImportDeclaration => self.handle_interop_import_declaration(node),
            Kind::InterfaceDeclaration | Kind::ClassDeclaration => self.handle_interop_heritage_clauses(node),
            Kind::NewExpression => self.handle_interop_new_expression(node),
            Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression => {
                if let Some(typ) = self.checker.get_contextual_type(node, ContextFlags::None) {
                    if self.is_literal_initializer_for_sendable_type(typ, node) { self.add(node, Fault::SendableObjectInitialization); }
                }
            }
            Kind::AsExpression => {
                let target = self.checker.get_type_of_node(node.type_node().unwrap()); let target = self.checker.get_non_nullable_type(target);
                let expression = self.checker.get_type_of_node(node.expression().unwrap()); let expression = self.checker.get_non_nullable_type(expression);
                self.handle_sendable_as_expression(node, target, expression);
            }
            Kind::ExportDeclaration => self.handle_interop_export_declaration(node),
            Kind::ExportAssignment => if self.interop_sdk_root().1 {
                let expression = node.expression().unwrap(); if self.is_sendable_class_or_interface_entity(expression) { self.add(expression, Fault::SendableTypeExported); }
            },
            _ => {}
        }
        node.for_each_child(&mut |child| { self.visit_interop(child); false });
    }
    fn handle_interop_import_declaration(&mut self, node: P<Node>) {
        let declaration = node.as_import_declaration();
        let Some(resolved) = self.interop_resolved_module(Some(declaration.module_specifier)) else { return; };
        let base_name = tspath::get_base_file_name(resolved.resolved_file_name);
        if base_name.starts_with("@kit.") && base_name.ends_with(tspath::EXTENSION_DTS) {
            if self.checker.compiler_options.ets_loader_path.is_empty() { return; }
            let Some(clause) = declaration.import_clause else { return; }; let clause = clause.as_import_clause();
            if clause.name().is_some() { return; }
            if let Some(bindings) = clause.named_bindings.filter(|b| ast::is_named_imports(*b)) { self.check_interop_kit_bindings(bindings.as_named_imports().elements.nodes(), &base_name); }
            return;
        }
        if !matches!(resolved.extension, tspath::EXTENSION_ETS | tspath::EXTENSION_DETS) || matches!(base_name.as_str(), "@arkts.lang.d.ets" | "@arkts.collections.d.ets") { return; }
        let Some(clause) = declaration.import_clause else { self.add(node, Fault::NoSideEffectImportETSToTS); return; }; let clause = clause.as_import_clause();
        let (sdk_root, in_sdk) = self.interop_sdk_root();
        let allow_sdk_sendable = in_sdk && tspath::normalize_path(resolved.resolved_file_name).starts_with(&sdk_root) && base_name.contains("sendable");
        if let Some(name) = clause.name() { if !allow_sdk_sendable { self.check_interop_imported_entity(name); } }
        if let Some(bindings) = clause.named_bindings {
            if ast::is_namespace_import(bindings) { self.add(bindings, Fault::NoNamespaceImportETSToTS); }
            else if ast::is_named_imports(bindings) { for element in bindings.as_named_imports().elements.nodes() { self.check_interop_imported_entity(element.name().unwrap()); } }
        }
    }
    fn interop_resolved_module(&self, specifier: Option<P<Node>>) -> Option<P<ResolvedModule>> {
        let specifier = specifier.filter(|s| ast::is_string_literal_like(*s))?;
        let mode = self.checker.program.get_mode_for_usage_location(self.file, specifier);
        self.checker.program.get_resolved_module(self.file, specifier.text(), mode).filter(|m| m.is_resolved())
    }
    fn interop_sdk_root(&self) -> (String, bool) {
        let loader = &self.checker.compiler_options.ets_loader_path;
        if loader.is_empty() { return (String::new(), false); }
        let root = tspath::normalize_path(&tspath::resolve_path(loader, &["../.."]));
        let in_sdk = tspath::normalize_path(self.file.file_name()).starts_with(&root); (root, in_sdk)
    }
    fn check_interop_imported_entity(&mut self, node: P<Node>) { if !self.is_sendable_class_or_interface_entity(node) { self.add(node, Fault::NoTSImportETS); } }
    fn is_sendable_class_or_interface_entity(&mut self, node: P<Node>) -> bool {
        let Some(declaration) = self.true_symbol_at_location(node).and_then(|s| s.declarations().first().copied()) else { return false; };
        if ast::is_class_declaration(declaration) { return self.has_decorator(declaration, "Sendable"); }
        if !ast::is_interface_declaration(declaration) { return false; }
        let typ = self.checker.get_type_of_node(declaration); self.is_sendable_class_or_interface(typ)
    }
    fn handle_interop_heritage_clauses(&mut self, node: P<Node>) {
        let clauses = if ast::is_class_declaration(node) { node.as_class_declaration().heritage_clauses() } else {
            if !node.name().and_then(|n| self.true_symbol_at_location(n)).is_some_and(|s| !s.declarations().is_empty()) { return; }
            node.as_interface_declaration().heritage_clauses
        };
        let Some(clauses) = clauses else { return; };
        for clause in clauses.nodes() {
            for &heritage in clause.as_heritage_clause().types().nodes() {
                let typ = self.checker.get_type_of_node(heritage); if self.is_sendable_class_or_interface(reduce_reference(typ)) { self.add(heritage, Fault::SendableTypeInheritance); }
            }
        }
    }
    fn handle_interop_new_expression(&mut self, node: P<Node>) {
        let typ = self.checker.get_type_of_node(node); if !self.is_sendable_class_or_interface(typ) { return; }
        for &argument in node.type_arguments() { if !self.is_sendable_type_node(argument, false, &mut FxHashSet::default()) { self.add(argument, Fault::SendableGenericTypes); } }
    }
    fn handle_interop_export_declaration(&mut self, node: P<Node>) {
        let declaration = node.as_export_declaration();
        if let Some(specifier) = declaration.module_specifier.filter(|s| ast::is_string_literal_like(*s)) {
            let Some(resolved) = self.interop_resolved_module(Some(specifier)) else { return; };
            let base_name = tspath::get_base_file_name(resolved.resolved_file_name);
            if base_name.starts_with("@kit.") && base_name.ends_with(tspath::EXTENSION_DTS) {
                if !self.checker.compiler_options.ets_loader_path.is_empty() {
                    if let Some(clause) = declaration.export_clause.filter(|c| ast::is_named_exports(*c)) { self.check_interop_kit_bindings(clause.as_named_exports().elements.nodes(), &base_name); }
                }
                return;
            }
            if matches!(resolved.extension, tspath::EXTENSION_ETS | tspath::EXTENSION_DETS) { self.add(specifier, Fault::NoTSReExportETS); }
            return;
        }
        if !self.interop_sdk_root().1 { return; }
        if let Some(clause) = declaration.export_clause.filter(|c| ast::is_named_exports(*c)) {
            for specifier in clause.as_named_exports().elements.nodes() { let name = specifier.name().unwrap(); if self.is_sendable_class_or_interface_entity(name) { self.add(name, Fault::SendableTypeExported); } }
        }
    }
    fn check_interop_kit_bindings(&mut self, elements: &[P<Node>], kit_file_name: &str) {
        if !self.load_interop_kit_info(kit_file_name) { return; }
        for &element in elements {
            let name = element.name().unwrap();
            let lookup = match element.kind() { Kind::ImportSpecifier => element.as_import_specifier().property_name().unwrap_or(name), Kind::ExportSpecifier => element.as_export_specifier().property_name.unwrap_or(name), _ => name };
            let Some(symbol) = self.kit_infos[kit_file_name].symbols.get(lookup.text()) else { continue; };
            if symbol.source.is_empty() || symbol.source.ends_with(tspath::EXTENSION_DTS) { continue; }
            let whitelist = matches!(symbol.source.as_str(), "@arkts.collections.d.ets" | "@arkts.lang.d.ets");
            let Some(declaration) = self.true_symbol_at_location(name).and_then(|s| s.declarations().first().copied()) else { continue; };
            if ast::is_module_declaration(declaration) { if !whitelist { self.add(element, Fault::NoTSImportETS); } }
            else if !self.is_sendable_class_or_interface_entity(name) { self.add(element, Fault::NoTSImportETS); }
        }
    }
    fn load_interop_kit_info(&mut self, file_name: &str) -> bool {
        if self.kit_infos.contains_key(file_name) { return true; }
        let mut roots = vec![tspath::resolve_path(&self.checker.compiler_options.ets_loader_path, &["../ets-loader/kit_configs"])];
        if let Some(paths) = &self.checker.compiler_options.oh_external_api_paths { for path in paths { roots.push(tspath::resolve_path(path, &["build-tools/ets-loader/kit_configs"])); } }
        let config_name = format!("{}.json", file_name.strip_suffix(tspath::EXTENSION_DTS).unwrap_or(file_name)); let mut info = None;
        for root in roots {
            if let Some(contents) = self.checker.program.read_file(&tspath::resolve_path(&root, &[&config_name])) {
                if let Ok(parsed) = serde_json::from_str::<KitInfo>(&contents) { info = Some(parsed); }
            }
        }
        if let Some(info) = info { self.kit_infos.insert(file_name.to_string(), info); true } else { false }
    }
}
