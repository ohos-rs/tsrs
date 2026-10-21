use crate::{self as ast, Kind, ModifierList, Node, NodeFlags};
use tsrs_core::{EtsOptions, P, ScriptKind, ets_contains};

pub fn is_struct_declaration(node: P<Node>) -> bool {
    ast::is_class_declaration(node) && node.flags().intersects(NodeFlags::Struct)
}

pub fn is_annotation_declaration(node: P<Node>) -> bool {
    ast::is_class_declaration(node) && node.flags().intersects(NodeFlags::Annotation)
}

pub fn is_annotation_property_declaration(node: P<Node>) -> bool {
    ast::is_property_declaration(node) && node.flags().intersects(NodeFlags::Annotation)
}

pub fn is_ets_component_expression(node: P<Node>) -> bool {
    ast::is_call_expression(node) && node.flags().intersects(NodeFlags::EtsComponent)
}

pub fn has_arkui_bare_decorator(modifiers: Option<P<ModifierList>>, names: &[&str]) -> bool {
    modifiers.is_some_and(|list| {
        list.nodes().iter().any(|node| {
            ast::is_decorator(*node)
                && node
                    .expression()
                    .is_some_and(|expression| ast::is_identifier(expression) && names.contains(&expression.text()))
        })
    })
}

pub fn has_ets_decorator(node: P<Node>, options: &EtsOptions) -> bool {
    node.modifier_nodes().iter().any(|modifier| {
        if !ast::is_decorator(*modifier) {
            return false;
        }
        let expression = modifier.expression().unwrap();
        if ast::is_call_expression(expression) {
            let callee = expression.expression().unwrap();
            return ast::is_identifier(callee) && ets_contains(options.extend.decorator.as_deref(), callee.text());
        }
        ast::is_identifier(expression)
            && (ets_contains(options.render.decorator.as_deref(), expression.text())
                || options.styles.decorator.as_deref() == Some(expression.text())
                || options.concurrent.decorator.as_deref() == Some(expression.text()))
    })
}

pub fn is_ets_function_decorator(decorator: P<Node>, options: &EtsOptions) -> bool {
    if !ast::is_decorator(decorator) {
        return false;
    }
    let mut expression = decorator.expression().unwrap();
    if ast::is_call_expression(expression) {
        expression = expression.expression().unwrap();
    }
    ast::is_identifier(expression)
        && (ets_contains(options.render.decorator.as_deref(), expression.text())
            || ets_contains(options.extend.decorator.as_deref(), expression.text())
            || options.styles.decorator.as_deref() == Some(expression.text()))
}

pub fn has_ets_styles_decorator(node: P<Node>, options: &EtsOptions) -> bool {
    options
        .styles
        .decorator
        .as_deref()
        .is_some_and(|name| has_arkui_bare_decorator(node.modifiers(), &[name]))
}

pub fn is_ets_builder(node: P<Node>, options: &EtsOptions) -> bool {
    match &options.render.decorator {
        None => has_arkui_bare_decorator(node.modifiers(), &["Builder", "LocalBuilder"]),
        Some(names) => names.iter().any(|name| has_arkui_bare_decorator(node.modifiers(), &[name])),
    }
}

pub fn is_sendable_function_or_type(node: P<Node>) -> bool {
    if !matches!(node.kind(), Kind::FunctionDeclaration | Kind::TypeAliasDeclaration)
        || ast::get_source_file_of_node(node).unwrap().script_kind() != ScriptKind::ETS
    {
        return false;
    }
    let mut decorators = node.modifier_nodes().iter().filter(|node| ast::is_decorator(**node));
    decorators.next().is_some_and(|node| {
        let expression = node.expression().unwrap();
        ast::is_identifier(expression) && expression.text() == "Sendable"
    }) && decorators.next().is_none()
}
