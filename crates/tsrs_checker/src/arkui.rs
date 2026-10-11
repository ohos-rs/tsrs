use crate::*;

impl Checker {
    // OH checker/arkui.go: SDK system components and configured render contexts.
    pub(crate) fn is_system_ets_component(&mut self, node: P<Node>) -> bool {
        if !ast::is_identifier(node)
            || !tsrs_core::ets_contains(&self.compiler_options.ets.components, node.text())
            || self.compiler_options.ets_loader_path.is_empty()
        {
            return false;
        }
        let Some(symbol) = self.get_symbol_at_location(node, false) else { return false };
        let symbol = self.resolve_symbol(symbol);
        let declarations = symbol.declarations();
        if declarations.len() != 1 { return false; }
        let Some(file) = ast::get_source_file_of_node(declarations[0]) else { return false };
        let path = tsrs_core::tspath::get_normalized_absolute_path("declarations", &self.compiler_options.ets_loader_path);
        tsrs_core::tspath::normalize_path(file.file_name()).starts_with(&path)
    }

    pub(crate) fn is_in_build_or_page_transition_context(&self, node: P<Node>) -> bool {
        let options = &self.compiler_options.ets.render;
        if options.method.is_none() && options.decorator.is_none() { return false; }
        let mut container = ast::get_containing_function(node);
        while let Some(current) = container {
            if ast::is_method_declaration(current)
                && ast::get_containing_class(current).is_some_and(ast::is_struct_declaration)
                && current.name().is_some_and(|name| tsrs_core::ets_contains(&options.method, name.text()))
            {
                return true;
            }
            if (ast::is_method_declaration(current) || ast::is_function_declaration(current))
                && options.decorator.as_ref().is_some_and(|names| names.iter().any(|name| ast::has_arkui_bare_decorator(current.modifiers(), &[name])))
            {
                return true;
            }
            container = ast::get_containing_function(current);
        }
        false
    }

    pub(crate) fn check_struct_declaration(&mut self, node: P<Node>) {
        if node.name().is_none() && !ast::has_syntactic_modifier(node, ModifierFlags::Default) {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::A_struct_declaration_without_the_default_modifier_must_have_a_name,
                &[],
            );
        }
        self.check_class_like_declaration(node);
        if let Some(name) = node.name() {
            if ast::is_identifier(name) && tsrs_core::ets_contains(&self.compiler_options.ets.components, name.text()) {
                self.error(
                    Some(name),
                    &diagnostics::The_struct_name_cannot_contain_reserved_tag_name_Colon_0,
                    &[&name.text()],
                );
            }
        }
        self.check_source_elements(node.members());
        self.register_for_unused_identifiers_check(node);
    }

    pub(crate) fn get_arkui_style_property(&mut self, node: P<Node>, left: P<Node>, right: P<Node>) -> Option<P<Symbol>> {
        let mut root = ets_component_root(left);
        if root.is_none() {
            let mut ancestor = Some(node);
            while let Some(current) = ancestor {
                if ast::is_object_literal_expression(current) {
                    if current.parent().is_some_and(ast::is_property_assignment) {
                        let mut call = current.parent();
                        while let Some(current) = call {
                            if ast::is_call_expression(current) {
                                root = ets_component_root(current);
                                if root.is_some() {
                                    break;
                                }
                            }
                            call = current.parent();
                        }
                    }
                    break;
                }
                ancestor = current.parent();
            }
        }
        let root = root?;
        let mut symbol = None;
        let options = &self.compiler_options.ets;
        if let Some(local) = ast::get_source_file_of_node(node)
            .unwrap()
            .as_node()
            .locals()
            .and_then(|locals| locals.lookup(right.text()))
        {
            if let Some(declaration) = local.value_declaration() {
                for &decorator in declaration.modifier_nodes() {
                    if !ast::is_decorator(decorator) || !ast::is_call_expression(decorator.expression().unwrap()) {
                        continue;
                    }
                    let call = decorator.expression().unwrap();
                    if !ast::is_identifier(call.expression().unwrap())
                        || call.arguments().is_empty()
                        || !ast::is_identifier(call.arguments()[0])
                    {
                        continue;
                    }
                    let name = call.expression().unwrap().text();
                    if (tsrs_core::ets_contains(&options.extend.decorator, name)
                        || options.extend.decorator.is_none() && "Extend".contains(name))
                        && ast::is_identifier(root.expression().unwrap())
                        && call.arguments()[0].text() == root.expression().unwrap().text()
                    {
                        symbol = Some(local);
                    }
                }
                if ast::has_ets_styles_decorator(declaration, options) {
                    symbol = Some(local);
                }
            }
        }
        let mut parent = node.parent();
        while let Some(current) = parent {
            if ast::is_struct_declaration(current) {
                if let Some(member) = current
                    .symbol()
                    .and_then(|s| s.members())
                    .and_then(|members| members.lookup(right.text()))
                {
                    if member
                        .value_declaration()
                        .is_some_and(|d| ast::has_ets_styles_decorator(d, options))
                    {
                        symbol = Some(member);
                    }
                }
                break;
            }
            parent = current.parent();
        }
        symbol
    }
}

fn ets_component_root(mut root: P<Node>) -> Option<P<Node>> {
    while ast::is_call_expression(root) || ast::is_property_access_expression(root) {
        if ast::is_ets_component_expression(root) {
            return Some(root);
        }
        root = root.expression().unwrap();
    }
    None
}
