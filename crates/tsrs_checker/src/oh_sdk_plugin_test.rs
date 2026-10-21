use crate::Checker;
use tsrs_ast::{Kind, Node, NodeFactory, NodeFlags, SourceFileParseOptions};
use tsrs_core::{P, TextRange};

fn attach(node: P<Node>, text: &'static str) -> P<Node> {
    let factory = NodeFactory::default();
    let file = factory.new_source_file(SourceFileParseOptions { file_name: "/sdk.ets".into(), ..Default::default() },
        text, factory.new_node_list(vec![]), factory.new_token(Kind::EndOfFile));
    node.set_parent(Some(file));
    node
}
fn verify(node: P<Node>, fallback: bool) {
    let snapshot = Checker::oh_sdk_node_snapshot(node);
    let old = tsrs_scanner::get_text_of_node(node);
    assert_eq!(snapshot.text.is_some(), fallback);
    if let Some(text) = snapshot.text { assert_eq!(text.as_bytes(), old.as_bytes()); }
    else {
        let utf16: Vec<_> = snapshot.source.encode_utf16().collect();
        let slice = String::from_utf16(&utf16[snapshot.start as usize..snapshot.end as usize]).unwrap();
        assert_eq!(slice.as_bytes(), old.as_bytes());
    }
}

#[test]
fn oh_sdk_snapshot_slice_or_legacy_text_matches_scanner_including_jsdoc_types() {
    let factory = NodeFactory::default();
    let text = "\u{feff}// 😀\r\n  name";
    let ordinary = factory.new_identifier("name");
    ordinary.set_loc(TextRange::new(0, text.len() as u32));
    verify(attach(ordinary, text), false);
    let missing = factory.new_identifier("");
    missing.set_loc(TextRange::new(0, 0));
    verify(attach(missing, text), true);
    let virtual_node = factory.new_identifier("virtual");
    virtual_node.set_virtual(true);
    virtual_node.set_loc(TextRange::new(0, 0));
    verify(attach(virtual_node, text), true);

    // scanner/utilities::normalize_jsdoc_type_source_text removes comment
    // markers and normalizes line endings; a source slice cannot represent it.
    let text = "\r\n * T\r\n * U";
    let child = factory.new_identifier("T");
    child.set_flags(NodeFlags::JSDoc);
    child.set_loc(TextRange::new(0, text.len() as u32));
    let type_node = factory.new_type_reference_node(child, None);
    type_node.set_flags(NodeFlags::JSDoc);
    type_node.set_loc(TextRange::new(0, text.len() as u32));
    let expression = factory.new_jsdoc_type_expression(type_node);
    expression.set_loc(TextRange::new(0, text.len() as u32));
    attach(expression, text);
    type_node.set_parent(Some(expression));
    child.set_parent(Some(type_node));
    for node in [expression, type_node, child] { verify(node, true); }
    // A JSDoc flag by itself does not require normalization (a non-type child).
    let identifier = factory.new_identifier("name");
    identifier.set_flags(NodeFlags::JSDoc);
    identifier.set_loc(TextRange::new(0, 4));
    verify(attach(identifier, "name"), false);
}
