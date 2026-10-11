use std::sync::Arc;
use tsrs_api_codec::{decode_source_file, encode_source_file};
use tsrs_ast::{self as ast, SourceFileParseOptions};
use tsrs_core::{ScriptKind, TextRange, P};

fn shape(node: P<ast::Node>, output: &mut Vec<String>) {
    let extra = if ast::is_call_expression(node) {
        format!("body={}", node.as_call_expression().ets_body().is_some())
    } else if ast::is_import_clause(node) {
        format!("lazy={},phase={:?}", node.as_import_clause().is_lazy(), node.as_import_clause().phase_modifier())
    } else { String::new() };
    output.push(format!("{:?}:{:?}:{}:{extra}", node.kind(), node.flags(), node.is_virtual()));
    node.for_each_child(&mut |child| { shape(child, output); false });
}

#[test]
fn arkts_binary_tree_retains_virtual_nodes_and_parser_identity() {
    let region = tsrs_core::arena::Region::new(1 << 18);
    let _scope = region.enter();
    let opts = SourceFileParseOptions {
        file_name: "/input.ets".into(),
        ets: Arc::new(serde_json::from_str(include_str!("../../tsrs_parser/tests/fixtures/ets.json")).unwrap()),
        ets_annotations_enable: true,
        open_harmony49: true,
        ets_loader_path: "/loader".into(),
        no_transformed_kit_in_parser: true,
        ..Default::default()
    };
    let file = tsrs_parser::parse_source_file(opts.clone(),
        "/*😀*/ import lazy { value } from 'mod'; @interface Mark { value: string = 'x' } struct Page { build() { Column() { Text('value') } } }", ScriptKind::ETS);
    assert!(file.diagnostics().is_empty());
    let import_end = file.statements().nodes()[0].end();
    file.marked_kit_import_ranges.set(tsrs_core::alloc_slice(&[TextRange::new(0, import_end)]));
    let utf16_end = file.text()[..import_end as usize].encode_utf16().count() as i32;
    let metadata: serde_json::Value = serde_json::from_str(&tsrs_api_codec::encoder::source_file_arkts_options_json(file.get())).unwrap();
    assert_eq!(metadata["markedKitImportRanges"], serde_json::json!([[0, utf16_end]]));
    let (bytes, _) = encode_source_file(file.get()).unwrap();
    let decoded = decode_source_file(&bytes).unwrap();
    let result = decoded.root().as_source_file();
    assert_eq!(result.parse_options(), &opts);
    assert_eq!(result.marked_kit_import_ranges.get(), &[TextRange::new(0, utf16_end)]);
    let (mut before, mut after) = (Vec::new(), Vec::new());
    shape(file.as_node(), &mut before);
    shape(decoded.root(), &mut after);
    assert_eq!(before, after);
    assert!(before.iter().any(|line| line.contains("body=true")));
    assert!(before.iter().any(|line| line.contains("lazy=true")));
    // Also consumed by the owned Node SDK integration test.
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/arkts-codec.bin");
    std::fs::write(out, bytes).unwrap();
}
