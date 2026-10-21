use crate::*;
use tsrs_core::{ScriptKind, tspath};

pub(crate) fn is_annotation_symbol(symbol: Option<P<Symbol>>) -> bool {
    symbol.is_some_and(|s| s.flags().intersects(SymbolFlags::Annotation))
}

#[derive(Default)]
pub struct AnnotationLinks {
    source_retention: Cell<Tristate>,
    checked: Cell<bool>,
}

impl crate::links::LinkCopy for AnnotationLinks {
    fn copy_link_from(&self, frozen: &Self) {
        self.source_retention.set(frozen.source_retention.get());
        self.checked.set(frozen.checked.get());
    }
}

fn is_retention_annotation_declaration(node: P<Node>) -> bool {
    node.name().unwrap().text() == "Retention"
        && tspath::get_base_file_name(ast::get_source_file_of_node(node).unwrap().file_name()).eq_ignore_ascii_case("@arkts.lang.d.ets")
}

impl Checker {
    fn has_source_retention_policy(&mut self, node: P<Node>) -> bool {
        if is_retention_annotation_declaration(node) {
            return false;
        }
        for &modifier in node.modifier_nodes() {
            if !ast::is_decorator(modifier) {
                continue;
            }
            let Some(declaration) = self.annotation_for_decorator(modifier) else {
                continue;
            };
            if !is_retention_annotation_declaration(declaration) {
                continue;
            }
            let expression = modifier.expression().unwrap();
            if !ast::is_call_expression(expression)
                || expression.arguments().len() != 1
                || !ast::is_object_literal_expression(expression.arguments()[0])
            {
                continue;
            }
            for &prop in expression.arguments()[0].properties() {
                if !ast::is_property_assignment(prop) || tsrs_scanner::get_text_of_node(prop.name().unwrap()) != "policy" {
                    continue;
                }
                let value = prop.initializer().unwrap();
                if ast::is_string_literal(value) {
                    return value.text() == "source";
                }
                if !ast::is_property_access_expression(value) {
                    continue;
                }
                if let Some(symbol) = self.resolve_entity_name(value, SymbolFlags::Value, true, false, None) {
                    if symbol.flags().intersects(SymbolFlags::EnumMember)
                        && ast::is_enum_const(symbol.value_declaration().unwrap().parent().unwrap())
                        && self.get_constant_value(symbol.value_declaration().unwrap()) == Some(LiteralValue::String("source"))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(crate) fn is_source_retention_annotation_declaration(&mut self, node: P<Node>) -> bool {
        let links = self.annotation_links.get(node);
        if links.source_retention.get() == Tristate::Unknown {
            links.source_retention.set(Tristate::False);
            let name = node.name().unwrap().text();
            let sdk_source_annotation = matches!(name, "Available" | "SuppressWarnings")
                && tspath::normalize_path(ast::get_source_file_of_node(node).unwrap().file_name()).ends_with("@ohos.annotation.d.ets");
            if sdk_source_annotation || self.has_source_retention_policy(node) {
                links.source_retention.set(Tristate::True);
            }
        }
        links.source_retention.get() == Tristate::True
    }

    pub(crate) fn check_annotation_declaration(&mut self, node: P<Node>) {
        self.check_annotation_js_har(node);
        if !ast::is_source_file(node.parent().unwrap()) {
            self.grammar_error_on_node(node, &diagnostics::Annotation_must_be_defined_at_top_level_only, &[]);
        }
        self.check_grammar_modifiers(node);
        if ast::has_syntactic_modifier(node, ModifierFlags::Default) {
            self.error(Some(node), &diagnostics::Annotation_cannot_be_exported_as_default, &[]);
        }
        for &modifier in node.modifier_nodes() {
            if ast::is_decorator(modifier) {
                if let Some(declaration) = self.annotation_for_decorator(modifier) {
                    self.check_annotation_use(modifier, declaration);
                } else {
                    self.grammar_error_on_node(modifier, &diagnostics::Decorators_are_not_valid_here, &[]);
                }
            }
        }
        self.check_collisions_for_declaration_name(node, node.name());
        self.check_exports_on_merged_declarations(node);
        let symbol = self.get_symbol_of_declaration(node).unwrap();
        let t = self.get_declared_type_of_symbol(symbol);
        let static_type = self.get_type_of_symbol(symbol);
        self.check_object_type_for_duplicate_declarations(node, true);
        self.check_index_constraints(t, symbol, false);
        self.check_index_constraints(static_type, symbol, true);
        self.check_property_initialization(node);
        self.check_source_elements(node.members());
    }

    pub(crate) fn annotation_for_decorator(&mut self, node: P<Node>) -> Option<P<Node>> {
        if ast::get_source_file_of_node(node).unwrap().script_kind() != ScriptKind::ETS {
            return None;
        }
        let mut expression = node.expression().unwrap();
        if ast::is_call_expression(expression) {
            expression = expression.expression().unwrap();
        }
        if !ast::is_identifier(expression) && !ast::is_property_access_expression(expression) {
            return None;
        }
        let symbol = self.resolve_entity_name(expression, SymbolFlags::Value, true, false, None);
        let symbol = symbol.filter(|s| is_annotation_symbol(Some(*s)))?;
        symbol.value_declaration()
    }

    pub(crate) fn check_annotation_use(&mut self, node: P<Node>, declaration: P<Node>) {
        let links = self.annotation_links.get(node);
        if links.checked.replace(true) {
            return;
        }
        if !self.is_source_retention_annotation_declaration(declaration) { self.check_annotation_js_har(node); }
        let expression = node.expression().unwrap();
        if node.parent() != Some(declaration) {
            if !ast::is_call_expression(expression) {
                self.check_expression(expression);
            } else if expression.arguments().is_empty() {
                self.check_expression(expression.expression().unwrap());
            }
        }
        let mut argument = None;
        if ast::is_call_expression(expression) && !expression.arguments().is_empty() {
            if expression.arguments().len() != 1 || !ast::is_object_literal_expression(expression.arguments()[0]) {
                self.error(
                    Some(node),
                    &diagnostics::Only_an_object_literal_have_to_be_provided_as_annotation_parameters_list_got_Colon_0,
                    &[&tsrs_scanner::get_text_of_node(expression.arguments()[0])],
                );
                return;
            }
            argument = Some(expression.arguments()[0]);
        }
        if argument.is_none_or(|arg| arg.properties().is_empty()) {
            for &member in declaration.members() {
                if member.initializer().is_none() {
                    self.error(
                        Some(node),
                        &diagnostics::When_annotation_0_is_applied_all_fields_without_default_values_must_be_provided,
                        &[&tsrs_scanner::get_text_of_node(expression)],
                    );
                    return;
                }
            }
        } else {
            for &prop in argument.unwrap().properties() {
                if !ast::is_property_assignment(prop) {
                    self.error(
                        Some(node),
                        &diagnostics::Only_an_object_literal_have_to_be_provided_as_annotation_parameters_list_got_Colon_0,
                        &[&tsrs_scanner::get_text_of_node(argument.unwrap())],
                    );
                    return;
                }
                if self
                    .evaluate_annotation_constant(prop.initializer().unwrap(), &mut FxHashSet::default())
                    .is_none()
                {
                    self.error(Some(node), &diagnostics::All_members_of_object_literal_which_is_provided_as_annotation_parameters_list_have_to_be_constant_expressions_got_Colon_0, &[&tsrs_scanner::get_text_of_node(prop.initializer().unwrap())]);
                    return;
                }
            }
            let t = self.check_expression(expression);
            if self.is_error_type(t) {
                return;
            }
        }
        let parent = node.parent().unwrap();
        if self.is_source_retention_annotation_declaration(declaration) {
            if !is_source_retention_annotation_target(parent) {
                self.error(
                    Some(node),
                    &diagnostics::X_0_annotation_are_not_valid_here_got_Colon_1,
                    &[
                        &tsrs_scanner::get_text_of_node(declaration.name().unwrap()),
                        &tsrs_scanner::get_text_of_node(parent),
                    ],
                );
                return;
            }
        } else if is_retention_annotation_declaration(declaration) {
            if !ast::is_annotation_declaration(parent) {
                self.error(
                    Some(node),
                    &diagnostics::X_0_should_only_be_applied_to_annotation_declarations,
                    &[&"@Retention"],
                );
                return;
            }
        } else {
            let class = |n| ast::is_class_declaration(n) && !ast::is_annotation_declaration(n) && !ast::is_struct_declaration(n);
            if !(class(parent) || ast::is_method_declaration(parent) && class(parent.parent().unwrap())) {
                let message = match parent.kind() {
                    Kind::Constructor => &diagnostics::Annotation_cannot_be_applied_for_constructor_got_Colon_0,
                    Kind::GetAccessor | Kind::SetAccessor => &diagnostics::Annotation_cannot_be_applied_for_getter_or_setter_got_Colon_0,
                    _ if ast::is_annotation_declaration(parent) => &diagnostics::Annotation_cannot_be_applied_for_annotation_declaration,
                    _ => &diagnostics::Annotation_have_to_be_applied_for_classes_or_methods_in_classes_only_got_Colon_0,
                };
                self.error(Some(node), message, &[&tsrs_scanner::get_text_of_node(parent)]);
                return;
            }
            if ast::has_syntactic_modifier(parent, ModifierFlags::Abstract)
                || ast::is_method_declaration(parent) && ast::has_syntactic_modifier(parent.parent().unwrap(), ModifierFlags::Abstract)
            {
                self.error(Some(node), &diagnostics::Annotation_have_to_be_applied_only_for_non_abstract_class_declarations_and_method_declarations_in_non_abstract_classes_got_Colon_0, &[&tsrs_scanner::get_text_of_node(parent)]);
                return;
            }
        }
        for &previous in parent.modifier_nodes() {
            if previous == node {
                break;
            }
            if ast::is_decorator(previous) && self.annotation_for_decorator(previous) == Some(declaration) {
                self.error(
                    Some(node),
                    &diagnostics::Repeatable_annotation_are_not_supported_got_Colon_0,
                    &[&tsrs_scanner::get_text_of_node(node)],
                );
                return;
            }
        }
        if self.is_source_retention_annotation_declaration(declaration) { self.check_source_retention_annotation_content(node, declaration); }
    }

    fn check_annotation_js_har(&mut self, node: P<Node>) {
        if self.compiler_options.is_compile_js_har == Tristate::True && !self.compiler_options.module_root_path.is_empty()
            && tspath::normalize_path(ast::get_source_file_of_node(node).unwrap().file_name()).starts_with(&tspath::normalize_path(&self.compiler_options.module_root_path)) {
            self.grammar_error_on_node(node, &diagnostics::Annotations_are_not_supported_in_Hars_compiled_to_JavaScript_files, &[]);
        }
    }

    pub(crate) fn check_annotation_property_declaration(&mut self, node: P<Node>) {
        if node.type_node().is_none() && node.initializer().is_none() {
            self.error(
                Some(node),
                &diagnostics::An_annotation_property_must_have_a_type_or_Slashand_an_initializer,
                &[],
            );
        }
        self.check_variable_like_declaration(node);
        let t = self.get_type_of_node(node);
        if !self.is_allowed_annotation_property_type(t) {
            let type_name = self.type_to_string_exported(t);
            self.error(Some(node), &diagnostics::A_type_of_annotation_property_have_to_be_number_boolean_string_const_enumeration_types_or_array_of_above_types_got_Colon_0, &[&type_name]);
        }
        if let Some(init) = node.initializer() {
            if self.evaluate_annotation_constant(init, &mut FxHashSet::default()).is_none() {
                self.error(
                    Some(node),
                    &diagnostics::Default_value_of_annotation_property_can_be_a_constant_expression_got_Colon_0,
                    &[&tsrs_scanner::get_text_of_node(init)],
                );
            }
        }
    }

    fn is_allowed_annotation_property_type(&mut self, t: P<Type>) -> bool {
        if [self.number_type, self.string_type, self.boolean_type].contains(&t) {
            return true;
        }
        if let Some(element) = self.get_element_type_of_array_type(t) {
            return self.is_allowed_annotation_property_type(element);
        }
        let Some(symbol) = t.symbol() else {
            return false;
        };
        if !symbol.flags().intersects(SymbolFlags::ConstEnum)
            && !symbol.parent().is_some_and(|p| p.flags().intersects(SymbolFlags::ConstEnum))
        {
            return false;
        }
        for &declaration in symbol.declarations() {
            if ast::is_enum_declaration(declaration) {
                for pair in declaration.members().windows(2).skip(1) {
                    if self.get_constant_value(pair[0]).map(|v| std::mem::discriminant(&v))
                        != self.get_constant_value(pair[1]).map(|v| std::mem::discriminant(&v))
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}

fn is_source_retention_annotation_target(node: P<Node>) -> bool {
    match node.kind() {
        Kind::ClassDeclaration
        | Kind::FunctionDeclaration
        | Kind::VariableStatement
        | Kind::TypeAliasDeclaration
        | Kind::InterfaceDeclaration
        | Kind::EnumDeclaration
        | Kind::ModuleDeclaration => true,
        Kind::GetAccessor | Kind::SetAccessor | Kind::MethodDeclaration | Kind::PropertyDeclaration => {
            !ast::is_annotation_property_declaration(node)
                && (ast::is_class_declaration(node.parent().unwrap()) && !ast::is_annotation_declaration(node.parent().unwrap())
                    || ast::is_interface_declaration(node.parent().unwrap()))
        }
        Kind::PropertySignature | Kind::MethodSignature => ast::is_interface_declaration(node.parent().unwrap()),
        _ => false,
    }
}

#[derive(Clone, PartialEq)]
pub enum AnnotationConstant {
    Scalar(LiteralValue),
    Array(Vec<AnnotationConstant>),
}
impl AnnotationConstant {
    fn same_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Scalar(a), Self::Scalar(b)) => std::mem::discriminant(a) == std::mem::discriminant(b),
            (Self::Array(_), Self::Array(_)) => true,
            _ => false,
        }
    }
    fn string(&self) -> String {
        match self {
            Self::Scalar(v) => evaluator::any_to_string(*v),
            Self::Array(values) => values.iter().map(Self::string).collect::<Vec<_>>().join(","),
        }
    }
}

impl Checker {
    fn evaluate_annotation_constant(&mut self, node: P<Node>, active: &mut FxHashSet<P<Node>>) -> Option<AnnotationConstant> {
        if !active.insert(node) {
            return None;
        }
        let value = self.evaluate_annotation_constant_worker(node, active);
        active.remove(&node);
        value
    }
    fn evaluate_annotation_constant_worker(&mut self, node: P<Node>, active: &mut FxHashSet<P<Node>>) -> Option<AnnotationConstant> {
        use AnnotationConstant::{Array, Scalar};
        use LiteralValue::{Boolean, Number, String};
        match node.kind() {
            Kind::TrueKeyword => Some(Scalar(Boolean(true))),
            Kind::FalseKeyword => Some(Scalar(Boolean(false))),
            Kind::NumericLiteral => Some(Scalar(Number(jsnum::from_string(node.text())))),
            Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral => Some(Scalar(String(node.text()))),
            Kind::ParenthesizedExpression => self.evaluate_annotation_constant(node.expression().unwrap(), active),
            Kind::Identifier => {
                if matches!(node.text(), "Infinity" | "NaN") {
                    return Some(Scalar(Number(jsnum::from_string(node.text()))));
                }
                let resolved = self.get_resolved_symbol(node);
                let symbol = self.get_export_symbol_of_value_symbol_if_exported(Some(resolved))?;
                let declaration = symbol.value_declaration()?;
                if ast::is_variable_declaration(declaration) && self.is_var_const_like(declaration) {
                    self.evaluate_annotation_constant(declaration.initializer()?, active)
                } else {
                    None
                }
            }
            Kind::PropertyAccessExpression => self.get_constant_value(node).map(Scalar),
            Kind::ArrayLiteralExpression => {
                let mut values: Vec<AnnotationConstant> = Vec::new();
                for &element in node.elements() {
                    let value = self.evaluate_annotation_constant(element, active)?;
                    if values.last().is_some_and(|last| !last.same_kind(&value)) {
                        return None;
                    }
                    values.push(value);
                }
                Some(Array(values))
            }
            Kind::PrefixUnaryExpression => {
                let expr = node.as_prefix_unary_expression();
                let value = self.evaluate_annotation_constant(expr.operand(), active)?;
                if expr.operator() == Kind::ExclamationToken {
                    if let Scalar(value) = value {
                        return Some(Scalar(Boolean(!evaluator::is_truthy(value))));
                    }
                }
                if let Scalar(Number(n)) = value {
                    return match expr.operator() {
                        Kind::PlusToken => Some(Scalar(Number(n))),
                        Kind::MinusToken => Some(Scalar(Number(-n))),
                        Kind::TildeToken => Some(Scalar(Number(n.bitwise_not()))),
                        _ => None,
                    };
                }
                None
            }
            Kind::BinaryExpression => {
                let expr = node.as_binary_expression();
                let left = self.evaluate_annotation_constant(expr.left(), active)?;
                let right = self.evaluate_annotation_constant(expr.right(), active)?;
                annotation_binary(expr.operator_token().kind(), left, right)
            }
            _ => None,
        }
    }
}

#[expect(
    clippy::float_cmp,
    reason = "annotation constant values are compared exactly, as in the Go source"
)]
fn annotation_binary(operator: Kind, left: AnnotationConstant, right: AnnotationConstant) -> Option<AnnotationConstant> {
    use AnnotationConstant::{Array, Scalar};
    use LiteralValue::{Boolean, Number, String};
    if let (Scalar(l), Scalar(_)) = (&left, &right) {
        match operator {
            Kind::AmpersandAmpersandToken => return Some(if evaluator::is_truthy(*l) { right } else { left }),
            Kind::BarBarToken => return Some(if evaluator::is_truthy(*l) { left } else { right }),
            _ => {}
        }
    }
    if left.same_kind(&right) {
        if let (Array(_), Array(_)) = (&left, &right) {
            match operator {
                Kind::AmpersandAmpersandToken => return Some(right),
                Kind::BarBarToken => return Some(left),
                Kind::LessThanToken | Kind::LessThanEqualsToken | Kind::GreaterThanToken | Kind::GreaterThanEqualsToken => {
                    return annotation_binary(
                        operator,
                        Scalar(String(alloc_str(&left.string()))),
                        Scalar(String(alloc_str(&right.string()))),
                    );
                }
                _ => {}
            }
        }
        let equal = match (&left, &right) {
            (Scalar(Number(l)), Scalar(Number(r))) => l.0 == r.0,
            (Scalar(l), Scalar(r)) => l == r,
            _ => false,
        };
        match operator {
            Kind::EqualsEqualsToken | Kind::EqualsEqualsEqualsToken => return Some(Scalar(Boolean(equal))),
            Kind::ExclamationEqualsToken | Kind::ExclamationEqualsEqualsToken => return Some(Scalar(Boolean(!equal))),
            _ => {}
        }
    }
    Some(Scalar(match (left, right) {
        (Scalar(Number(l)), Scalar(Number(r))) => match operator {
            Kind::PlusToken => Number(l + r),
            Kind::MinusToken => Number(l - r),
            Kind::AsteriskToken => Number(l * r),
            Kind::SlashToken => Number(l / r),
            Kind::PercentToken => Number(l.remainder(r)),
            Kind::AsteriskAsteriskToken => Number(l.exponentiate(r)),
            Kind::BarToken => Number(l.bitwise_or(r)),
            Kind::AmpersandToken => Number(l.bitwise_and(r)),
            Kind::CaretToken => Number(l.bitwise_xor(r)),
            Kind::LessThanLessThanToken => Number(l.left_shift(r)),
            Kind::GreaterThanGreaterThanToken => Number(l.signed_right_shift(r)),
            Kind::GreaterThanGreaterThanGreaterThanToken => Number(l.unsigned_right_shift(r)),
            Kind::LessThanToken => Boolean(l.0 < r.0),
            Kind::LessThanEqualsToken => Boolean(l.0 <= r.0),
            Kind::GreaterThanToken => Boolean(l.0 > r.0),
            Kind::GreaterThanEqualsToken => Boolean(l.0 >= r.0),
            _ => return None,
        },
        (Scalar(String(l)), Scalar(String(r))) => {
            let order = l.encode_utf16().cmp(r.encode_utf16());
            match operator {
                Kind::PlusToken => String(alloc_str(&format!("{l}{r}"))),
                Kind::LessThanToken => Boolean(order.is_lt()),
                Kind::LessThanEqualsToken => Boolean(order.is_le()),
                Kind::GreaterThanToken => Boolean(order.is_gt()),
                Kind::GreaterThanEqualsToken => Boolean(order.is_ge()),
                _ => return None,
            }
        }
        (Scalar(Boolean(l)), Scalar(Boolean(r))) => match operator {
            Kind::LessThanToken => Boolean(!l && r),
            Kind::LessThanEqualsToken => Boolean(!l || r),
            Kind::GreaterThanToken => Boolean(l && !r),
            Kind::GreaterThanEqualsToken => Boolean(l || !r),
            _ => return None,
        },
        _ => return None,
    }))
}

/// Checker facts for OH `ohApi.ts::transformAnnotation`; code generation stays
/// with the consumer. Constants distinguish invalid/absent from empty arrays.
pub struct AnnotationInfo {
    pub declaration: P<Node>,
    pub source_retention: bool,
    pub properties: Vec<AnnotationPropertyInfo>,
}
pub struct AnnotationPropertyInfo {
    pub declaration: P<Node>,
    pub typ: P<Type>,
    pub initializer: Option<AnnotationConstant>,
    pub argument: Option<AnnotationConstant>,
    pub array_depth: usize,
    pub element_type: P<Type>,
    pub enum_declaration: Option<P<Node>>,
    pub enum_first_value: Option<AnnotationConstant>,
}

impl Checker {
    // OH checker/annotation_info.go::GetAnnotationInfo.
    pub fn get_annotation_info(&mut self, node: P<Node>) -> Option<AnnotationInfo> {
        let mut declaration = node;
        let mut arguments = FxHashMap::default();
        if ast::is_decorator(node) {
            declaration = self.annotation_for_decorator(node)?;
            self.check_annotation_use(node, declaration);
            let expression = node.expression().unwrap();
            if ast::is_call_expression(expression) && !expression.arguments().is_empty() {
                if expression.arguments().len() != 1 || !ast::is_object_literal_expression(expression.arguments()[0]) {
                    return None;
                }
                for &property in expression.arguments()[0].properties() {
                    if !ast::is_property_assignment(property) { return None; }
                    let value = self.evaluate_annotation_constant(property.initializer()?, &mut FxHashSet::default())?;
                    let name = ast::try_get_text_of_property_name(property.name()?)?;
                    arguments.insert(name, value);
                }
            }
        }
        if !ast::is_annotation_declaration(declaration) { return None; }
        let mut info = AnnotationInfo {
            declaration,
            source_retention: self.is_source_retention_annotation_declaration(declaration),
            properties: Vec::new(),
        };
        for &member in declaration.members() {
            if !ast::is_annotation_property_declaration(member) { return None; }
            let typ = self.get_type_of_node(member);
            if !self.is_allowed_annotation_property_type(typ) { return None; }
            let initializer = match member.initializer() {
                Some(node) => Some(self.evaluate_annotation_constant(node, &mut FxHashSet::default())?),
                None => None,
            };
            let name = ast::try_get_text_of_property_name(member.name()?)?;
            let mut property = AnnotationPropertyInfo {
                declaration: member, typ, initializer, argument: arguments.get(&name).cloned(),
                array_depth: 0, element_type: typ, enum_declaration: None, enum_first_value: None,
            };
            while let Some(element) = self.get_element_type_of_array_type(property.element_type) {
                property.array_depth += 1;
                property.element_type = element;
            }
            if let Some(symbol) = property.element_type.symbol().filter(|s| s.flags().intersects(SymbolFlags::ConstEnum)) {
                for &decl in symbol.declarations() {
                    if ast::is_enum_declaration(decl) {
                        property.enum_declaration = Some(decl);
                        if let Some(&first) = decl.members().first() {
                            property.enum_first_value = self.get_constant_value(first).map(AnnotationConstant::Scalar);
                        }
                        break;
                    }
                }
            }
            info.properties.push(property);
        }
        Some(info)
    }

    // OH checker/annotation_info.go::GetAnnotationImportDisposition.
    pub fn get_annotation_import_disposition(&mut self, node: P<Node>) -> &'static str {
        if !ast::is_import_specifier(node) || ast::is_type_only_import_or_export_declaration(node) { return "unchanged"; }
        let Some(symbol) = self.get_symbol_of_declaration(node) else { return "unchanged"; };
        let target = self.resolve_alias(symbol);
        if target == self.unknown_symbol { return "unchanged"; }
        if target.flags().intersects(SymbolFlags::ConstEnum) {
            for &declaration in target.declarations() {
                if ast::is_enum_declaration(declaration) && declaration.name().unwrap().text() == "RetentionPolicy"
                    && tspath::get_base_file_name(ast::get_source_file_of_node(declaration).unwrap().file_name()).eq_ignore_ascii_case("@arkts.lang.d.ets") {
                    return "remove";
                }
            }
        }
        if !is_annotation_symbol(Some(target)) { return "unchanged"; }
        let Some(declaration) = target.value_declaration().or_else(|| target.declarations().first().copied()) else { return "unchanged"; };
        if !ast::is_annotation_declaration(declaration) { return "unchanged"; }
        if is_retention_annotation_declaration(declaration) || self.is_source_retention_annotation_declaration(declaration) { "remove" } else { "rename" }
    }
}
