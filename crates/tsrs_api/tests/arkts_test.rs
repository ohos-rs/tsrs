#[expect(dead_code, reason = "This target uses only part of the shared integration helper module.")]
mod common;
use common::*;
use tsrs_core::json;

#[test]
fn kit_expansion_survives_snapshot_updates_and_reports_resolved_types() {
    let dir = TempDir::new("arkts-kit");
    dir.write("entry.ets", "import { Foo as Local } from '@kit.Test'; const x: number = new Local().value;");
    dir.write("sdk/api/@ohos.foo.d.ts", "export class Foo { value: number }");
    dir.write("sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json", r#"{"symbols":{"Foo":{"source":"@ohos.foo.d.ts","bindings":"Foo"}}}"#);
    let session = session(&dir.dir(), false);
    let options = format!(r#"{{"noEmit":true,"skipLibCheck":true,"module":99,"moduleResolution":100,"etsLoaderPath":{},"ohSdkConfigs":[{{"apiPaths":[{}]}}],"ohSystemModules":["@ohos.foo.d.ts","@ohos.bar.d.ts"]}}"#, quote(&dir.path("sdk/openharmony/ets/build-tools/ets-loader")), quote(&dir.path("sdk/api")));
    let created = call(&session, "createSnapshot", &format!(r#"{{"createPrograms":[{{"rootFiles":[{}],"compilerOptions":{options}}}]}}"#, quote(&dir.path("entry.ets"))));
    let snapshot = json::marshal(get(&created, "snapshot")).unwrap();
    let project = quote(str_of(get(&created, "operation.createdPrograms.0")));
    let params = format!(r#"{{"snapshot":{snapshot},"project":{project}}}"#);
    assert_eq!(json::marshal(&call(&session, "getSemanticDiagnostics", &params)).unwrap(), "[]");
    let names = json::marshal(&call(&session, "getSourceFileNames", &params)).unwrap();
    assert!(names.contains("@ohos.foo.d.ts"), "{names}");
    dir.write("entry.ets", "import { Foo as Local } from '@kit.Test'; const x: string = new Local().value;");
    let root = quote(&dir.path("entry.ets"));
    let updated = call(&session, "updateSnapshot", &format!(r#"{{"snapshot":{snapshot},"changes":{{"reconfigurePrograms":[{{"id":{project},"rootFiles":[{root}],"compilerOptions":{options}}}],"fileNotifications":{{"changed":[{}]}}}}}}"#, quote(&dir.path("entry.ets"))));
    let next = json::marshal(get(&updated, "snapshot")).unwrap();
    let next_params = format!(r#"{{"snapshot":{next},"project":{project}}}"#);
    let diagnostics = json::marshal(&call(&session, "getSemanticDiagnostics", &next_params)).unwrap();
    assert!(diagnostics.contains("2322"), "{diagnostics}");
    dir.write("sdk/api/@ohos.bar.d.ts", "export class Foo { value: string }");
    let kit = dir.write("sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json", r#"{"symbols":{"Foo":{"source":"@ohos.bar.d.ts","bindings":"Foo"}}}"#);
    let changed_kit = call(&session, "updateSnapshot", &format!(r#"{{"snapshot":{next},"changes":{{"ensurePrograms":[{project}],"fileNotifications":{{"changed":[{}],"created":[{}]}}}}}}"#, quote(&kit), quote(&dir.path("sdk/api/@ohos.bar.d.ts"))));
    let newest = json::marshal(get(&changed_kit, "snapshot")).unwrap();
    assert_eq!(json::marshal(&call(&session, "getSemanticDiagnostics", &format!(r#"{{"snapshot":{newest},"project":{project}}}"#))).unwrap(), "[]");
    std::fs::remove_file(&kit).unwrap();
    let removed = call(&session, "updateSnapshot", &format!(r#"{{"snapshot":{newest},"changes":{{"ensurePrograms":[{project}],"fileNotifications":{{"deleted":[{}]}}}}}}"#, quote(&kit)));
    let removed_snapshot = json::marshal(get(&removed, "snapshot")).unwrap();
    let removed_diagnostics = json::marshal(&call(&session, "getSemanticDiagnostics", &format!(r#"{{"snapshot":{removed_snapshot},"project":{project}}}"#))).unwrap();
    assert!(removed_diagnostics.contains("2307"), "{removed_diagnostics}");
    dir.write("sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json", r#"{"symbols":{"Foo":{"source":"@ohos.bar.d.ts","bindings":"Foo"}}}"#);
    let restored = call(&session, "updateSnapshot", &format!(r#"{{"snapshot":{removed_snapshot},"changes":{{"ensurePrograms":[{project}],"fileNotifications":{{"created":[{}]}}}}}}"#, quote(&kit)));
    let restored_snapshot = json::marshal(get(&restored, "snapshot")).unwrap();
    assert_eq!(json::marshal(&call(&session, "getSemanticDiagnostics", &format!(r#"{{"snapshot":{restored_snapshot},"project":{project}}}"#))).unwrap(), "[]");
    // The old snapshot keeps its original bound file after the new parse replaces imports.
    assert_eq!(json::marshal(&call(&session, "getSemanticDiagnostics", &params)).unwrap(), "[]");
}

#[test]
fn arkts_linter_uses_esobject_library_in_both_diagnostic_apis() {
    let dir = TempDir::new("arkts-linter");
    dir.write("entry.ets", "declare function foo(): ESObject; function Fn() { let f1: ESObject = foo(); let f2 = f1; }");
    let session = session(&dir.dir(), false);
    let created = call(&session, "createSnapshot", &format!(r#"{{"createPrograms":[{{"rootFiles":[{}],"compilerOptions":{{"noEmit":true,"strict":false,"skipLibCheck":true,"needDoArkTsLinter":true}}}}]}}"#, quote(&dir.path("entry.ets"))));
    let params = format!(r#"{{"snapshot":{},"project":{}}}"#, json::marshal(get(&created, "snapshot")).unwrap(), quote(str_of(get(&created, "operation.createdPrograms.0"))));
    let diagnostics = json::marshal(&call(&session, "getArkTSLinterDiagnostics", &params)).unwrap();
    assert!(diagnostics.contains("arkts-no-any-unknown"), "{diagnostics}");
    let build = call(&session, "getArkTSBuildDiagnostics", &params);
    assert_eq!(json::marshal(get(&build, "linterDiagnostics")).unwrap(), diagnostics);
}
