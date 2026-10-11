use crate::{ParseFlags, Parser, ParsingContext};
use tsrs_ast::{self as ast, Kind, ModifierList, Node, NodeFlags, NodeList};
use tsrs_core::{EtsComponentDeclaration, P, ScriptKind, ets_contains, new_text_range};

#[derive(Clone, Copy, Default)]
pub(crate) struct ArkUIState {
    pub struct_: bool,
    pub ui: bool,
    pub styles: bool,
    pub callback: bool,
    pub build: bool,
    pub style_declaration: Option<P<EtsComponentDeclaration>>,
    pub style_generic: bool,
    pub new_: bool,
    pub state_root: &'static str,
}

impl Parser {
    pub(crate) fn is_struct_declaration(&mut self) -> bool {
        self.script_kind == ScriptKind::ETS
            && self.token == Kind::Identifier
            && self.scanner.token_value() == "struct"
            && self.look_ahead(|p| {
                p.next_token();
                (p.is_identifier() || p.token == Kind::OpenBraceToken) && !p.has_preceding_line_break()
            })
    }

    pub(crate) fn enter_ets_function(&mut self, modifiers: Option<P<ModifierList>>, method_name: &str, method: bool) -> ArkUIState {
        let saved = std::mem::take(&mut self.arkui);
        self.arkui.struct_ = saved.struct_;
        if self.script_kind != ScriptKind::ETS {
            return saved;
        }
        self.arkui.build = self.has_ets_builder(modifiers, false)
            || method && method_name == "build" && ets_contains(&self.opts.ets.render.method, "build");
        self.arkui.ui = self.has_ets_builder(modifiers, true);
        if method {
            self.arkui.ui = self.arkui.struct_ && (method_name == "build" || method_name == "pageTransition" || self.arkui.ui);
        }
        let mut has_extend = false;
        if !method {
            let mut last = "";
            if let Some(modifiers) = modifiers {
                for &decorator in modifiers.nodes() {
                    if !ast::is_decorator(decorator) {
                        continue;
                    }
                    let expression = decorator.expression().unwrap();
                    if !ast::is_call_expression(expression) {
                        continue;
                    }
                    let callee = expression.expression().unwrap();
                    if ast::is_identifier(callee) && ets_contains(&self.opts.ets.extend.decorator, callee.text()) {
                        has_extend = true;
                        if let Some(&argument) = expression.arguments().first() {
                            if ast::is_identifier(argument) {
                                last = argument.text();
                            }
                        }
                    }
                }
            }
            for declaration in self.opts.ets.extend.components.as_deref().unwrap_or_default() {
                if !last.is_empty() && declaration.name == last {
                    self.arkui.style_declaration = Some(P::new(declaration.clone()));
                }
            }
        }
        if has_extend {
            self.arkui.ui = false;
        }
        if !has_extend && (!method || self.arkui.struct_) {
            if let Some(name) = self.opts.ets.styles.decorator.as_deref() {
                if ast::has_arkui_bare_decorator(modifiers, &[name]) {
                    if !method {
                        self.arkui.ui = false;
                    }
                    if let Some(declaration) = &self.opts.ets.styles.component {
                        self.arkui.style_declaration = Some(P::new(declaration.clone()));
                        self.arkui.style_generic = true;
                    }
                }
            }
        }
        self.arkui.styles = self.arkui.style_declaration.is_some();
        saved
    }

    fn has_ets_builder(&self, modifiers: Option<P<ModifierList>>, fallback: bool) -> bool {
        if self.opts.ets.render.decorator.is_none() && fallback {
            return ast::has_arkui_bare_decorator(modifiers, &["Builder", "LocalBuilder"]);
        }
        self.opts
            .ets
            .render
            .decorator
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|name| ast::has_arkui_bare_decorator(modifiers, &[name]))
    }

    pub(crate) fn finish_ets_virtual_node(&mut self, node: P<Node>, pos: i32) -> P<Node> {
        self.finish_node_with_end(node, pos, pos);
        node.set_virtual(true);
        node
    }

    pub(crate) fn ets_virtual_identifier(&mut self, name: &str, pos: i32) -> P<Node> {
        let node = self.factory.new_identifier(tsrs_core::alloc_str(name));
        self.finish_ets_virtual_node(node, pos)
    }

    pub(crate) fn add_ets_struct_constructor(&mut self, members: P<NodeList>, pos: i32) -> P<NodeList> {
        let mut properties = Vec::new();
        for &member in members.nodes() {
            if !ast::is_property_declaration(member) {
                continue;
            }
            let modifiers: Vec<_> = member
                .modifier_nodes()
                .iter()
                .filter(|m| !ast::is_decorator(**m))
                .map(|m| self.factory.deep_clone_reparse(Some(*m)).unwrap())
                .collect();
            let modifiers = if modifiers.is_empty() {
                None
            } else {
                Some(self.factory.new_modifier_list(modifiers))
            };
            let typ = self.factory.deep_clone_reparse(member.type_node());
            let name = self.factory.deep_clone_reparse(member.name()).unwrap();
            let question = self.factory.new_token(Kind::QuestionToken);
            let property = self
                .factory
                .new_property_signature_declaration(modifiers, name, Some(question), typ, None);
            properties.push(self.finish_ets_virtual_node(property, 0));
        }
        let mut parameters = Vec::new();
        if !properties.is_empty() {
            let typ = self
                .factory
                .new_type_literal_node(self.new_node_list(new_text_range(0, 0), &properties));
            let typ = self.finish_ets_virtual_node(typ, 0);
            let name = self.ets_virtual_identifier("value", 0);
            let question = self.factory.new_token(Kind::QuestionToken);
            let parameter = self
                .factory
                .new_parameter_declaration(None, None, name, Some(question), Some(typ), None);
            parameters.push(self.finish_ets_virtual_node(parameter, 0));
        }
        let name = self.ets_virtual_identifier("LocalStorage", 0);
        let storage = self.factory.new_type_reference_node(name, None);
        let storage = self.finish_ets_virtual_node(storage, 0);
        let name = self.ets_virtual_identifier("##storage", 0);
        let question = self.factory.new_token(Kind::QuestionToken);
        let parameter = self
            .factory
            .new_parameter_declaration(None, None, name, Some(question), Some(storage), None);
        parameters.push(self.finish_ets_virtual_node(parameter, 0));
        let body = self.factory.new_block(self.new_node_list(new_text_range(0, 0), &[]), false);
        let body = self.finish_ets_virtual_node(body, 0);
        let constructor = self.factory.new_constructor_declaration(
            None,
            None,
            Some(self.new_node_list(new_text_range(0, 0), &parameters)),
            None,
            None,
            Some(body),
        );
        let constructor = self.finish_ets_virtual_node(constructor, pos);
        let mut nodes = vec![constructor];
        nodes.extend_from_slice(members.nodes());
        self.new_node_list(members.loc.get(), &nodes)
    }

    pub(crate) fn ets_struct_heritage(&mut self, name: &str) -> P<NodeList> {
        let pos = self.node_pos();
        let name = self.ets_virtual_identifier(name, pos);
        let expression = self.factory.new_expression_with_type_arguments(name, None);
        let expression = self.finish_ets_virtual_node(expression, pos);
        let clause = self
            .factory
            .new_heritage_clause(Kind::ExtendsKeyword, self.new_node_list(new_text_range(pos, pos), &[expression]));
        let clause = self.finish_ets_virtual_node(clause, pos);
        self.new_node_list(new_text_range(pos, pos), &[clause])
    }

    pub(crate) fn add_arkui_readonly(&mut self, modifiers: Option<P<ModifierList>>) -> Option<P<ModifierList>> {
        if !self.arkui.struct_ {
            return modifiers;
        }
        let list = modifiers?;
        let (mut param, mut once, mut env) = (false, false, false);
        for &modifier in list.nodes() {
            if modifier.kind() == Kind::ReadonlyKeyword {
                return modifiers;
            }
            if !ast::is_decorator(modifier) {
                continue;
            }
            let expression = modifier.expression().unwrap();
            if ast::is_identifier(expression) {
                param |= expression.text() == "Param";
                once |= expression.text() == "Once";
            } else if ast::is_call_expression(expression) {
                let callee = expression.expression().unwrap();
                env |= ast::is_identifier(callee) && matches!(callee.text(), "Env" | "CustomEnv");
            }
        }
        if !env && (!param || once) {
            return modifiers;
        }
        let readonly = self.factory.new_modifier(Kind::ReadonlyKeyword);
        let readonly = self.finish_ets_virtual_node(readonly, self.node_pos());
        let mut nodes = list.nodes().to_vec();
        nodes.push(readonly);
        Some(self.new_modifier_list(list.loc(), &nodes))
    }

    pub(crate) fn is_arkui_iteration(&self, node: P<Node>) -> bool {
        if ast::is_identifier(node) {
            return ets_contains(&self.opts.ets.syntax_components.params_ui_callback, node.text());
        }
        if ast::is_property_access_expression(node) {
            let mut root = node.expression().unwrap();
            while ast::is_call_expression(root) || ast::is_property_access_expression(root) {
                root = root.expression().unwrap();
            }
            if ast::is_identifier(root) {
                for component in self.opts.ets.syntax_components.attr_ui_callback.as_deref().unwrap_or_default() {
                    if component.name == root.text() {
                        return ets_contains(&component.attributes, node.name().unwrap().text());
                    }
                }
            }
        }
        false
    }

    pub(crate) fn parse_arkui_argument_list(&mut self, expression: P<Node>) -> P<NodeList> {
        if !self.arkui.callback {
            return self.parse_argument_list();
        }
        let callback = self.arkui.callback;
        let skip_first = !ast::is_property_access_expression(expression) || expression.name().unwrap().text() != "each";
        let mut index = 0;
        self.parse_expected(Kind::OpenParenToken);
        let result = self
            .parse_delimited_list(ParsingContext::ArgumentExpressions, |p| {
                p.arkui.callback = !skip_first || index > 0;
                index += 1;
                p.parse_argument_expression()
            })
            .unwrap();
        self.parse_expected(Kind::CloseParenToken);
        self.arkui.callback = callback;
        result
    }

    pub(crate) fn parse_ets_component(&mut self) -> P<Node> {
        let pos = self.node_pos();
        let name = self.parse_binding_identifier();
        let arguments = self.parse_argument_list();
        let body = if self.token == Kind::OpenBraceToken {
            Some(self.parse_function_block(ParseFlags::None, None))
        } else {
            None
        };
        let result = self.factory.new_call_expression(name, None, None, arguments, body, NodeFlags::None);
        let result = self.finish_node(result, pos);
        result.set_flags(result.flags() | NodeFlags::EtsComponent);
        result
    }
}

impl Parser {
    pub(crate) fn parse_ets_function_type_parameters(&mut self) -> Option<P<NodeList>> {
        if !self.arkui.style_generic {
            return self.parse_type_parameters();
        }
        let pos = self.node_pos();
        let parameter = self.parse_type_parameter_worker(true);
        Some(self.new_node_list(new_text_range(pos, self.node_pos()), &[parameter]))
    }

    pub(crate) fn parse_ets_function_return_type(&mut self) -> Option<P<Node>> {
        let pos = self.node_pos();
        let result = self.parse_return_type(Kind::ColonToken, false);
        if result.is_none() {
            if let Some(declaration) = self.arkui.style_declaration {
                let name = self.ets_virtual_identifier(&declaration.r#type, pos);
                let node = self.factory.new_type_reference_node(name, None);
                return Some(self.finish_ets_virtual_node(node, pos));
            }
        }
        result
    }

    pub(crate) fn ets_attribute_arguments(
        &mut self,
        expression: P<Node>,
        pos: i32,
        original: Option<P<NodeList>>,
    ) -> (Option<P<NodeList>>, bool) {
        if (!self.arkui.build && !self.arkui.styles) || !ast::is_property_access_expression(expression) {
            return (original, false);
        }
        let attribute = expression.name().unwrap().text();
        let state_property = Some(attribute) == self.opts.ets.styles.property.as_deref();
        let (mut root_name, mut component, mut callback_root) = ("", false, false);
        let mut node = expression;
        loop {
            if ast::is_ets_component_expression(node) {
                root_name = node.expression().unwrap().text();
                component = true;
                break;
            }
            if ast::is_call_expression(node) && ast::is_identifier(node.expression().unwrap()) {
                root_name = "Common";
                for entry in self.opts.ets.syntax_components.attr_ui_callback.as_deref().unwrap_or_default() {
                    if entry.name == node.expression().unwrap().text() {
                        root_name = node.expression().unwrap().text();
                        callback_root = true;
                        break;
                    }
                }
                break;
            }
            if !ast::is_call_expression(node) && !ast::is_property_access_expression(node) {
                break;
            }
            node = node.expression().unwrap();
        }
        let mut virtual_type = String::new();
        if !root_name.is_empty() {
            self.arkui.state_root = if state_property { root_name } else { "" };
            let callback = callback_root
                && self
                    .opts
                    .ets
                    .syntax_components
                    .attr_ui_callback
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .find(|entry| entry.name == root_name)
                    .is_some_and(|entry| ets_contains(&entry.attributes, attribute));
            if callback {
                self.arkui.callback = true;
            } else if root_name == "WithEnv" && matches!(attribute, "env" | "customEnv") {
            } else if component {
                virtual_type = format!("{root_name}Attribute");
            }
        } else if !self.arkui.state_root.is_empty() {
            virtual_type = format!("{}Attribute", self.arkui.state_root);
        } else if self.arkui.styles && state_property {
            let mut node = expression;
            while ast::is_call_expression(node) || ast::is_property_access_expression(node) {
                if ast::is_property_access_expression(node) && node.expression().unwrap().is_virtual() {
                    self.arkui.state_root = tsrs_core::alloc_str(&node.expression().unwrap().text().replacen("Instance", "", 1));
                    virtual_type = format!("{}Attribute", self.arkui.state_root);
                    break;
                }
                node = node.expression().unwrap();
            }
        }
        if virtual_type.is_empty() {
            return (original, state_property);
        }
        let name = self.ets_virtual_identifier(&virtual_type, pos);
        let typ = self.factory.new_type_reference_node(name, None);
        let typ = self.finish_ets_virtual_node(typ, pos);
        (
            Some(self.new_node_list(new_text_range(self.node_pos(), self.node_pos()), &[typ])),
            state_property,
        )
    }
}
