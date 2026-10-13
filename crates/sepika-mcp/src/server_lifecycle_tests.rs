use super::super::persist::SavePoint;
use sepika_io::results::ResultKind;
use std::future::Future;
use std::task::Poll;

struct SaveGate {
    entered: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    released: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}

impl SaveGate {
    fn new() -> (Arc<Self>, tokio::sync::oneshot::Receiver<()>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                entered: std::sync::Mutex::new(Some(tx)),
                released: std::sync::Mutex::new(false),
                wake: std::sync::Condvar::new(),
            }),
            rx,
        )
    }

    fn block(&self) {
        if let Some(tx) = self.entered.lock().unwrap().take() {
            let _ = tx.send(());
        }
        let guard = self.released.lock().unwrap();
        drop(self.wake.wait_while(guard, |released| !*released).unwrap());
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

struct ReleaseGate(Arc<SaveGate>);
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn zero_requested_eigen_modes_remain_known_after_publication_and_reopen() {
    let dir = test_store_dir("provenance-zero-modes");
    let model = cantilever_with_load_case();
    let server = SepikaServer::new(make_state(model.clone(), &dir));
    let mut args = run_args(JobKind::Eigen);
    args.n_modes = Some(0);
    let run = server.analysis_run(Parameters(args)).await.unwrap();
    let id = extract_job_id(&run);
    let summary = done_summary(&wait_for_terminal(&server, &id).await);
    assert_eq!(summary["n_modes"], 0);
    assert_eq!(summary["store"]["persisted"], true);
    assert_eq!(summary["store"]["entries"][0]["rows"], 0);
    assert_eq!(summary["store"]["entries"][0]["provenance"]["conditions"]["n_modes"], 0);

    for reopened in [false, true] {
        let reopened_server = SepikaServer::new(make_state(model.clone(), &dir));
        let selected = if reopened { &reopened_server } else { &server };
        let got = selected.result_get(Parameters(ResultGetArgs {
            case: 0, kind: "Modal".into(), node_ids: None, member_ids: None, step_range: None,
        })).await.unwrap();
        let value = response_json(&got);
        assert_eq!(value["status"], "current");
        assert_eq!(value["rows"], serde_json::json!([]));
        assert_eq!(value["truncated"], false);
        assert_eq!(value["entry"], summary["store"]["entries"][0]);
        assert_eq!(value["current_scope"], "model_input_only");
    }

    let original = std::fs::read(dir.join("manifest.json")).unwrap();
    let entry_path = summary["store"]["entries"][0]["path"].as_str().unwrap();
    let original_bytes = std::fs::read(entry_path).unwrap();
    for condition in [None, Some(serde_json::Value::Null), Some(serde_json::json!("unknown"))] {
        let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let conditions = manifest["entries"][0]["provenance"]["conditions"].as_object_mut().unwrap();
        match condition {
            Some(value) => { conditions.insert("n_modes".into(), value); },
            None => { conditions.remove("n_modes"); },
        }
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let unknown = SepikaServer::new(make_state(model.clone(), &dir));
        let got = unknown.result_get(Parameters(ResultGetArgs {
            case: 0, kind: "Modal".into(), node_ids: None, member_ids: None, step_range: None,
        })).await.unwrap();
        let value = response_json(&got);
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["reason"], "unknown_provenance");
        assert!(value.get("rows").is_none());
        assert_eq!(std::fs::read(entry_path).unwrap(), original_bytes);
    }
}

fn response_json(result: &CallToolResult) -> serde_json::Value {
    serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap()
}

async fn public_status(server: &SepikaServer, id: &str) -> serde_json::Value {
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        server.analysis_status(Parameters(AnalysisStatusArgs { job_id: id.into() })),
    )
    .await
    .expect("保存中statusの応答期限")
    .unwrap();
    response_json(&result)
}

async fn saving_tools_contract(name: &str) {
    for point in [
        SavePoint::Finish(1, ResultKind::NodalDisp),
        SavePoint::Finish(1, ResultKind::MemberForce),
        SavePoint::Sync,
    ] {
        let dir = test_store_dir(&format!("{name}-{point:?}"));
        let mut server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let (gate, entered) = SaveGate::new();
        let release = ReleaseGate(gate.clone());
        server.save_hook = Some(Arc::new(move |actual| {
            if actual == point {
                gate.block();
            }
            Ok(())
        }));
        let run = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let id = extract_job_id(&run);
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        let status = public_status(&server, &id).await;
        assert_eq!(status["id"], id);
        assert_eq!(status["kind"], "LinearStatic");
        assert_eq!(
            status["status"],
            serde_json::json!({"Running": {"progress": null}})
        );

        let mut edit = Box::pin(server.model_edit(Parameters(EditArgs {
            body: serde_json::json!({"command": "SetStbStrengths", "input": cantilever_with_load_case().stb_strengths}),
        })));
        let mut get = Box::pin(server.result_get(Parameters(ResultGetArgs {
            case: 1,
            kind: "NodalDisp".into(),
            node_ids: None,
            member_ids: None,
            step_range: None,
        })));
        assert!(std::future::poll_fn(|cx| Poll::Ready(edit.as_mut().poll(cx).is_pending())).await);
        assert!(std::future::poll_fn(|cx| Poll::Ready(get.as_mut().poll(cx).is_pending())).await);
        drop(release);
        assert!(tokio::time::timeout(Duration::from_secs(5), edit)
            .await
            .unwrap()
            .is_ok());
        let result = tokio::time::timeout(Duration::from_secs(5), get)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response_json(&result)["rows"].as_array().unwrap().len(), 2);
        let summary = done_summary(&wait_for_terminal(&server, &id).await);
        assert_eq!(
            summary["store"]["kinds"],
            serde_json::json!(["NodalDisp", "MemberForce"])
        );
        assert_eq!(
            sepika_io::results::FsResultStore::open(&dir)
                .unwrap()
                .manifest()
                .entries
                .len(),
            2
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn saving_current_thread_status_and_state_exclusion() {
    saving_tools_contract("save-current").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saving_multithread_status_and_state_exclusion() {
    saving_tools_contract("save-multi").await;
}

#[tokio::test]
async fn saving_failure_and_panic_discard_pending_before_next_job() {
    for (point, panic) in [
        (SavePoint::Start(1, ResultKind::NodalDisp), false),
        (SavePoint::Write(1, ResultKind::MemberForce), false),
        (SavePoint::Finish(1, ResultKind::MemberForce), false),
        (SavePoint::Sync, false),
        (SavePoint::Sync, true),
    ] {
        let dir = test_store_dir(&format!("save-fail-{point:?}-{panic}"));
        let mut server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let previous_run = server.analysis_run(Parameters(run_args(JobKind::LinearStatic))).await.unwrap();
        let previous_id = extract_job_id(&previous_run);
        done_summary(&wait_for_terminal(&server, &previous_id).await);
        let previous_manifest = std::fs::read(dir.join("manifest.json")).unwrap();
        let previous_files: Vec<_> = server.state.lock().await.results.manifest().entries.iter()
            .map(|entry| (entry.path.clone(), std::fs::read(&entry.path).unwrap())).collect();
        server.state.lock().await.model.load_cases[0].nodal[0].values[2] *= 2.0;
        server.save_hook = Some(Arc::new(move |actual| {
            if actual == point {
                assert!(!panic, "保存worker異常終了の注入");
                return Err(std::io::Error::other(format!("保存失敗の注入: {actual:?}")));
            }
            Ok(())
        }));
        let run = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let id = extract_job_id(&run);
        let status = wait_for_terminal(&server, &id).await;
        assert!(
            matches!(status, JobStatus::Failed { ref kind, .. } if kind == "internal"),
            "{status:?}"
        );
        assert_eq!(std::fs::read(dir.join("manifest.json")).unwrap(), previous_manifest);
        for (path, bytes) in &previous_files { assert_eq!(std::fs::read(path).unwrap(), *bytes); }
        assert_eq!(server.state.lock().await.results.manifest().entries.len(), 2);
        assert!(public_status(&server, &id).await["status"]
            .get("Failed")
            .is_some());

        server.save_hook = None;
        let run = server
            .analysis_run(Parameters(run_args(JobKind::Eigen)))
            .await
            .unwrap();
        let next_id = extract_job_id(&run);
        assert_ne!(id, next_id);
        let summary = done_summary(&wait_for_terminal(&server, &next_id).await);
        assert_eq!(summary["store"]["kinds"], serde_json::json!(["Modal"]));
        let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
        assert_eq!(reopened.manifest().entries.len(), 3);
        assert_eq!(reopened.manifest().entries[2].kind, ResultKind::Modal);
    }
}

#[tokio::test]
async fn saving_real_writer_and_manifest_failures_are_failed() {
    for manifest in [false, true] {
        let dir = test_store_dir(&format!("save-real-fail-{manifest}"));
        let server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        if manifest {
            std::fs::create_dir_all(dir.join("manifest.json")).unwrap();
        } else {
            std::fs::remove_dir(&dir).unwrap();
            std::fs::write(&dir, "writer開始失敗").unwrap();
        }
        let run = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let id = extract_job_id(&run);
        let status = wait_for_terminal(&server, &id).await;
        assert!(
            matches!(status, JobStatus::Failed { ref kind, .. } if kind == "internal"),
            "{status:?}"
        );
        assert!(server
            .state
            .lock()
            .await
            .results
            .manifest()
            .entries
            .is_empty());
        let mut st = server.state.lock().await;
        st.results.sync().unwrap();
        assert!(st.results.manifest().entries.is_empty());
    }
}

#[tokio::test]
async fn saving_concurrent_jobs_keep_their_ids_summaries_and_exclusive_store() {
    for (second_kind, second_case) in [
        (JobKind::LinearStatic, 1),
        (JobKind::LinearStatic, 2),
        (JobKind::Eigen, 0),
    ] {
        let dir = test_store_dir(&format!("save-concurrent-{second_kind:?}-{second_case}"));
        let mut model = cantilever_with_load_case();
        let mut case2 = model.load_cases[0].clone();
        case2.id = LoadCaseId(2);
        case2.name = "case2".into();
        model.load_cases.push(case2);
        let mut server = SepikaServer::new(make_state(model, &dir));
        let (gate, entered) = SaveGate::new();
        let release = ReleaseGate(gate.clone());
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let save_active = active.clone();
        server.before_save = Some(Arc::new(move || {
            assert_eq!(
                save_active.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                0
            );
            gate.block();
        }));
        let save_active = active.clone();
        server.after_save = Some(Arc::new(move || {
            assert_eq!(
                save_active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst),
                1
            );
        }));
        let (compute_gate, _) = SaveGate::new();
        let compute_release = ReleaseGate(compute_gate.clone());
        server.before_compute = Some(Arc::new(move || compute_gate.block()));
        let a = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let mut second_args = run_args(second_kind);
        if second_kind == JobKind::LinearStatic {
            second_args.load_case = Some(second_case);
        }
        let b = server.analysis_run(Parameters(second_args)).await.unwrap();
        drop(compute_release);
        let a = extract_job_id(&a);
        let b = extract_job_id(&b);
        assert_ne!(a, b);
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        assert!(public_status(&server, &a).await["status"]
            .get("Running")
            .is_some());
        assert!(public_status(&server, &b).await["status"]
            .get("Running")
            .is_some());
        drop(release);
        let summary_a = done_summary(&wait_for_terminal(&server, &a).await);
        let summary_b = done_summary(&wait_for_terminal(&server, &b).await);
        assert_eq!(public_status(&server, &a).await["kind"], "LinearStatic");
        assert_eq!(
            public_status(&server, &b).await["kind"],
            serde_json::to_value(second_kind).unwrap()
        );
        assert_eq!(
            summary_a["store"]["kinds"],
            serde_json::json!(["NodalDisp", "MemberForce"])
        );
        let expected = if second_kind == JobKind::Eigen {
            serde_json::json!(["Modal"])
        } else {
            serde_json::json!(["NodalDisp", "MemberForce"])
        };
        assert_eq!(summary_b["store"]["kinds"], expected);
        assert_eq!(summary_a["store"]["case"], 1);
        assert_eq!(summary_b["store"]["case"], second_case);
    }
}

#[tokio::test]
async fn saving_computed_snapshot_survives_live_model_edit() {
    let dir = test_store_dir("save-snapshot");
    let model = cantilever_with_load_case();
    let expected = compute_job(&model, JobKind::LinearStatic, &JobParams::default()).unwrap();
    let expected_dir = test_store_dir("save-snapshot-expected");
    let mut expected_store = sepika_io::results::FsResultStore::open(&expected_dir).unwrap();
    let source = JobParams::default().result_provenance(&model, JobKind::LinearStatic, "expected").unwrap();
    let expected_summary: serde_json::Value =
        serde_json::from_str(&persist_job_outcome(&mut expected_store, expected, &source).unwrap()).unwrap();
    let expected_rows = result_get_json(&expected_store, &model, 1, "NodalDisp", None, None, None).unwrap();
    let mut server = SepikaServer::new(make_state(model, &dir));
    let (gate, entered) = SaveGate::new();
    let release = ReleaseGate(gate.clone());
    server.before_compute = Some(Arc::new(move || gate.block()));
    let run = server
        .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
        .await
        .unwrap();
    let id = extract_job_id(&run);
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    {
        let mut st = server.state.lock().await;
        st.model.nodes[1].coord[0] = 2000.0;
        st.model.load_cases.clear();
    }
    drop(release);
    let summary = done_summary(&wait_for_terminal(&server, &id).await);
    let normalize = |mut value: serde_json::Value| {
        for entry in value["store"]["entries"].as_array_mut().unwrap() {
            entry["path"] = "公開path".into();
            entry["provenance"]["run"] = "実行識別".into();
        }
        value
    };
    assert_eq!(normalize(summary), normalize(expected_summary));
    let st = server.state.lock().await;
    assert_eq!(st.model.nodes[1].coord[0], 2000.0);
    assert!(st.model.load_cases.is_empty());
    assert_eq!(
        result_get_json(&st.results, &st.model, 1, "NodalDisp", None, None, None).unwrap()["rows"],
        expected_rows["rows"]
    );
    let stale = result_get_json(&st.results, &st.model, 1, "NodalDisp", Some(vec![1]), None, None).unwrap();
    assert_eq!(stale["status"], "stale");
    assert_eq!(stale["entry"]["provenance"]["input_sha256"], source.input_sha256);
    assert_eq!(stale["rows"].as_array().unwrap().len(), 1);
    let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
    assert_eq!(result_get_json(&reopened, &st.model, 1, "NodalDisp", None, None, None).unwrap()["status"], "stale");
}

#[tokio::test]
async fn result_public_api_keeps_old_snapshot_and_distinguishes_restarted_job_ids() {
    let dir = test_store_dir("provenance-restart");
    let model = cantilever_with_load_case();
    let server = SepikaServer::new(make_state(model.clone(), &dir));
    let run = server.analysis_run(Parameters(run_args(JobKind::LinearStatic))).await.unwrap();
    let old_id = extract_job_id(&run);
    let old_summary = done_summary(&wait_for_terminal(&server, &old_id).await);
    let old_entry = old_summary["store"]["entries"][0].clone();
    let old_bytes = std::fs::read(old_entry["path"].as_str().unwrap()).unwrap();
    let mut edited = model;
    edited.nodes[1].coord[0] = 2000.0;
    let restarted = SepikaServer::new(make_state(edited, &dir));
    let got = restarted.result_get(Parameters(ResultGetArgs {
        case: 1, kind: "NodalDisp".into(), node_ids: Some(vec![1]), member_ids: None, step_range: None,
    })).await.unwrap();
    assert_eq!(response_json(&got)["status"], "stale");
    assert_eq!(response_json(&got)["entry"], old_entry);
    let run = restarted.analysis_run(Parameters(run_args(JobKind::Eigen))).await.unwrap();
    let new_id = extract_job_id(&run);
    assert_eq!(new_id, old_id);
    let new_summary = done_summary(&wait_for_terminal(&restarted, &new_id).await);
    assert_ne!(new_summary["store"]["entries"][0]["provenance"]["run"], old_entry["provenance"]["run"]);
    assert_eq!(std::fs::read(old_entry["path"].as_str().unwrap()).unwrap(), old_bytes);
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    manifest["entries"][0].as_object_mut().unwrap().remove("provenance");
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
    let unknown = SepikaServer::new(make_state(restarted.state.lock().await.model.clone(), &dir));
    let got = unknown.result_get(Parameters(ResultGetArgs {
        case: 1, kind: "NodalDisp".into(), node_ids: None, member_ids: None, step_range: None,
    })).await.unwrap();
    let result = response_json(&got);
    assert_eq!(result["status"], "unavailable");
    assert_eq!(result["reason"], "unknown_provenance");
    assert_eq!(result["action"], "analysis_run");
    assert!(result.get("rows").is_none());
}

#[tokio::test]
async fn saving_compute_worker_panic_reaches_failed() {
    let dir = test_store_dir("compute-worker-panic");
    let mut server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
    server.before_compute = Some(Arc::new(|| panic!("計算worker異常終了の注入")));
    let run = server
        .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
        .await
        .unwrap();
    let id = extract_job_id(&run);
    let status = wait_for_terminal(&server, &id).await;
    assert!(
        matches!(status, JobStatus::Failed { ref kind, .. } if kind == "internal"),
        "{status:?}"
    );
    assert!(public_status(&server, &id).await["status"]
        .get("Failed")
        .is_some());
    assert!(server
        .state
        .lock()
        .await
        .results
        .manifest()
        .entries
        .is_empty());
}
