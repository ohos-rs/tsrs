use std::sync::Arc;
use tsrs_ast::{self as ast, SourceFileParseOptions};
use tsrs_core::{P, ScriptKind};

fn parse(text: &str, kind: ScriptKind) -> P<ast::SourceFile> {
    let opts = SourceFileParseOptions {
        file_name: if kind == ScriptKind::ETS { "/input.ets" } else { "/input.ts" }.into(),
        ets: Arc::new(serde_json::from_str(include_str!("fixtures/ets.json")).unwrap()),
        ets_annotations_enable: true,
        ..Default::default()
    };
    tsrs_parser::parse_source_file(opts, text, kind)
}

fn valid(text: &str) -> P<ast::SourceFile> {
    let file = parse(text, ScriptKind::ETS);
    let errors: Vec<_> = file
        .diagnostics()
        .iter()
        .map(|d| format!("{} at {}: {}", d.code(), d.pos(), d.localize()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{text}");
    file
}

#[test]
fn arkts_reference_syntax() {
    for source in [
        include_str!("fixtures/struct.ets"),
        include_str!("fixtures/callbacks.ets"),
        include_str!("fixtures/annotations.ets"),
        "@Entry @Component struct Page { @State message: string = 'hello'; build() { Column() { Text(this.message).fontSize(20)\n if (this.message) { Row() { Text('nested') } }\n ForEach([1, 2], (n: number) => { Row() { Text(n.toString()) } })\n }.width('100%') } }",
        "@Builder export function content() { Column() { Text('hello') } }\n export default struct Page { build() { content() } }",
        "export declare struct Widget { value: string; build(): void; }",
        "@Styles function common() { .width(100).height(100) }\n @Extend(Text) function emphasis() { .fontSize(24) }",
        "const struct = 1; function f() { return struct; }\n struct Page { @Builder part() { Column() {} } build() { this.part() } }",
        "struct Page { build() { Text($$this.message).stateStyles({ normal: { .fontSize(20).width(100) } }) } }",
    ] {
        valid(source);
    }
}

#[test]
fn preserves_struct_and_ui_children() {
    let file = valid("struct Page { value: string = ''; build() { Column() { Text(this.value) } } }");
    let node = file.statements().nodes()[0];
    assert!(ast::is_struct_declaration(node));
    assert!(node.members()[0].is_virtual());
    let mut component_count = 0;
    fn visit(node: P<ast::Node>, count: &mut usize) {
        if ast::is_ets_component_expression(node) {
            *count += 1;
        }
        node.for_each_child(&mut |child| {
            visit(child, count);
            false
        });
    }
    visit(node, &mut component_count);
    assert_eq!(component_count, 2);
}

#[test]
fn lazy_import_is_contextual() {
    for source in [
        "import lazy { value } from 'mod';",
        "import lazy value from 'mod';",
        "import lazy value, { other } from 'mod';",
    ] {
        let file = valid(source);
        assert!(
            file.statements().nodes()[0]
                .as_import_declaration()
                .import_clause
                .unwrap()
                .as_import_clause()
                .is_lazy
        );
    }
    let file = valid("import lazy from 'mod';");
    let clause = file.statements().nodes()[0].as_import_declaration().import_clause.unwrap();
    assert!(!clause.as_import_clause().is_lazy);
    assert_eq!(clause.name().unwrap().text(), "lazy");
}

#[test]
fn annotation_diagnostics_and_typescript_isolation() {
    let file = parse("@ interface Bad {}", ScriptKind::ETS);
    assert!(file.diagnostics().iter().any(|d| d.code() == 28037));
    assert!(!parse("@interface Bad {}", ScriptKind::TS).diagnostics().is_empty());
    assert!(
        !parse("struct Page { build() { Column() {} } }", ScriptKind::TS)
            .diagnostics()
            .is_empty()
    );
    for source in [
        "@interface Bad { public value: number }",
        "@interface Bad { ; }",
        "@interface Bad { value() {} }",
    ] {
        assert!(!parse(source, ScriptKind::ETS).diagnostics().is_empty(), "{source}");
    }
}
