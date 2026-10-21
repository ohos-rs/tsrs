#[expect(dead_code, reason = "This target uses only part of the shared integration helper module.")]
mod common;
use common::*;
use std::sync::Arc;
use tsrs_api::{
    Session, SessionOptions,
    embedded::{Compiler, FileChanges},
};
use tsrs_core::{CompilerOptions, Tristate};
use tsrs_vfs::{FS, bundled, osvfs};

fn options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Tristate::True,
        skip_lib_check: Tristate::True,
        strict_null_checks: Tristate::True,
        ..Default::default()
    }
}
fn compiler(cwd: &str, fs: Arc<dyn FS>) -> Compiler {
    Compiler::new(SessionOptions::new(
        cwd.into(),
        bundled::lib_path(),
        fs,
        false,
    ))
    .unwrap()
}

#[test]
fn snapshots_and_types_own_their_memory_and_reject_foreign_handles() {
    let dir = TempDir::new("embedded-ownership");
    let a = dir.write(
        "a.ets",
        "export class A { value = 1 }; let x: A | undefined;",
    );
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let weak = Arc::downgrade(&fs);
    let service = compiler(&dir.dir(), Arc::clone(&fs));
    let old = service
        .create_program(&[a.clone()], options(), Arc::clone(&fs))
        .unwrap();
    let types = old.types_at_positions(&a, &[45]).unwrap();
    let source = dir.write(
        "a.ets",
        "export class A { value = 'new' }; let x: A | undefined;",
    );
    let next = service
        .update_program(
            &old,
            &[a.clone()],
            options(),
            Arc::clone(&fs),
            &FileChanges {
                changed: vec![source],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        next.non_nullable_type(&types[0])
            .unwrap_err()
            .message
            .contains("another program")
    );
    let other = compiler(&dir.dir(), Arc::clone(&fs));
    assert!(
        other
            .update_program(
                &old,
                &[a],
                options(),
                Arc::clone(&fs),
                &FileChanges::default()
            )
            .unwrap_err()
            .message
            .contains("another compiler")
    );
    drop(other);
    drop(service);
    drop(fs);
    drop(next);
    drop(old);
    assert!(
        weak.upgrade().is_some(),
        "type lease must keep its compiler alive"
    );
    drop(types);
    assert!(
        weak.upgrade().is_none(),
        "last type lease must free the compiler and its filesystem"
    );
}

#[test]
fn incremental_kit_changes_preserve_old_snapshots() {
    let dir = TempDir::new("embedded-kit");
    let a = dir.write(
        "a.ets",
        "import { Foo } from '@kit.Test'; const x: number = new Foo().value;",
    );
    dir.write(
        "sdk/api/@ohos.foo.d.ts",
        "export class Foo { value: number }",
    );
    dir.write(
        "sdk/api/@ohos.bar.d.ts",
        "export class Foo { value: string }",
    );
    let kit = dir.write(
        "sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json",
        r#"{"symbols":{"Foo":{"source":"@ohos.foo.d.ts","bindings":"Foo"}}}"#,
    );
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = compiler(&dir.dir(), Arc::clone(&fs));
    let opts = CompilerOptions {
        module: tsrs_core::ModuleKind::ESNext,
        module_resolution: tsrs_core::ModuleResolutionKind::Bundler,
        ets_loader_path: dir.path("sdk/openharmony/ets/build-tools/ets-loader"),
        oh_sdk_configs: Some(vec![tsrs_core::OhSdkConfig {
            api_paths: vec![dir.path("sdk/api")],
        }]),
        oh_system_modules: Some(vec!["@ohos.foo.d.ts".into(), "@ohos.bar.d.ts".into()]),
        ..options()
    };
    let old = service
        .create_program(&[a.clone()], opts.clone(), Arc::clone(&fs))
        .unwrap();
    assert!(
        old.semantic_diagnostics().unwrap().is_empty(),
        "{:?}",
        old.semantic_diagnostics().unwrap()
    );
    dir.write(
        "sdk/openharmony/ets/build-tools/ets-loader/kit_configs/@kit.Test.json",
        r#"{"symbols":{"Foo":{"source":"@ohos.bar.d.ts","bindings":"Foo"}}}"#,
    );
    let new = service
        .update_program(
            &old,
            &[a],
            opts,
            fs,
            &FileChanges {
                changed: vec![kit],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        new.semantic_diagnostics()
            .unwrap()
            .iter()
            .any(|d| d.code == 2322)
    );
    assert!(
        old.semantic_diagnostics().unwrap().is_empty(),
        "{:?}",
        old.semantic_diagnostics().unwrap()
    );
}

#[test]
fn typed_diagnostics_match_wire_utf16_and_message_chains() {
    let dir = TempDir::new("embedded-diagnostics");
    let a = dir.write("a.ets", "const emoji = '😀'; const wrong: number = '中文';");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = compiler(&dir.dir(), Arc::clone(&fs));
    let program = service.create_program(&[a.clone()], options(), fs).unwrap();
    let typed = program.semantic_diagnostics().unwrap();
    let wire = session(&dir.dir(), false);
    let created = call(
        &wire,
        "createSnapshot",
        &format!(
            r#"{{"createPrograms":[{{"rootFiles":[{}],"compilerOptions":{{"noEmit":true,"skipLibCheck":true,"strictNullChecks":true}}}}]}}"#,
            quote(&a)
        ),
    );
    let result = call(
        &wire,
        "getSemanticDiagnostics",
        &format!(
            r#"{{"snapshot":{},"project":{}}}"#,
            tsrs_core::json::marshal(get(&created, "snapshot")).unwrap(),
            quote(str_of(get(&created, "operation.createdPrograms.0")))
        ),
    );
    let wire: serde_json::Value =
        serde_json::from_str(&tsrs_core::json::marshal(&result).unwrap()).unwrap();
    assert_eq!(typed.len(), wire.as_array().unwrap().len());
    for (d, w) in typed.iter().zip(wire.as_array().unwrap()) {
        assert_eq!(d.pos, w["pos"].as_i64().unwrap());
        assert_eq!(d.end, w["end"].as_i64().unwrap());
        assert_eq!(d.code, w["code"].as_i64().unwrap() as i32);
        assert_eq!(d.text, w["text"].as_str().unwrap());
    }
}

struct EmptyPluginHost;
impl tsrs_api::embedded::SdkPluginHost for EmptyPluginHost {
    fn invoke_batch(
        &self,
        requests: Vec<tsrs_api::embedded::SdkPluginRequest>,
    ) -> Result<Vec<tsrs_core::OhSdkPluginResponse>, String> {
        Ok(requests
            .iter()
            .map(|_| tsrs_core::OhSdkPluginResponse::default())
            .collect())
    }
}

fn plugin_options(host: Arc<dyn tsrs_api::embedded::SdkPluginHost>) -> CompilerOptions {
    CompilerOptions {
        need_do_ark_ts_linter: Tristate::True,
        checkers: Some(2),
        oh_sdk_plugin_executor: Some(Arc::new(tsrs_api::embedded::SdkPluginExecutor::new(host))),
        oh_sdk_class_check_plugins: Some(vec![tsrs_core::OhSdkClassCheckPlugin {
            path: "host.js".into(),
            class_name: "Checker".into(),
            ..Default::default()
        }]),
        ..options()
    }
}

#[test]
fn repeated_compilers_release_options_plugins_and_filesystems() {
    let dir = TempDir::new("embedded-repeat");
    let a = dir.write("a.ets", "export const value = 1;");
    for _ in 0..12 {
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
        let weak = Arc::downgrade(&fs);
        let host: Arc<dyn tsrs_api::embedded::SdkPluginHost> = Arc::new(EmptyPluginHost);
        let weak_host = Arc::downgrade(&host);
        let service = compiler(&dir.dir(), Arc::clone(&fs));
        let mut program = service
            .create_program(&[a.clone()], plugin_options(host), Arc::clone(&fs))
            .unwrap();
        program.arkts_build_diagnostics().unwrap();
        for index in 0..4 {
            dir.write("a.ets", &format!("export const value = {index};"));
            program = service
                .update_program(
                    &program,
                    &[a.clone()],
                    options(),
                    Arc::clone(&fs),
                    &FileChanges {
                        changed: vec![a.clone()],
                        ..Default::default()
                    },
                )
                .unwrap();
            program.semantic_diagnostics().unwrap();
        }
        drop(program);
        drop(service);
        drop(fs);
        assert!(weak.upgrade().is_none());
        assert!(
            weak_host.upgrade().is_none(),
            "options must release their SDK host"
        );
    }
}

#[test]
fn config_requests_do_not_leak_the_session_host() {
    let dir = TempDir::new("embedded-config-host");
    let config = dir.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"noEmit":true},"files":["a.ts"]}"#,
    );
    dir.write("a.ts", "export {};");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let weak = Arc::downgrade(&fs);
    let s = Session::new(SessionOptions::new(
        dir.dir(),
        bundled::lib_path(),
        fs,
        false,
    ));
    call(
        &s,
        "parseConfigFile",
        &format!(r#"{{"file":{}}}"#, quote(&config)),
    );
    drop(s);
    assert!(
        weak.upgrade().is_none(),
        "configuration parsing retained its filesystem"
    );
}

#[test]
fn embedded_requests_do_not_use_the_hosts_small_stack() {
    let dir = TempDir::new("embedded-stack");
    let source = format!("const value = {}1{};", "(".repeat(1800), ")".repeat(1800));
    let file = dir.write("deep.ets", &source);
    std::thread::Builder::new()
        .stack_size(128 << 10)
        .spawn(move || {
            let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
            let service = compiler(&dir.dir(), Arc::clone(&fs));
            let program = service.create_program(&[file], options(), fs).unwrap();
            assert!(program.syntactic_diagnostics().unwrap().is_empty());
            program.semantic_diagnostics().unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn failed_host_request_releases_inputs_and_keeps_the_compiler_usable() {
    struct PanicHost;
    impl tsrs_api::embedded::SdkPluginHost for PanicHost {
        fn invoke_batch(
            &self,
            _: Vec<tsrs_api::embedded::SdkPluginRequest>,
        ) -> Result<Vec<tsrs_core::OhSdkPluginResponse>, String> {
            panic!("host failure regression")
        }
    }
    let dir = TempDir::new("embedded-host-failure");
    let a = dir.write("a.ets", "const value: number = 'error';");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = compiler(&dir.dir(), Arc::clone(&fs));
    let old = service
        .create_program(&[a.clone()], options(), Arc::clone(&fs))
        .unwrap();
    let host: Arc<dyn tsrs_api::embedded::SdkPluginHost> = Arc::new(PanicHost);
    let weak = Arc::downgrade(&host);
    let error = service
        .update_program(
            &old,
            &[a.clone()],
            plugin_options(host),
            Arc::clone(&fs),
            &FileChanges::default(),
        )
        .unwrap_err();
    assert!(error.message.contains("host failure regression"));
    assert!(weak.upgrade().is_none());
    assert!(
        old.semantic_diagnostics()
            .unwrap()
            .iter()
            .any(|d| d.code == 2322)
    );
    let new = service
        .update_program(&old, &[a], options(), fs, &FileChanges::default())
        .unwrap();
    assert!(
        new.semantic_diagnostics()
            .unwrap()
            .iter()
            .any(|d| d.code == 2322)
    );
}

// R4a: the strict linter consumes owned normal keys after its regions are retired.
// Run this target with TSRS_ARENA_POISON=1 to catch checker-pointer escapes.
#[test]
fn phased_arkts_build_preserves_owned_diagnostics_facts_and_api_types() {
    use tsrs_api::embedded::{ArktsTransformTypeQuery, TypeQueryRange};
    let dir = TempDir::new("embedded-arkts-phases");
    let source = r#"
@interface Marker { value: number = 1; }
@Marker class Value { property: number = 1; }
let maybe: Value | undefined;
const emoji = '😀'; const wrong: number = '中文';
function implicit(value) { return value; }
interface Shape { name: string }
const shape: Shape = { name: 123 };
"#;
    let a = dir.write("a.ets", source);
    let b = dir.write("b.ts", "export const wrong: string = 1;");
    let roots = [a.clone(), b];
    let property = source.find("property").unwrap();
    let query = ArktsTransformTypeQuery {
        file: a.clone().into(),
        properties: vec![TypeQueryRange {
            pos: property,
            end: property + "property".len(),
        }],
        builder_accesses: Vec::new(),
        member_accesses: Vec::new(),
    };
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = compiler(&dir.dir(), Arc::clone(&fs));
    for (linter, strict_only) in [(true, false), (true, true), (false, false)] {
        let opts = CompilerOptions {
            need_do_ark_ts_linter: if linter {
                Tristate::True
            } else {
                Tristate::False
            },
            strict_checker_only: if strict_only {
                Tristate::True
            } else {
                Tristate::False
            },
            checkers: Some(2),
            ets_annotations_enable: Tristate::True,
            ..options()
        };
        let reference = service
            .create_program(&roots, opts.clone(), Arc::clone(&fs))
            .unwrap();
        let expected = reference.arkts_build_diagnostics().unwrap();
        assert!(!expected.semantic_diagnostics.is_empty());
        if linter {
            assert!(!expected.linter_diagnostics.is_empty());
        }
        let annotations = reference.annotation_transform_infos(&roots).unwrap();
        assert!(!annotations[0].declarations.is_empty());
        let facts = reference
            .arkts_transform_type_facts(std::slice::from_ref(&query), &[])
            .unwrap();
        assert_eq!(facts[0].properties.len(), 1);
        let program = service
            .create_program(&roots, opts, Arc::clone(&fs))
            .unwrap();
        let types = program
            .types_at_positions(&a, &[source.find("maybe").unwrap() as u32])
            .unwrap();
        // Reacquisition after retirement must create a live checker, not reuse freed links.
        for _ in 0..2 {
            let normal = program.arkts_normal_diagnostics().unwrap();
            let actual_annotations = program.annotation_transform_infos(&roots).unwrap();
            let actual_facts = program
                .arkts_transform_type_facts(std::slice::from_ref(&query), &[])
                .unwrap();
            let actual = program.arkts_strict_diagnostics(normal).unwrap();
            assert_eq!(
                actual, expected,
                "all fields and original list order must survive retirement"
            );
            assert_eq!(
                serde_json::to_value(actual_annotations).unwrap(),
                serde_json::to_value(&annotations).unwrap()
            );
            assert_eq!(
                serde_json::to_value(actual_facts).unwrap(),
                serde_json::to_value(&facts).unwrap()
            );
            let non_null = program.non_nullable_type(&types[0]).unwrap();
            assert_eq!(
                program.type_symbol(&non_null).unwrap().unwrap().name,
                "Value"
            );
        }
        let foreign = reference.arkts_normal_diagnostics().unwrap();
        assert!(
            program
                .arkts_strict_diagnostics(foreign)
                .unwrap_err()
                .message
                .contains("another program")
        );
        // Transform-only callers can also release the build pool and keep owned facts.
        let facts = program.annotation_transform_infos(&roots).unwrap();
        program.release_arkts_build_checkers().unwrap();
        assert_eq!(
            serde_json::to_value(facts).unwrap(),
            serde_json::to_value(&annotations).unwrap()
        );
    }
}
