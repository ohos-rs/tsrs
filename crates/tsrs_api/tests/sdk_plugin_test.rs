use std::collections::BTreeSet;
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;
use tsrs_api::embedded::{SdkPluginExecutor, SdkPluginHost, SdkPluginRequest};
use tsrs_core::{
    OhSdkClassCheckPlugin, OhSdkPluginExecutor, OhSdkPluginNodeSnapshot, OhSdkPluginResponse,
};

struct RegistrationHost {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    registered: Mutex<BTreeSet<(u64, String, u64)>>,
    requests: Mutex<Vec<SdkPluginRequest>>,
}

impl SdkPluginHost for RegistrationHost {
    fn invoke_batch(
        &self,
        requests: Vec<SdkPluginRequest>,
    ) -> Result<Vec<OhSdkPluginResponse>, String> {
        let request = &requests[0];
        let payload = &request.args[0];
        // Delay the first registration after dispatch has begun. A competing
        // checker may already have interned the same source/config IDs.
        if payload.get("projectConfig").is_some() {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        let mut registered = self.registered.lock().unwrap();
        for (kind, id, full) in [
            (
                "project",
                payload["projectConfigId"].as_u64().unwrap(),
                payload.get("projectConfig").is_some(),
            ),
            (
                "source",
                payload["node"]["sourceId"].as_u64().unwrap(),
                payload["node"].get("source").is_some(),
            ),
            (
                "source",
                payload["declaration"]["sourceId"].as_u64().unwrap(),
                payload["declaration"].get("source").is_some(),
            ),
        ] {
            let key = (request.session_id, kind.to_string(), id);
            if full {
                registered.insert(key);
            } else if !registered.contains(&key) {
                return Err(format!("{kind} {id} referenced before registration"));
            }
        }
        self.requests.lock().unwrap().extend(requests);
        Ok(vec![OhSdkPluginResponse {
            found: true,
            check_result: true,
            check_message: "SDK check executed".into(),
            ..Default::default()
        }])
    }
}

#[test]
fn concurrent_sdk_checks_publish_snapshots_before_referencing_interned_ids() {
    let (entered, registration) = mpsc::channel();
    let (release, pending) = mpsc::channel();
    let host = Arc::new(RegistrationHost {
        entered,
        release: Mutex::new(pending),
        registered: Mutex::default(),
        requests: Mutex::default(),
    });
    let sdk_host = Arc::clone(&host);
    let executor = SdkPluginExecutor::new(sdk_host);
    let plugin = OhSdkClassCheckPlugin {
        path: "sdk/SyscapChecker.js".into(),
        class_name: "SyscapChecker".into(),
        ..Default::default()
    };
    let node = OhSdkPluginNodeSnapshot {
        file_name: "input.ets".into(),
        source: "current".into(),
        pos: 0,
        end: 7,
        text: "current".into(),
    };
    let declaration = OhSdkPluginNodeSnapshot {
        file_name: "sdk/api.d.ets".into(),
        source: "declare current".into(),
        pos: 0,
        end: 15,
        text: "declare current".into(),
    };
    let config = serde_json::json!({"etsLoaderPath": "sdk/ets-loader"})
        .as_object()
        .unwrap()
        .clone();
    let competing = Barrier::new(2);
    std::thread::scope(|scope| {
        let check =
            || executor.check_syscap(&plugin, node.clone(), declaration.clone(), config.clone());
        let first = scope.spawn(check);
        registration.recv_timeout(Duration::from_secs(5)).unwrap();
        let barrier = &competing;
        let second = scope.spawn(move || {
            barrier.wait();
            check()
        });
        competing.wait();
        // Let the competing checker attempt dispatch while registration is
        // deliberately blocked; then permit the first callback to complete.
        std::thread::sleep(Duration::from_millis(100));
        release.send(()).unwrap();
        for result in [first.join().unwrap(), second.join().unwrap()] {
            let response = result.unwrap();
            assert!(response.check_result);
            assert_eq!(response.check_message, "SDK check executed");
        }
    });
    let requests = host.requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "both SDK checks must execute");
    assert!(requests[0].args[0].get("projectConfig").is_some());
    assert!(requests[1].args[0].get("projectConfig").is_none());
    assert!(requests[1].args[0]["node"].get("source").is_none());
    assert!(requests[1].args[0]["declaration"].get("source").is_none());
}
