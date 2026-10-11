// OH compiler/kitimports_test.go, with AST and reference-inventory assertions.
use super::*;
use tsrs_ast::{self as ast, NodeFlags, SourceFileParseOptions};
use tsrs_core::{ScriptKind, Tristate};
use tsrs_vfs::{bundled, vfstest};
use std::sync::Arc;

const LOADER: &str = "/sdk/openharmony/ets/build-tools/ets-loader";
const CONFIG: &str = "/sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json";
const MAPPING: &str = r#"{"symbols":{"default":{"source":"@ohos.default.d.ts","bindings":"default"},"Foo":{"source":"@ohos.foo.d.ts","bindings":"ExportedFoo"},"Bar":{"source":"@ohos.bar.d.ts","bindings":"Bar"}}}"#;
fn parse(name: &str, text: &str, disabled: bool) -> tsrs_core::P<ast::SourceFile> {
    tsrs_parser::parse_source_file(SourceFileParseOptions {
        file_name: name.into(), path: name.into(), ets_loader_path: LOADER.into(), no_transformed_kit_in_parser: disabled,
        ..Default::default()
    }, text, if name.ends_with(".ets") { ScriptKind::ETS } else { ScriptKind::TS })
}

#[test]
fn kit_imports_define_semantic_program() {
    let fs = Arc::new(bundled::wrap_fs(vfstest::from_map([
        ("/entry.ets", "import lazy DefaultThing, { Foo as Local, type Bar } from '@kit.Test';\nconst value: number = new DefaultThing().value; const local: Local | undefined = undefined; let bar: Bar = {}; value; local; bar;"),
        (CONFIG, MAPPING),
        ("/sdk/api/@ohos.default.d.ts", "export default class DefaultThing { value: number; }"),
        ("/sdk/api/@ohos.foo.d.ts", "export declare class ExportedFoo {}"),
        ("/sdk/api/@ohos.bar.d.ts", "export interface Bar {}"),
    ], true)));
    let mut options = tsrs_core::CompilerOptions::default();
    options.no_emit = Tristate::True;
    options.target = tsrs_core::ScriptTarget::ESNext;
    options.module = tsrs_core::ModuleKind::ESNext;
    options.module_resolution = tsrs_core::ModuleResolutionKind::Bundler;
    options.ets_loader_path = LOADER.into();
    options.oh_sdk_configs = Some(vec![tsrs_core::OhSdkConfig { api_paths: vec!["/sdk/api".into()], ..Default::default() }]);
    options.oh_system_modules = Some(vec!["@ohos.default.d.ts".into(), "@ohos.foo.d.ts".into(), "@ohos.bar.d.ts".into()]);
    let config = tsrs_tsoptions::new_parsed_command_line(tsrs_core::P::new(options), vec!["/entry.ets".into()], Vec::new(), Default::default());
    let program = new_program(ProgramOptions::new(tsrs_core::P::new(config), new_compiler_host("/", fs, &bundled::lib_path(), None, None, None)));
    let context = Context::background();
    assert!(program.get_syntactic_diagnostics(&context, None).is_empty());
    let diagnostics = program.get_semantic_diagnostics(&context, None);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let file = program.get_source_file("/entry.ets").unwrap();
    assert_eq!(file.marked_kit_import_ranges.get().len(), 1);
    let imports = &file.statements().nodes()[..3];
    for (&node, module) in imports.iter().zip(["@ohos.default", "@ohos.foo", "@ohos.bar"]) {
        let import = node.as_import_declaration();
        assert_eq!(import.module_specifier.text(), module);
        assert!(import.import_clause.unwrap().as_import_clause().is_lazy);
        assert!(node.flags().intersects(NodeFlags::KitImport));
        assert!(node.is_virtual());
        assert_eq!(node.parent(), Some(file.as_node()));
    }
    assert_eq!(imports[2].as_import_declaration().import_clause.unwrap().as_import_clause().phase_modifier.get(), ast::Kind::TypeKeyword);
    assert_eq!(file.imports().iter().map(|n| n.text()).collect::<Vec<_>>(), ["@ohos.default", "@ohos.foo", "@ohos.bar"]);
}

#[test]
fn kit_declaration_fallback_and_context_gates() {
    let fs = vfstest::from_map([(CONFIG, MAPPING)], true);
    for (name, source, disabled) in [
        ("/entry.ets", "import { Foo, Missing } from '@kit.Test';", false),
        ("/entry.ets", "const x = 1; import { Foo } from '@kit.Test';", false),
        ("/entry.ets", "import { Foo } from '@kit.Test';", true),
        ("/entry.ets", "import * as Kit from '@kit.Test';", false),
        ("/entry.ets", "import type { type Foo } from '@kit.Test';", false),
    ] {
        let file = parse(name, source, disabled);
        assert!(!KitImportProcessor::default().process(file, &fs), "{source}");
        assert!(file.marked_kit_import_ranges.get().is_empty());
    }
    let file = parse("/entry.ts", "const x = 1; import { Foo as Local } from '@kit.Test';", false);
    assert!(KitImportProcessor::default().process(file, &fs));
    assert_eq!(file.imports()[0].text(), "@ohos.foo");
}

#[test]
fn kit_supplements_component_attributes() {
    let fs = vfstest::from_map([("/sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.ArkUI.json", r#"{"symbols":{"ArcList":{"source":"@ohos.arkui.ArcList.d.ts","bindings":"ArcList"},"ArcListAttribute":{"source":"@ohos.arkui.ArcList.d.ts","bindings":"ArcListAttribute"}}}"#)], true);
    let file = parse("/entry.ets", "import { ArcList } from '@kit.ArkUI'; import { ArcSwiper } from '@ohos.arkui.ArcSwiper';", false);
    assert!(KitImportProcessor::default().process(file, &fs));
    let names: Vec<_> = file.statements().nodes().iter().flat_map(|node| node.as_import_declaration().import_clause.unwrap().as_import_clause().named_bindings.unwrap().as_named_imports().elements().nodes()).map(|n| n.name().unwrap().text()).collect();
    assert_eq!(names, ["ArcList", "ArcListAttribute", "ArcSwiper", "ArcSwiperAttribute"]);
}
