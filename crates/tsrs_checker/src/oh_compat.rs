//! OH 4.9 semantic rules selected by etsLoaderPath. Ordinary TS keeps the pinned base behavior.
use crate::*;
impl Checker {
    pub(crate) fn open_harmony_iterable_type_arguments(&self, yielded: P<Type>) -> Vec<P<Type>> {
        if self.compiler_options.ets_loader_path.is_empty() { vec![yielded, self.void_type, self.undefined_type] } else { vec![yielded] }
    }
    pub(crate) fn open_harmony_flow_depth_limit(&self) -> f64 {
        let depth = self.compiler_options.max_flow_depth;
        if depth == 0.0 { f64::from(crate::flowmemo::FLOW_DEPTH_LIMIT) } else { depth }
    }
    pub(crate) fn is_open_harmony_literal_enum(&mut self, symbol: P<Symbol>) -> bool {
        let mut non_literal = false;
        for declaration in symbol.declarations() {
            if declaration.kind() != Kind::EnumDeclaration { continue; }
            for &member in declaration.members() {
                if member.initializer().is_some_and(ast::is_string_literal_like) { return true; }
                if !self.is_open_harmony_literal_enum_member(member) { non_literal = true; }
            }
        }
        !non_literal
    }
    fn is_open_harmony_literal_enum_member(&mut self, member: P<Node>) -> bool {
        let Some(initializer) = member.initializer() else { return !member.flags().intersects(NodeFlags::Ambient); };
        match initializer.kind() {
            Kind::StringLiteral | Kind::NumericLiteral | Kind::NoSubstitutionTemplateLiteral => true,
            Kind::PrefixUnaryExpression => {
                let expression = initializer.as_prefix_unary_expression();
                expression.operator == Kind::MinusToken && expression.operand.kind() == Kind::NumericLiteral
            }
            Kind::Identifier => ast::node_is_missing(Some(initializer)) || self.get_symbol_of_declaration(member.parent().unwrap()).and_then(|s| s.exports()).is_some_and(|e| e.lookup(initializer.text()).is_some()),
            Kind::BinaryExpression => Self::is_open_harmony_string_concat(initializer),
            _ => false,
        }
    }
    fn is_open_harmony_string_concat(expression: P<Node>) -> bool {
        if ast::is_string_literal_like(expression) { return true; }
        if !ast::is_binary_expression(expression) { return false; }
        let binary = expression.as_binary_expression();
        Self::is_open_harmony_string_concat(binary.left) && Self::is_open_harmony_string_concat(binary.right())
    }
    pub(crate) fn get_open_harmony_inferred_type(&mut self, n: P<InferenceContext>, index: usize) -> P<Type> {
        let inference = n.inferences.get()[index];
        let type_parameter = inference.type_parameter.get().unwrap();
        let inferred = if let Some(signature) = n.signature.get() {
            let covariant = if inference.candidates.is_empty() { None } else { Some(self.get_covariant_inference(inference, signature)) };
            if !inference.contra_candidates.is_empty() {
                if covariant.is_some_and(|t| !t.flags().intersects(TypeFlags::Never) && inference.contra_candidates.to_vec().iter().any(|&candidate| self.is_type_subtype_of(t, candidate))) { covariant }
                else { self.get_contravariant_inference(inference) }
            } else if covariant.is_some() { covariant }
            else if n.flags.get().intersects(InferenceFlags::NoDefault) { Some(self.silent_never_type) }
            else if let Some(default) = self.get_default_from_type_parameter(type_parameter) {
                let backreference = self.new_backreference_mapper(n, index);
                let mapper = merge_type_mappers(Some(backreference), n.non_fixing_mapper().unwrap());
                Some(self.instantiate_type(default, Some(mapper)))
            } else { None }
        } else { self.get_type_from_inference(inference) };
        inference.inferred_type.set(Some(inferred.unwrap_or(if n.flags.get().intersects(InferenceFlags::AnyDefault) { self.any_type } else { self.unknown_type })));
        if let Some(constraint) = self.get_constraint_of_type_parameter(type_parameter) {
            let constraint = self.instantiate_type(constraint, n.non_fixing_mapper());
            let replace = if let Some(inferred) = inferred {
                let target = self.get_type_with_this_argument(constraint, Some(inferred), false);
                n.compare_types.get().unwrap()(self, inferred, target, false) == Ternary::False
            } else { true };
            if replace { inference.inferred_type.set(Some(constraint)); }
        }
        self.clear_active_mapper_caches();
        inference.inferred_type.get().unwrap()
    }
}
