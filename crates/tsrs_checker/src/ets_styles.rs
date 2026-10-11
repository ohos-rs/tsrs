use crate::*;

impl Checker {
    pub(crate) fn check_ets_style_return_type(&mut self, node: P<Node>, result: Option<P<Type>>) -> bool {
        let Some(result) = result else {
            return false;
        };
        let options = &self.compiler_options.ets;
        let mut expected = None;
        let mut message = None;
        if ast::is_function_declaration(node) {
            let mut last = "";
            for &decorator in node.modifier_nodes() {
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
                if tsrs_core::ets_contains(&options.extend.decorator, name) || options.extend.decorator.is_none() && "Extend".contains(name)
                {
                    last = call.arguments()[0].text();
                }
            }
            if !last.is_empty() {
                message = Some(&diagnostics::Should_not_add_return_type_to_the_function_that_is_annotated_by_Extend);
                for component in options.extend.components.as_deref().unwrap_or_default() {
                    if component.name == last {
                        expected = Some(component.r#type.as_str());
                    }
                }
            }
        }
        if message.is_none()
            && (ast::is_function_declaration(node) || ast::is_method_declaration(node))
            && ast::has_arkui_bare_decorator(node.modifiers(), &[options.styles.decorator.as_deref().unwrap_or("Styles")])
        {
            message = Some(&diagnostics::Should_not_add_return_type_to_the_function_that_is_annotated_by_Styles);
            expected = options.styles.component.as_ref().map(|c| c.r#type.as_str());
        }
        let Some(message) = message else {
            return false;
        };
        if result.symbol().map(|s| s.name()) != expected {
            self.error(node.type_node(), message, &[]);
        }
        true
    }
}
