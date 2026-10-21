use crate::parser_1::JsdocScannerInfo;
use crate::utilities::token_is_identifier_or_keyword;
use crate::{Parser, ParsingContext};
use tsrs_ast::{self as ast, Kind, ModifierList, Node, NodeFlags};
use tsrs_core::{P, ScriptKind};
use tsrs_diagnostics as diagnostics;

impl Parser {
    pub(crate) fn is_annotation_declaration(&mut self) -> bool {
        self.script_kind == ScriptKind::ETS
            && self.opts.ets_annotations_enable
            && self.token == Kind::AtToken
            && self.look_ahead(|p| p.next_token() == Kind::InterfaceKeyword)
    }

    pub(crate) fn parse_annotation_declaration(
        &mut self,
        pos: u32,
        jsdoc: JsdocScannerInfo,
        modifiers: Option<P<ModifierList>>,
    ) -> P<Node> {
        let at = self.scanner.token_start();
        self.parse_expected(Kind::AtToken);
        let keyword = self.scanner.token_start();
        self.parse_expected(Kind::InterfaceKeyword);
        if keyword - at > 1 {
            self.parse_error_at(
                at + 1,
                keyword,
                &diagnostics::In_annotation_declaration_any_symbols_between_and_interface_are_forbidden,
                &[],
            );
        }
        let is_binding_identifier = self.is_binding_identifier();
        let name = self.create_identifier(is_binding_identifier);
        let members = if self.parse_expected(Kind::OpenBraceToken) {
            let members = self.parse_list(ParsingContext::AnnotationMembers, Parser::parse_annotation_element);
            self.parse_expected(Kind::CloseBraceToken);
            members
        } else {
            self.create_missing_list()
        };
        let result = self.factory.new_class_declaration(modifiers, Some(name), None, None, members);
        result.set_flags(result.flags() | NodeFlags::Annotation);
        self.finish_node(result, pos);
        self.with_jsdoc(result, jsdoc);
        result
    }

    pub(crate) fn scan_annotation_member_start(&mut self) -> bool {
        if !token_is_identifier_or_keyword(self.token) || ast::is_keyword(self.token) {
            return false;
        }
        self.next_token();
        self.token == Kind::ColonToken || self.token == Kind::EqualsToken || self.can_parse_semicolon()
    }

    fn parse_annotation_element(&mut self) -> P<Node> {
        let pos = self.node_pos();
        let jsdoc = self.jsdoc_scanner_info();
        let name = self.parse_property_name();
        let type_node = self.parse_type_annotation();
        let initializer = self.do_in_context(
            NodeFlags::YieldContext | NodeFlags::AwaitContext | NodeFlags::DisallowInContext,
            false,
            Parser::parse_initializer,
        );
        self.parse_semicolon_after_property_name(name, type_node, initializer);
        let node = self.factory.new_property_declaration(None, name, None, type_node, initializer);
        node.set_flags(node.flags() | NodeFlags::Annotation);
        self.finish_node(node, pos);
        self.with_jsdoc(node, jsdoc);
        node
    }
}
