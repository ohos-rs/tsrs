#[expect(dead_code, reason = "This target uses only part of the shared integration helper module.")]
mod common;

use std::collections::BTreeSet;
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;
use tsrs_api::embedded::{SdkPluginExecutor, SdkPluginHost, SdkPluginRequest};
use tsrs_core::{
    OhSdkClassCheckPlugin, OhSdkPluginExecutor, OhSdkPluginNodeSnapshot, OhSdkPluginResponse,
};

fn check_syscap(executor: &SdkPluginExecutor, plugin: &OhSdkClassCheckPlugin, node: OhSdkPluginNodeSnapshot,
    declaration: OhSdkPluginNodeSnapshot, config: Arc<serde_json::Map<String, serde_json::Value>>) -> Result<OhSdkPluginResponse, String> {
    executor.check_syscap_batch(std::slice::from_ref(plugin), vec![(node, declaration)], config)
        .pop().unwrap().map(Option::unwrap_or_default)
}

// The host deliberately retains submissions until the test processes them.
// A caller cannot accidentally pass by blocking for the first reply.
struct QueuedHost(
    mpsc::Sender<(
        Vec<SdkPluginRequest>,
        tsrs_api::embedded::SdkPluginCompletion,
    )>,
);
impl QueuedHost {
    fn responses(requests: &[SdkPluginRequest]) -> Vec<OhSdkPluginResponse> {
        requests
            .iter()
            .map(|r| OhSdkPluginResponse {
                found: true,
                check_result: true,
                check_message: if r.operation == "syscap" {
                    format!("SDK warning at {}", r.args[0]["node"]["start"])
                } else {
                    String::new()
                },
                ..Default::default()
            })
            .collect()
    }
}
impl SdkPluginHost for QueuedHost {
    fn invoke_batch(
        &self,
        requests: Vec<SdkPluginRequest>,
    ) -> Result<Vec<OhSdkPluginResponse>, String> {
        Ok(Self::responses(&requests))
    }
    fn submit_batch(
        &self,
        requests: Vec<SdkPluginRequest>,
        complete: tsrs_api::embedded::SdkPluginCompletion,
    ) {
        self.0.send((requests, complete)).unwrap();
    }
}

#[test]
fn concurrent_sdk_checks_publish_snapshots_before_referencing_interned_ids() {
    let (send, receive) = mpsc::channel();
    let executor = SdkPluginExecutor::new(Arc::new(QueuedHost(send)));
    let plugin = OhSdkClassCheckPlugin {
        path: "sdk/SyscapChecker.js".into(),
        class_name: "SyscapChecker".into(),
        ..Default::default()
    };
    let config = Arc::new(
        serde_json::json!({"etsLoaderPath": "sdk/ets-loader"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let submit = |pos| {
        executor.submit_syscap_batch(
            std::slice::from_ref(&plugin),
            vec![(
                OhSdkPluginNodeSnapshot {
                    pos,
                    start: pos,
                    ..snapshot(1)
                },
                snapshot(2),
            )],
            Arc::clone(&config),
        )
    };
    let replies = std::thread::scope(|scope| {
        let first = scope.spawn(|| [submit(0), submit(1)]);
        let second = scope.spawn(|| [submit(2), submit(3)]);
        // Both checkers must finish submitting before any host reply exists.
        [first.join().unwrap(), second.join().unwrap()]
    });
    let batches: Vec<_> = (0..4)
        .map(|_| receive.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    // Keep a newer registration outstanding after the older replies arrive.
    // Any successful publication must acknowledge the ID, not only the newest.
    let held = submit(9);
    let (held_requests, held_complete) = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut registered = BTreeSet::new();
    let mut descriptor = None;
    // Exercise an explicitly non-FIFO transport, including multiple outstanding
    // batches from each checker. Every unacknowledged registration is resent.
    for (requests, complete) in batches.into_iter().rev() {
        let request = &requests[0];
        if let Some(id) = descriptor {
            assert_eq!(request.plugin_id, id);
        }
        descriptor = Some(request.plugin_id);
        assert!(request.path.is_some());
        assert!(request.function_name.is_some());
        assert!(request.class_name.is_some());
        let payload = &request.args[0];
        assert!(payload.get("projectConfig").is_some());
        for field in ["node", "declaration"] {
            assert!(payload[field].get("source").is_some());
            assert!(payload[field].get("text").is_none());
            registered.insert(payload[field]["sourceId"].as_u64().unwrap());
        }
        complete(Ok(QueuedHost::responses(&requests)));
    }
    for reply in replies.into_iter().flatten() {
        assert!(
            reply.wait()[0]
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .check_result
        );
    }
    let last = submit(4);
    let (requests, complete) = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let request = &requests[0];
    assert_eq!(Some(request.plugin_id), descriptor);
    for field in [&request.path, &request.function_name, &request.class_name] {
        assert!(field.is_none());
    }
    assert!(request.args[0].get("projectConfig").is_none());
    for field in ["node", "declaration"] {
        assert!(request.args[0][field].get("source").is_none());
        assert!(registered.contains(&request.args[0][field]["sourceId"].as_u64().unwrap()));
    }
    complete(Ok(QueuedHost::responses(&requests)));
    assert!(last.wait()[0].is_ok());
    held_complete(Ok(QueuedHost::responses(&held_requests)));
    assert!(held.wait()[0].is_ok());
    assert!(submit(4).wait()[0].is_ok(), "cache hit never reaches host");
    assert!(receive.try_recv().is_err());
}

#[derive(Default)]
struct SpanHost {
    batch_sizes: Mutex<Vec<usize>>,
    requests: Mutex<Vec<SdkPluginRequest>>,
    sources: Mutex<std::collections::BTreeMap<u64, String>>,
    legacy_text: Mutex<std::collections::BTreeMap<(String, i32, i32), String>>,
}

impl SpanHost {
    fn record_legacy_text(&self, file: tsrs_core::P<tsrs_ast::SourceFile>) {
        let positions = file.get_position_map();
        let mut pending = vec![file.as_node()];
        let mut expected = self.legacy_text.lock().unwrap();
        while let Some(node) = pending.pop() {
            expected.insert(
                (
                    file.file_name().into(),
                    positions.utf8_to_utf16(node.pos().source_offset().unwrap_or(0)) as i32,
                    positions.utf8_to_utf16(node.end().source_offset().unwrap_or(0)) as i32,
                ),
                tsrs_scanner::get_source_text_of_node_from_source_file(file, node, false),
            );
            node.for_each_child(&mut |child| { pending.push(child); false });
        }
    }
}

impl SdkPluginHost for SpanHost {
    fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
        if requests.first().is_some_and(|r| r.operation == "syscap") { self.batch_sizes.lock().unwrap().push(requests.len()); }
        let mut sources = self.sources.lock().unwrap();
        let expected = self.legacy_text.lock().unwrap();
        for request in &requests {
            if request.operation != "syscap" { continue; }
            for field in ["node", "declaration"] {
                let node = &request.args[0][field];
                assert!(node.get("text").is_none(), "legacy text must not cross the wire");
                let id = node["sourceId"].as_u64().unwrap();
                if let Some(source) = node.get("source") {
                    assert!(sources.insert(id, source.as_str().unwrap().into()).is_none(), "register each source version only once");
                }
                let source = sources.get(&id).expect("register before referencing");
                let utf16: Vec<_> = source.encode_utf16().collect();
                let start = node["start"].as_u64().unwrap() as usize;
                let end = node["end"].as_u64().unwrap() as usize;
                // JS String.slice uses UTF-16 offsets. Compare the resulting UTF-8
                // bytes with the old scanner get_text_of_node path for EVERY request.
                let slice = String::from_utf16(&utf16[start..end]).unwrap();
                let key = (node["fileName"].as_str().unwrap().into(), node["pos"].as_i64().unwrap() as i32, end as i32);
                assert_eq!(slice.as_bytes(), expected[&key].as_bytes(), "{node}");
            }
        }
        let responses = requests.iter().map(|_| OhSdkPluginResponse { found: true, ..Default::default() }).collect();
        self.requests.lock().unwrap().extend(requests);
        Ok(responses)
    }
}

#[test]
fn sdk_spans_preserve_legacy_text_and_update_program_registers_new_source_versions() {
    use common::TempDir;
    use tsrs_api::{SessionOptions, embedded::{Compiler, FileChanges}};
    use tsrs_core::{CompilerOptions, Tristate};
    use tsrs_vfs::{FS, bundled, osvfs};

    let dir = TempDir::new("sdk-source-versions");
    let declaration = "\u{feff}// 😀 SDK\r\n/** @syscap base */\r\ndeclare function 调用(value: '旧'): string;\r\n/** @syscap base */\r\ndeclare class Device {\r\n  /** @syscap base */\r\n  method(value: string): string;\r\n}\r\n";
    let source = "\u{feff}// 😀 uses\r\n/* comment */ 调用('旧');\r\n调用('旧');\r\nconst device = new Device();\r\ndevice. /* 中文 */ method('😀');\r\n";
    let sdk = dir.write("api.d.ets", declaration);
    let app = dir.write("input.ets", source);
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let host = Arc::new(SpanHost::default());
    let parser = tsrs_compiler::new_compiler_host(&dir.dir(), Arc::clone(&fs), &bundled::lib_path(), None, None, None);
    let record = || {
        for name in [&sdk, &app] {
            host.record_legacy_text(parser.get_source_file(tsrs_ast::SourceFileParseOptions {
                file_name: name.clone(), path: tsrs_core::tspath::Path::new(name),
                ets_loader_path: dir.path("ets-loader"), ..Default::default()
            }).unwrap());
        }
    };
    record();
    let plugin_host = Arc::clone(&host);
    let options = CompilerOptions {
        no_emit: Tristate::True,
        skip_lib_check: Tristate::True,
        ets_loader_path: dir.path("ets-loader"),
        oh_compile_mode: "moduleJson".into(),
        oh_device_types: Some(vec!["phone".into()]),
        oh_syscap_intersection: Some(vec!["base".into()]),
        oh_all_module_paths: Some(vec![sdk.clone()]),
        oh_sdk_class_check_plugins: Some(vec![OhSdkClassCheckPlugin {
            tag_name: "syscap".into(), path: "sdk/SyscapChecker.js".into(), class_name: "SyscapChecker".into(),
        }]),
        oh_sdk_plugin_executor: Some(Arc::new(SdkPluginExecutor::new(plugin_host))),
        ..Default::default()
    };
    let service = Compiler::new(SessionOptions::new(dir.dir(), bundled::lib_path(), Arc::clone(&fs), false)).unwrap();
    let roots = [sdk.clone(), app.clone()];
    let old = service.create_program(&roots, options.clone(), Arc::clone(&fs)).unwrap();
    old.semantic_diagnostics().unwrap();
    let before = host.requests.lock().unwrap().len();
    dir.write("api.d.ets", &declaration.replace('旧', "新"));
    dir.write("input.ets", &source.replace('旧', "新"));
    record();
    let next = service.update_program(&old, &roots, options, fs, &FileChanges {
        changed: roots.to_vec(), ..Default::default()
    }).unwrap();
    next.semantic_diagnostics().unwrap();
    let requests = host.requests.lock().unwrap();
    let first: Vec<_> = requests[..before].iter().filter(|r| r.operation == "syscap").collect();
    let second: Vec<_> = requests[before..].iter().filter(|r| r.operation == "syscap").collect();
    assert!(first.len() >= 4, "exercise repeated calls and declaration kinds: {}", first.len());
    assert_eq!(first.len(), second.len());
    assert_eq!(*host.batch_sizes.lock().unwrap(), [first.len(), second.len()], "one host call per checked file version");
    for field in ["node", "declaration"] {
        let original = &first[0].args[0][field];
        let updated = &second[0].args[0][field];
        assert_eq!(first[0].session_id, second[0].session_id);
        assert_eq!(original["fileName"], updated["fileName"]);
        assert_ne!(original["sourceId"], updated["sourceId"]);
        assert!(original["source"].as_str().unwrap().contains('旧'));
        assert!(updated["source"].as_str().unwrap().contains('新'));
        for request in first.iter().skip(1).chain(second.iter().skip(1)) {
            assert!(request.args[0][field].get("source").is_none());
        }
    }
    assert!(first.iter().any(|r| r.args[0]["node"]["start"] != r.args[0]["node"]["pos"]));
}

#[derive(Default)]
struct ResultHost {
    requests: Mutex<Vec<SdkPluginRequest>>,
    failures: Mutex<std::collections::VecDeque<bool>>,
}
impl SdkPluginHost for ResultHost {
    fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
        self.requests.lock().unwrap().extend(requests);
        match self.failures.lock().unwrap().pop_front() {
            Some(true) => Err("transport failed".into()),
            Some(false) => Ok(vec![OhSdkPluginResponse { found: true, phase: "invoke".into(), error: "checker failed".into(), ..Default::default() }]),
            None => Ok(vec![OhSdkPluginResponse { found: true, result: true, check_result: true, ..Default::default() }]),
        }
    }
}
fn plugin() -> tsrs_core::OhSdkCheckPlugin {
    tsrs_core::OhSdkCheckPlugin { path: "sdk/check.cjs".into(), function_name: "check".into(), ..Default::default() }
}
fn snapshot(file: u64) -> OhSdkPluginNodeSnapshot {
    OhSdkPluginNodeSnapshot { file_name: "/a.ets".into(), file_id: file, source: "name", pos: 0, end: 4, start: 0, text: None }
}

#[test]
fn function_results_cache_exact_inputs_and_retry_errors_without_caching_lifecycle_calls() {
    for operation in ["value", "format", "distribution", "regex"] {
        for failure in [false, true] {
            let host = Arc::new(ResultHost::default());
            host.failures.lock().unwrap().push_back(failure);
            let sdk_host = Arc::clone(&host);
            let executor = SdkPluginExecutor::new(sdk_host);
            let args = || vec![serde_json::json!(["17", "😀"])];
            let _ = executor.call(&plugin(), operation, args());
            assert!(executor.call(&plugin(), operation, args()).unwrap().result);
            assert!(executor.call(&plugin(), operation, args()).unwrap().result);
            assert_eq!(host.requests.lock().unwrap().len(), 2, "retry the failure, then cache the success: {operation}");
            executor.call(&plugin(), operation, vec![serde_json::json!("new")]).unwrap();
            let changed_export = tsrs_core::OhSdkCheckPlugin { function_name: "other".into(), ..plugin() };
            executor.call(&changed_export, operation, args()).unwrap();
            let changed_path = tsrs_core::OhSdkCheckPlugin { path: "other/check.cjs".into(), ..plugin() };
            executor.call(&changed_path, operation, args()).unwrap();
            assert_eq!(host.requests.lock().unwrap().len(), 5);
            for lifecycle in ["prepareClass", "releaseSession"] {
                executor.call(&plugin(), lifecycle, vec![]).unwrap();
                executor.call(&plugin(), lifecycle, vec![]).unwrap();
            }
            let requests = host.requests.lock().unwrap();
            assert_eq!(requests.len(), 9);
            let first = &requests[0];
            let second = &requests[1];
            if failure {
                assert_ne!(first.plugin_id, second.plugin_id, "retry a failed descriptor publication with a new ID");
                assert!(second.path.is_some());
            } else {
                assert_eq!(first.plugin_id, second.plugin_id);
                assert!(second.path.is_none());
            }
            let wire = serde_json::to_value(&requests[2]).unwrap();
            for field in ["path", "functionName", "className"] { assert!(wire.get(field).is_none()); }
            assert!(wire["pluginId"].as_u64().unwrap() > 0);
        }
    }
}

#[test]
fn syscap_results_are_exact_version_span_config_and_plugin_keys_and_retry_errors() {
    for failure in [false, true] {
        let host = Arc::new(ResultHost::default());
        host.failures.lock().unwrap().push_back(failure);
        let sdk_host = Arc::clone(&host);
        let executor = SdkPluginExecutor::new(sdk_host);
        let plugin = OhSdkClassCheckPlugin { path: "sdk/check.cjs".into(), class_name: "Checker".into(), ..Default::default() };
        let config = || Arc::new(serde_json::json!({"deviceTypes": ["phone"]}).as_object().unwrap().clone());
        let _ = check_syscap(&executor, &plugin, snapshot(1), snapshot(2), config());
        assert!(check_syscap(&executor, &plugin, snapshot(1), snapshot(2), config()).unwrap().check_result);
        check_syscap(&executor, &plugin, snapshot(1), snapshot(2), config()).unwrap();
        assert_eq!(host.requests.lock().unwrap().len(), 2);
        for index in [0, 1] {
            let request = &host.requests.lock().unwrap()[index];
            assert!(request.args[0].get("projectConfig").is_some(), "republish config after a failure");
            assert!(request.args[0]["node"].get("source").is_some());
            assert!(request.args[0]["declaration"].get("source").is_some());
        }
        check_syscap(&executor, &plugin, snapshot(3), snapshot(2), config()).unwrap();
        check_syscap(&executor, &plugin, snapshot(1), snapshot(4), config()).unwrap();
        check_syscap(&executor, &plugin, OhSdkPluginNodeSnapshot { start: 1, ..snapshot(1) }, snapshot(2), config()).unwrap();
        check_syscap(&executor, &plugin, snapshot(1), OhSdkPluginNodeSnapshot { end: 3, ..snapshot(2) }, config()).unwrap();
        check_syscap(&executor, &plugin, snapshot(1), snapshot(2), Arc::default()).unwrap();
        let other = OhSdkClassCheckPlugin { class_name: "OtherChecker".into(), ..plugin };
        check_syscap(&executor, &other, snapshot(1), snapshot(2), config()).unwrap();
        assert_eq!(host.requests.lock().unwrap().len(), 8);
    }
}

#[test]
fn function_host_calls_for_published_descriptors_do_not_hold_executor_mutexes() {
    struct BlockingHost { entered: mpsc::Sender<()>, release: Mutex<mpsc::Receiver<()>> }
    impl SdkPluginHost for BlockingHost {
        fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
            if requests[0].args[0] == "blocked" {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            }
            Ok(vec![OhSdkPluginResponse { found: true, result: true, ..Default::default() }])
        }
    }
    let (entered, pending) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let executor = SdkPluginExecutor::new(Arc::new(BlockingHost { entered, release: Mutex::new(wait) }));
    executor.call(&plugin(), "value", vec![serde_json::json!("publish")]).unwrap();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| executor.call(&plugin(), "value", vec![serde_json::json!("blocked")]));
        pending.recv_timeout(Duration::from_secs(5)).unwrap();
        let (completed, pending) = mpsc::channel();
        let executor = &executor;
        scope.spawn(move || { completed.send(executor.call(&plugin(), "value", vec![serde_json::json!("independent")])).unwrap(); });
        let result = pending.recv_timeout(Duration::from_secs(2));
        release.send(()).unwrap();
        assert!(result.unwrap().unwrap().result);
        first.join().unwrap().unwrap();
    });
}

#[test]
fn function_cache_distinguishes_operations_and_json_argument_types_and_coalesces_concurrent_hits() {
    let host = Arc::new(ResultHost::default());
    let sdk_host = Arc::clone(&host);
    let executor = SdkPluginExecutor::new(sdk_host);
    for operation in ["value", "format", "distribution", "regex"] {
        for _ in 0..2 { executor.call(&plugin(), operation, vec![serde_json::json!(false)]).unwrap(); }
    }
    assert_eq!(host.requests.lock().unwrap().len(), 4);
    executor.call(&plugin(), "value", vec![serde_json::json!("false")]).unwrap();
    assert_eq!(host.requests.lock().unwrap().len(), 5);
    let concurrent = Barrier::new(8);
    std::thread::scope(|scope| {
        let barrier = &concurrent;
        for _ in 0..8 {
            let executor = &executor;
            scope.spawn(move || {
                barrier.wait();
                executor.call(&plugin(), "value", vec![serde_json::json!("concurrent")]).unwrap();
            });
        }
    });
    let requests = host.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests[0].path.is_some());
    for request in requests.iter().skip(1) {
        assert_eq!(request.plugin_id, requests[0].plugin_id);
        assert!(request.path.is_none());
    }
}

#[test]
fn syscap_host_wait_does_not_block_cache_hits_or_independent_requests() {
    struct BlockingHost { entered: mpsc::Sender<()>, release: Mutex<mpsc::Receiver<()>>, calls: Mutex<Vec<i32>> }
    impl SdkPluginHost for BlockingHost {
        fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
            let pos = requests[0].args[0]["node"]["pos"].as_i64().unwrap() as i32;
            self.calls.lock().unwrap().push(pos);
            if pos == 1 {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            }
            Ok(vec![OhSdkPluginResponse { found: true, check_result: true, ..Default::default() }])
        }
    }
    let (entered, pending) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let host = Arc::new(BlockingHost { entered, release: Mutex::new(wait), calls: Mutex::default() });
    let executor = SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>);
    let plugin = OhSdkClassCheckPlugin { path: "sdk/check.cjs".into(), class_name: "Checker".into(), ..Default::default() };
    let config = Arc::default();
    let check = |pos| check_syscap(&executor, &plugin, OhSdkPluginNodeSnapshot { pos, start: pos, ..snapshot(1) }, snapshot(2), Arc::clone(&config));
    check(0).unwrap(); // Publish all three resources and cache one result.
    std::thread::scope(|scope| {
        let first = scope.spawn(|| check(1));
        pending.recv_timeout(Duration::from_secs(5)).unwrap();
        let (completed, pending) = mpsc::channel();
        let check = &check;
        scope.spawn(move || { completed.send((check(0), check(2))).unwrap(); });
        let result = pending.recv_timeout(Duration::from_secs(2));
        release.send(()).unwrap();
        let (cached, independent) = result.expect("a JS reply must not hold global executor locks");
        assert!(cached.unwrap().check_result);
        assert!(independent.unwrap().check_result);
        first.join().unwrap().unwrap();
    });
    assert_eq!(*host.calls.lock().unwrap(), [0, 1, 2]);
}

#[test]
fn syscap_config_identity_deduplicates_equal_arcs_and_publishes_changed_config() {
    let host = Arc::new(ResultHost::default());
    let executor = SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>);
    let plugin = OhSdkClassCheckPlugin { path: "sdk/check.cjs".into(), class_name: "Checker".into(), ..Default::default() };
    let config = Arc::new(serde_json::json!({"deviceTypes": ["phone"]}).as_object().unwrap().clone());
    let check = |pos, config| check_syscap(&executor, &plugin, OhSdkPluginNodeSnapshot { pos, start: pos, ..snapshot(1) }, snapshot(1), config).unwrap();
    check(0, Arc::clone(&config));
    check(1, Arc::clone(&config));
    check(2, Arc::new((*config).clone()));
    let mut changed = Arc::clone(&config);
    Arc::make_mut(&mut changed).insert("deviceTypes".into(), serde_json::json!(["tablet"]));
    check(0, Arc::clone(&changed));
    check(0, changed);
    check(0, config);
    let requests = host.requests.lock().unwrap();
    assert_eq!(requests.len(), 4, "identical calls with either config hit their own cached result");
    let first = &requests[0].args[0];
    assert!(first["node"].get("source").is_some());
    assert!(first["declaration"].get("source").is_none(), "send a shared node/declaration source once");
    for request in &requests[1..3] {
        assert_eq!(request.args[0]["projectConfigId"], first["projectConfigId"]);
        assert!(request.args[0].get("projectConfig").is_none());
    }
    assert_ne!(requests[3].args[0]["projectConfigId"], first["projectConfigId"]);
    assert_eq!(requests[3].args[0]["projectConfig"]["deviceTypes"], serde_json::json!(["tablet"]));
}

#[test]
fn structured_function_keys_preserve_nested_values_and_number_representations() {
    let host = Arc::new(ResultHost::default());
    let executor = SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>);
    let inputs = ["null", "false", "0", "0.0", "-0.0", "18446744073709551615", "\"0\"",
        "[0]", "[[0]]", "{\"a\":[false,null,0]}", "{\"a\":[false,null,0.0]}", "{\"b\":[false,null,0]}"];
    for input in inputs {
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        for _ in 0..2 { executor.call(&plugin(), "value", vec![value.clone()]).unwrap(); }
    }
    let requests = host.requests.lock().unwrap();
    assert_eq!(requests.len(), inputs.len());
    for (request, input) in requests.iter().zip(inputs) {
        assert_eq!(serde_json::to_string(&request.args[0]).unwrap(), input);
    }
}

#[test]
fn derived_config_is_shared_but_reconfigured_options_do_not_reuse_stale_values() {
    let options = tsrs_core::CompilerOptions { oh_device_types: Some(vec!["phone".into()]), ..Default::default() };
    let original = options.oh_sdk_plugin_config_cache.get(&options);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| assert!(Arc::ptr_eq(&original, &options.oh_sdk_plugin_config_cache.get(&options))));
        }
    });
    let mut updated = options.clone();
    updated.oh_device_types = Some(vec!["tablet".into()]);
    let next = updated.oh_sdk_plugin_config_cache.get(&updated);
    assert_eq!(original["deviceTypes"], serde_json::json!(["phone"]));
    assert_eq!(next["deviceTypes"], serde_json::json!(["tablet"]));
    assert!(!Arc::ptr_eq(&original, &next));
    let explicit = Arc::new(serde_json::json!({"custom": {"value": 42}}).as_object().unwrap().clone());
    updated.oh_sdk_plugin_project_config = Some(Arc::clone(&explicit));
    assert!(Arc::ptr_eq(&explicit, &updated.oh_sdk_plugin_config_cache.get(&updated)));
}

#[derive(Default)]
struct BatchHost {
    batches: Mutex<Vec<Vec<SdkPluginRequest>>>,
    descriptors: Mutex<std::collections::BTreeMap<u32, String>>,
    attempts: Mutex<std::collections::BTreeMap<(i64, String), usize>>,
}
impl SdkPluginHost for BatchHost {
    fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
        let mut descriptors = self.descriptors.lock().unwrap();
        // A host must register descriptors even for checks skipped below.
        for request in &requests {
            if let Some(name) = &request.class_name { descriptors.insert(request.plugin_id, name.clone()); }
        }
        let mut attempts = self.attempts.lock().unwrap();
        let mut responses: Vec<OhSdkPluginResponse> = Vec::new();
        for request in &requests {
            if request.after.is_some_and(|index| {
                let previous = &responses[index];
                previous.skipped || if previous.error.is_empty() { previous.found && !previous.check_result } else { previous.phase != "load" }
            }) {
                responses.push(OhSdkPluginResponse { skipped: true, ..Default::default() });
                continue;
            }
            let pos = request.args[0]["node"]["pos"].as_i64().unwrap();
            let name = &descriptors[&request.plugin_id];
            let attempt = attempts.entry((pos, name.clone())).or_default();
            *attempt += 1;
            let error = name == "Second" && pos >= 2 && *attempt == 1;
            responses.push(OhSdkPluginResponse {
                found: true, check_result: !(pos == 0 || pos == 1 && name == "Second"),
                check_message: format!("{pos}:{name}"),
                phase: if error && pos == 2 { "load".into() } else if error { "invoke".into() } else { String::new() },
                error: if error { "retry me".into() } else { String::new() },
                ..Default::default()
            });
        }
        self.batches.lock().unwrap().push(requests);
        Ok(responses)
    }
}

#[test]
fn syscap_batches_mix_local_hits_short_circuit_in_order_and_retry_only_uncached_results() {
    let host = Arc::new(BatchHost::default());
    let executor = SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>);
    let plugins: Vec<_> = ["First", "Second", "Third"].into_iter().map(|name| OhSdkClassCheckPlugin {
        path: "sdk/check.cjs".into(), class_name: name.into(), ..Default::default()
    }).collect();
    let config = Arc::default();
    let check = |positions: &[i32], file| executor.check_syscap_batch(&plugins, positions.iter().map(|&pos| (
        OhSdkPluginNodeSnapshot { pos, start: pos, ..snapshot(file) }, snapshot(10),
    )).collect(), Arc::clone(&config));
    let messages = |results: Vec<Result<Option<OhSdkPluginResponse>, String>>| results.into_iter()
        .map(|result| result.unwrap().unwrap().check_message).collect::<Vec<_>>();
    let first = check(&[0, 1, 2, 3, 2], 1);
    assert!(!first[0].as_ref().unwrap().as_ref().unwrap().check_result);
    assert_eq!(first[1].as_ref().unwrap().as_ref().unwrap().check_message, "1:Second");
    assert_eq!(first[2].as_ref().unwrap().as_ref().unwrap().check_message, "2:Third", "load errors do not stop later checkers");
    assert!(first[3].as_ref().unwrap_err().contains("invoke failed: retry me"));
    assert_eq!(first[4].as_ref().unwrap().as_ref().unwrap().check_message, "2:Third");
    assert_eq!(host.batches.lock().unwrap()[0].len(), 12, "duplicate node uses share one result inside the batch");
    assert_eq!(messages(check(&[3, 2, 1, 0], 1)), ["3:Third", "2:Third", "1:Second", "0:First"]);
    {
        let batches = host.batches.lock().unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].len(), 3, "only two errors and one previously skipped check reach JS");
        assert_eq!(batches[1].iter().map(|r| r.after).collect::<Vec<_>>(), [None, Some(0), None]);
        assert!(batches[1].iter().all(|r| r.path.is_none()), "descriptors registered on skipped entries are reusable");
    }
    assert_eq!(messages(check(&[2, 3, 0, 1], 1)), ["2:Third", "3:Third", "0:First", "1:Second"]);
    assert_eq!(host.batches.lock().unwrap().len(), 2, "all hits stay local");
    check(&[2], 2);
    assert_eq!(host.batches.lock().unwrap()[2].len(), 3, "a new source version misses all class results");
}

#[test]
fn overlapping_file_batches_coalesce_without_partial_reservation_deadlocks() {
    struct Host(Mutex<Vec<Vec<SdkPluginRequest>>>);
    impl SdkPluginHost for Host {
        fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
            let responses = requests.iter().map(|r| OhSdkPluginResponse {
                found: true, check_result: true, check_message: r.args[0]["node"]["pos"].to_string(), ..Default::default()
            }).collect();
            self.0.lock().unwrap().push(requests);
            Ok(responses)
        }
    }
    let host = Arc::new(Host(Mutex::default()));
    let executor = SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>);
    let plugins = [OhSdkClassCheckPlugin { path: "sdk/check.cjs".into(), class_name: "Checker".into(), ..Default::default() }];
    let barrier = Barrier::new(8);
    let config = Arc::default();
    std::thread::scope(|scope| {
        for i in 0..8 {
            let (barrier, executor, plugins, config) = (&barrier, &executor, &plugins, &config);
            scope.spawn(move || {
                let positions = if i % 2 == 0 { [0, 1] } else { [1, 0] };
                barrier.wait();
                let results = executor.check_syscap_batch(plugins, positions.iter().map(|&pos| (
                    OhSdkPluginNodeSnapshot { pos, start: pos, ..snapshot(1) }, snapshot(2),
                )).collect(), Arc::clone(config));
                for (result, pos) in results.into_iter().zip(positions) { assert_eq!(result.unwrap().unwrap().check_message, pos.to_string()); }
            });
        }
    });
    let batches = host.0.lock().unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 2);
}

#[test]
fn per_file_batches_preserve_sorted_diagnostic_bytes_and_can_i_use_suppression() {
    use common::TempDir;
    use tsrs_api::{SessionOptions, embedded::Compiler};
    use tsrs_core::{CompilerOptions, Tristate};
    use tsrs_vfs::{FS, bundled, osvfs};
    #[derive(Default)]
    struct Host(Mutex<Vec<Vec<SdkPluginRequest>>>);
    impl SdkPluginHost for Host {
        fn invoke_batch(&self, requests: Vec<SdkPluginRequest>) -> Result<Vec<OhSdkPluginResponse>, String> {
            let responses = requests.iter().map(|r| OhSdkPluginResponse {
                found: true, check_result: true, check_message: if r.operation == "syscap" {
                    format!("SDK warning at {}", r.args[0]["node"]["start"])
                } else { String::new() }, ..Default::default()
            }).collect();
            self.0.lock().unwrap().push(requests);
            Ok(responses)
        }
    }
    let dir = TempDir::new("sdk-file-batches");
    let sdk = dir.write("api.d.ets", "/** @syscap base */\ndeclare function sdkCall(): void;\ndeclare function canIUse(name: string): boolean;\n");
    let source = "sdkCall();\nif (canIUse('base')) { sdkCall(); }\nsdkCall();\n";
    let apps = [dir.write("a.ets", source), dir.write("b.ets", source)];
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = Compiler::new(SessionOptions::new(dir.dir(), bundled::lib_path(), Arc::clone(&fs), false)).unwrap();
    let mut expected = String::new();
    for app in &apps {
        expected.push_str(&format!("{app}:0:7:SDK warning at 0\n"));
        // The untagged SDK canIUse declaration produces the existing immediate
        // missing-syscap warning, between the two deferred plugin warnings.
        let pos = source.find("canIUse").unwrap();
        expected.push_str(&format!("{app}:{pos}:{}:The system capacity of this api 'canIUse' is not supported on all devices\n", pos + "canIUse".len()));
        let pos = source.rfind("sdkCall").unwrap();
        expected.push_str(&format!("{app}:{pos}:{}:SDK warning at {pos}\n", pos + "sdkCall".len()));
    }
    for strict in [Tristate::False, Tristate::True] {
        let host = Arc::new(Host::default());
        let options = CompilerOptions {
            no_emit: Tristate::True, skip_lib_check: Tristate::True, strict, checkers: Some(2),
            ets_loader_path: dir.path("ets-loader"), oh_compile_mode: "moduleJson".into(),
            oh_device_types: Some(vec!["phone".into()]), oh_syscap_intersection: Some(vec!["base".into()]),
            oh_all_module_paths: Some(vec![sdk.clone()]),
            oh_sdk_class_check_plugins: Some(vec![OhSdkClassCheckPlugin {
                tag_name: "syscap".into(), path: "sdk/check.cjs".into(), class_name: "Checker".into(),
            }]),
            oh_sdk_plugin_executor: Some(Arc::new(SdkPluginExecutor::new(Arc::clone(&host) as Arc<dyn SdkPluginHost>))),
            ..Default::default()
        };
        let program = service.create_program(&[sdk.clone(), apps[0].clone(), apps[1].clone()], options, Arc::clone(&fs)).unwrap();
        let diagnostic_bytes = || program.semantic_diagnostics().unwrap().into_iter().map(|d| {
            assert_eq!(d.category, tsrs_diagnostics::Category::Warning as u8);
            format!("{}:{}:{}:{}\n", d.file_name.unwrap().display(), d.pos, d.end, d.text)
        }).collect::<String>();
        assert_eq!(diagnostic_bytes(), expected);
        assert_eq!(diagnostic_bytes(), expected, "a repeated query keeps the same diagnostics");
        let batches = host.0.lock().unwrap();
        let syscap: Vec<_> = batches.iter().filter(|batch| batch[0].operation == "syscap").collect();
        assert_eq!(syscap.len(), 2, "one host call for each application file");
        for batch in syscap {
            assert_eq!(batch.len(), 3, "suppression happens after the plugin, as in checkSyscapAbility");
            assert!(batch.iter().all(|r| r.args[0]["node"]["fileName"] == batch[0].args[0]["node"]["fileName"]));
            let starts: Vec<_> = batch.iter().map(|r| r.args[0]["node"]["start"].as_u64().unwrap()).collect();
            assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "callbacks retain discovery order");
        }
    }
}

#[test]
fn embedded_syscap_pipeline_overlaps_two_files_then_drains_before_directive_filtering() {
    use common::TempDir;
    use tsrs_api::{SessionOptions, embedded::Compiler};
    use tsrs_core::{CompilerOptions, Tristate};
    use tsrs_vfs::{FS, bundled, osvfs};
    let dir = TempDir::new("sdk-pipeline");
    let sdk = dir.write("api.d.ets", "/** @syscap base */\ndeclare function sdkCall(): void;\ndeclare function canIUse(name: string): boolean;\n");
    let source = "export {};\nsdkCall();\n// @SuppressWarnings syscap\nsdkCall();\nconst cap = 'base';\nif (canIUse(cap)) { sdkCall(); }\n// @ts-ignore\nsdkCall();\n// @ts-expect-error\nsdkCall();\nsdkCall();\n";
    let apps: Vec<_> = (0..7)
        .map(|i| dir.write(&format!("app{i}.ets"), source))
        .collect();
    let mut files = vec![sdk.clone()];
    files.extend(apps);
    let run = |host: Arc<dyn SdkPluginHost>, pipelined| {
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
        let compiler = Compiler::new(SessionOptions::new(
            dir.dir(),
            bundled::lib_path(),
            Arc::clone(&fs),
            false,
        ))
        .unwrap();
        let options = CompilerOptions {
            no_emit: Tristate::True,
            skip_lib_check: Tristate::True,
            checkers: Some(1),
            ets_loader_path: dir.path("ets-loader"),
            oh_compile_mode: "moduleJson".into(),
            oh_device_types: Some(vec!["phone".into()]),
            oh_syscap_intersection: Some(vec!["base".into()]),
            oh_all_module_paths: Some(vec![sdk.clone()]),
            oh_sdk_class_check_plugins: Some(vec![OhSdkClassCheckPlugin {
                tag_name: "syscap".into(),
                path: "sdk/check.cjs".into(),
                class_name: "Checker".into(),
            }]),
            oh_sdk_plugin_executor: Some(Arc::new(SdkPluginExecutor::new(host))),
            ..Default::default()
        };
        let program = compiler.create_program(&files, options, fs).unwrap();
        if pipelined {
            let normal = program.arkts_normal_diagnostics().unwrap();
            // Retires the normal checker regions: no reply may outlive this boundary.
            program
                .arkts_strict_diagnostics(normal)
                .unwrap()
                .semantic_diagnostics
        } else {
            program.semantic_diagnostics().unwrap()
        }
    };
    struct ImmediateHost;
    impl SdkPluginHost for ImmediateHost {
        fn invoke_batch(
            &self,
            requests: Vec<SdkPluginRequest>,
        ) -> Result<Vec<OhSdkPluginResponse>, String> {
            Ok(QueuedHost::responses(&requests))
        }
    }
    let baseline = run(Arc::new(ImmediateHost), false);
    assert!(!baseline.is_empty());
    assert!(
        !baseline.iter().any(|d| d.code == 2578),
        "SDK warnings consume @ts-expect-error"
    );
    let (send, receive_async) = mpsc::channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| run(Arc::new(QueuedHost(send)), true));
        let first = receive_async.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = receive_async.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            receive_async
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "a checker cannot enqueue a third unfinished batch"
        );
        let (requests, complete) = second;
        complete(Ok(QueuedHost::responses(&requests)));
        let (requests, complete) = first;
        complete(Ok(QueuedHost::responses(&requests)));
        for _ in 2..7 {
            let (requests, complete) = receive_async.recv_timeout(Duration::from_secs(10)).unwrap();
            complete(Ok(QueuedHost::responses(&requests)));
        }
        assert_eq!(
            worker.join().unwrap(),
            baseline,
            "same ordered diagnostics, messages, spans and suppression as synchronous file checks"
        );
    });
    assert!(receive_async.try_recv().is_err());
}
