use super::*;

fn source(path: &str, text: &'static str) -> P<SourceFile> {
    let factory = ast::NodeFactory::default();
    factory.new_source_file(ast::SourceFileParseOptions { file_name: path.into(), ..Default::default() },
        text, factory.new_node_list(vec![]), factory.new_token(Kind::EndOfFile)).as_source_file_p()
}

#[test]
fn oh_sdk_file_facts_distinguish_live_versions_at_the_same_path() {
    let options = CompilerOptions { oh_project_root_path: "/app".into(), ..Default::default() };
    let old = source("/app/api.ets", "class API {}");
    let next = source("/app/api.ets", "@Available(17) class API {} // deviceInfo");
    let mut state = OhSdkState::default();
    assert!(!state.file_facts(old, &options).project_available);
    let facts = state.file_facts(next, &options);
    assert!(facts.project_available && facts.device_info);
    let facts = state.file_facts(old, &options);
    assert!(!facts.project_available && !facts.device_info);
    assert_eq!(state.files.len(), 2, "cache by immutable SourceFile ID, never by file name");
}

#[test]
fn oh_sdk_file_classification_preserves_directory_and_basename_rules() {
    let options = CompilerOptions {
        oh_project_root_path: "/app".into(), oh_module_path: "/app/module".into(),
        oh_card_entry_files: Some(vec!["/app/cards/../card.ets".into()]),
        oh_system_modules: Some(vec!["system.ets".into()]),
        oh_external_api_paths: Some(vec!["/sdk/api".into()]),
        oh_global_module_paths: Some(vec!["/sdk/api/".into()]),
        oh_ark_ui_declaration_dirs: Some(vec!["/sdk/ui".into()]),
        ets_loader_path: "/loader".into(), ..Default::default()
    };
    let mut state = OhSdkState::default();
    assert!(state.file_facts(source("/app/card.ets", ""), &options).card);
    assert!(state.file_facts(source("/app/module/src/ohosTest/test.ets", ""), &options).ohos_test);
    assert!(state.file_facts(source("/sdk/system.ets", ""), &options).system);
    assert!(!state.file_facts(source("/sdk/api-other/a.d.ts", ""), &options).sdk);
    let file = source("/sdk/api/@ohos.deviceInfo.d.ts", "");
    let facts = state.file_facts(file, &options);
    assert!(facts.sdk);
    assert_eq!(facts.whitelist, ["apiAvailable"]);
    assert_eq!(facts.api_module, "@ohos.deviceInfo");
    assert!(state.file_facts(source("/sdk/api/nested/@ohos.deviceInfo.d.ts", ""), &options).whitelist.is_empty());
    for directory in ["/sdk/ui", "/loader/declarations"] {
        assert!(state.file_facts(source(&format!("{directory}/view.d.ts"), ""), &options).arkui);
        for name in ["global.d.ts", "common_ts_ets_api.d.ts", "nested/view.d.ts"] {
            let facts = state.file_facts(source(&format!("{directory}/{name}"), ""), &options);
            assert!(facts.sdk);
            assert!(!facts.arkui, "ArkUI warning applies only to direct non-exempt declarations");
        }
    }
    assert!(state.file_facts(source("/app/input.ts", ""), &options).ts_implementation);
    assert!(!state.file_facts(source("/app/input.d.ts", ""), &options).ts_implementation);
}
