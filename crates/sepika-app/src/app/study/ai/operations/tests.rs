use super::*;

fn fixture() -> (App, usize) {
    let mut app = crate::app::study::tests::app();
    let s = app
        .create_study("AI接続の検証".into(), vec![ElemId(2)])
        .unwrap();
    (app, s)
}
fn request(app: &App, s: usize, p: usize, operation: &str, arguments: Value) -> Request {
    let permission = app.ai_connection.permission.as_ref().unwrap();
    Request {
        version: VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        operation: operation.into(),
        session: Some(permission.session.clone()),
        project: Some(permission.project.clone()),
        study: Some(permission.study.clone()),
        request: Some("task-1".into()),
        proposal: Some(app.ai_proposal_reference(s, p).proposal),
        input: Some(json!(app.ai_proposal_reference(s, p))),
        arguments,
    }
}
fn permit(app: &mut App, s: usize, mode: Mode) {
    app.ai_connection.pending = Some("client".into());
    app.approve_ai_connection(s, mode, true).unwrap();
}
fn call(app: &mut App, s: usize, p: usize, operation: &str, arguments: Value) -> Response {
    let request = request(app, s, p, operation, arguments);
    app.handle_ai_request("client", request)
}
fn accepted(response: Response) -> Value {
    assert!(response.error.is_none(), "{:?}", response.error);
    response.result.unwrap()
}

#[test]
fn worker_errors_and_disconnections_remain_failed_with_their_reason_after_reload() {
    for disconnected in [false, true] {
        let (mut app, s) = fixture();
        let p = app.create_comparison(s, "実行失敗".into()).unwrap();
        permit(&mut app, s, Mode::Comparison);
        accepted(call(&mut app, s, p, "begin_request", Value::Null));
        accepted(call(&mut app, s, p, "calculate", Value::Null));
        let input = app.ai_proposal_reference(s, p);
        let (tx, rx) = std::sync::mpsc::channel();
        app.studies.running.as_mut().unwrap().preparing = Some(rx);
        if !disconnected {
            tx.send(Err("fixtureの計算worker失敗".into())).unwrap();
        }
        drop(tx);
        app.studies.dirty = false;
        assert!(app.poll_study_calculation());
        assert!(app.studies.running.is_none());
        assert!(app.studies.items[s].runs.is_empty());
        assert_eq!(app.ai_calculation_state(s, &input), "failed");
        let error = app.studies.items[s].ai.jobs[0].error.clone().unwrap();
        assert!(error.contains(if disconnected {
            "異常終了"
        } else {
            "fixtureの計算worker失敗"
        }));
        assert!(app.studies.dirty);
        let saved = app.saved_studies();
        app.restore_studies(saved).unwrap();
        assert_eq!(app.ai_calculation_state(s, &input), "failed");
        assert_eq!(
            app.studies.items[s].ai.jobs[0].error.as_deref(),
            Some(error.as_str())
        );
        assert!(app.studies.items[s].ai.jobs[0].run_index.is_none());
    }
}

#[test]
fn completed_calculation_is_saved_as_calculated_without_an_intervening_ai_poll() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "即時保存".into()).unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "begin_request", Value::Null));
    accepted(call(&mut app, s, p, "calculate", Value::Null));
    let input = app.ai_proposal_reference(s, p);
    crate::app::study::tests::wait(&mut app);
    assert_eq!(app.studies.items[s].ai.jobs[0].state, "calculated");
    assert_eq!(app.studies.items[s].ai.jobs[0].run_index, Some(0));
    let path = std::env::temp_dir().join(format!(
        "sepika-ai-immediate-save-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    assert_eq!(reopened.ai_calculation_state(s, &input), "calculated");
    assert_eq!(reopened.studies.items[s].ai.jobs[0].run_index, Some(0));
    std::fs::remove_file(path).unwrap();
    let saved = app.saved_studies();
    assert_eq!(saved.items[s].ai.jobs[0].state, "calculated");
    app.restore_studies(saved).unwrap();
    assert_eq!(app.ai_calculation_state(s, &input), "calculated");
    assert_eq!(app.studies.items[s].ai.jobs[0].run_index, Some(0));
    let mut raced = app.saved_studies();
    raced.items[s].ai.jobs[0].state = "running".into();
    raced.items[s].ai.jobs[0].run_index = None;
    app.restore_studies(raced).unwrap();
    assert_eq!(app.ai_calculation_state(s, &input), "calculated");
    let mut raced = app.saved_studies();
    raced.items[s].ai.jobs[0].state = "interrupted_no_result".into();
    raced.items[s].ai.jobs[0].run_index = None;
    app.restore_studies(raced).unwrap();
    assert_eq!(app.ai_calculation_state(s, &input), "calculated");
    app.studies.items[s].ai.jobs[0].state = "running".into();
    app.studies.items[s].ai.jobs[0].run_index = None;
    assert_eq!(app.saved_studies().items[s].ai.jobs[0].state, "calculated");
}

#[test]
fn unfinished_calculation_reloads_as_interrupted_and_sync_changes_are_dirty_only_once() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "未完了".into()).unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "begin_request", Value::Null));
    accepted(call(&mut app, s, p, "calculate", Value::Null));
    let input = app.ai_proposal_reference(s, p);
    let saved = app.saved_studies();
    assert!(saved.items[s].runs.is_empty());
    app.restore_studies(saved).unwrap();
    assert_eq!(app.ai_calculation_state(s, &input), "interrupted_no_result");
    assert!(app.studies.items[s].ai.jobs[0].run_index.is_none());
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "calculate", Value::Null));
    crate::app::study::tests::wait(&mut app);
    assert_eq!(
        app.studies.items[s].ai.jobs[0].state,
        "interrupted_no_result"
    );
    assert!(app.studies.items[s].ai.jobs[0].run_index.is_none());
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    assert_eq!(
        app.studies.items[s].ai.jobs[0].state,
        "interrupted_no_result"
    );
    assert_eq!(app.studies.items[s].ai.jobs[1].state, "calculated");
    let last = app.studies.items[s].ai.jobs.len() - 1;
    app.studies.items[s].ai.jobs[last].state = "running".into();
    app.studies.items[s].ai.jobs[last].run_index = None;
    app.studies.dirty = false;
    app.core.scoped.staleness.unsaved_changes = false;
    assert!(!app.project_unsaved());
    let main = app.input_reference();
    app.sync_ai_jobs();
    assert!(app.project_unsaved());
    assert!(app.studies.dirty);
    assert_eq!(app.studies.items[s].ai.jobs[last].state, "calculated");
    assert_eq!(app.input_reference(), main);
    app.studies.dirty = false;
    app.core.scoped.staleness.unsaved_changes = false;
    app.sync_ai_jobs();
    assert!(!app.project_unsaved());
}

#[test]
fn only_the_calculating_proposal_is_locked_and_other_proposal_edits_do_not_change_its_input() {
    let (mut app, s) = fixture();
    let running = app.create_comparison(s, "計算中".into()).unwrap();
    let other = app.create_comparison(s, "別案".into()).unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let started = app.ai_proposal_reference(s, running);
    app.start_study_calculation(s, running).unwrap();
    let fixed = app.studies.items[s].fixed.clone();
    assert_eq!(
        call(
            &mut app,
            s,
            running,
            "assign_section",
            json!({"targets":fixed,"section":2})
        )
        .error
        .unwrap()
        .code,
        "calculation_busy"
    );
    accepted(call(
        &mut app,
        s,
        other,
        "assign_section",
        json!({"targets":fixed,"section":2}),
    ));
    assert_eq!(app.ai_proposal_reference(s, running), started);
    assert_eq!(
        app.proposal_input(s, other)
            .model
            .element(ElemId(2))
            .unwrap()
            .section,
        Some(SectionId(2))
    );
    crate::app::study::tests::wait(&mut app);
    let run = &app.studies.items[s].runs[0];
    assert_eq!(App::ai_input_reference(&run.input), started);
    assert_eq!(
        run.input.model.element(ElemId(2)).unwrap().section,
        Some(SectionId(1))
    );
    assert_eq!(app.studies.active, Location::Main);
}

#[test]
fn submitted_receipts_recheck_saved_version_and_original_calculation_even_without_result_references(
) {
    for include_reference in [false, true] {
        let (mut app, s) = fixture();
        let p = app.create_comparison(s, "提出の比較".into()).unwrap();
        app.start_study_calculation(s, p).unwrap();
        crate::app::study::tests::wait(&mut app);
        permit(&mut app, s, Mode::Comparison);
        accepted(call(&mut app, s, p, "begin_request", Value::Null));
        let references = if include_reference {
            comparison::projection::check_rows(&app.studies.items[s].runs[0].comparison_checks[0])
                .unwrap()
                .into_iter()
                .find_map(|row| row.evidence)
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        assert_eq!(references.len(), usize::from(include_reference));
        let mut submit = request(
            &app,
            s,
            p,
            "submit",
            json!({"explanation":"提出時の計算を参照","unchecked":["採用"],"references":references}),
        );
        let submitted = accepted(app.handle_ai_request("client", submit.clone()));
        assert_eq!(submitted["submission"]["run_index"], 0);
        let saved_submission =
            rmp_serde::to_vec_named(&app.studies.items[s].ai.tasks[0].submissions[0]).unwrap();
        let fixed = app.studies.items[s].fixed.clone();
        accepted(call(
            &mut app,
            s,
            p,
            "assign_section",
            json!({"targets":fixed,"section":2}),
        ));
        app.ai_connection.pending = Some("client".into());
        app.approve_ai_connection(s, Mode::Review, false).unwrap();
        submit.session = Some(
            app.ai_connection
                .permission
                .as_ref()
                .unwrap()
                .session
                .clone(),
        );
        assert_eq!(
            app.handle_ai_request("client", submit.clone())
                .error
                .unwrap()
                .code,
            "permission_denied"
        );
        assert_eq!(
            call(&mut app, s, p, "receipt", json!({"id":submit.id}))
                .error
                .unwrap()
                .code,
            "permission_denied"
        );
        permit(&mut app, s, Mode::Review);
        submit.session = Some(
            app.ai_connection
                .permission
                .as_ref()
                .unwrap()
                .session
                .clone(),
        );
        assert_eq!(
            accepted(app.handle_ai_request("client", submit.clone())),
            submitted
        );
        let other = app.create_comparison(s, "後からの別計算".into()).unwrap();
        app.start_study_calculation(s, other).unwrap();
        crate::app::study::tests::wait(&mut app);
        assert_eq!(
            accepted(app.handle_ai_request("client", submit.clone())),
            submitted
        );
        app.studies.items[s].runs[0]
            .results
            .as_mut()
            .unwrap()
            .analysis_contract_version = Some(0);
        for evidence in &mut app.studies.items[s].runs[0].comparison_checks {
            evidence.bundle.analysis_contract_version = Some(0);
        }
        assert_eq!(
            app.handle_ai_request("client", submit.clone())
                .error
                .unwrap()
                .code,
            "result_unavailable"
        );
        assert_eq!(
            call(&mut app, s, p, "receipt", json!({"id":submit.id}))
                .error
                .unwrap()
                .code,
            "result_unavailable"
        );
        assert_eq!(
            rmp_serde::to_vec_named(&app.studies.items[s].ai.tasks[0].submissions[0]).unwrap(),
            saved_submission
        );
    }
}

#[test]
fn quantity_only_submission_keeps_its_fixed_evidence_and_never_uses_a_later_calculation() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "数量の比較".into()).unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "begin_request", Value::Null));
    let quantity = accepted(call(
        &mut app,
        s,
        p,
        "quantity",
        json!({"offset":0,"limit":1}),
    ));
    let mut submit = request(
        &app,
        s,
        p,
        "submit",
        json!({"explanation":"数量だけの確認","unchecked":["解析・検定"],"references":[],"quantity_references":[quantity["identity"]]}),
    );
    let submitted = accepted(app.handle_ai_request("client", submit.clone()));
    assert!(submitted["submission"]["run_index"].is_null());
    assert_eq!(
        submitted["submission"]["calculation_state"],
        "not_calculated"
    );
    assert!(app.delete_comparison(s, p).is_err());
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    assert!(app.delete_comparison(s, p).is_err());
    let retained = app.studies.items[s].ai.tasks[0].submissions[0].quantity_references[0].clone();
    assert_eq!(retained.identity, quantity["identity"]);
    assert_eq!(retained.takeoff().unwrap().items.len(), 1);
    #[cfg(feature = "gui")]
    app.open_result_evidence(references::EvidenceReference::Quantity(Box::new(retained)))
        .unwrap();
    permit(&mut app, s, Mode::Comparison);
    submit.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    let fixed = app.studies.items[s].fixed.clone();
    accepted(call(
        &mut app,
        s,
        p,
        "assign_section",
        json!({"targets":fixed,"section":2}),
    ));
    app.ai_connection.pending = Some("client".into());
    app.approve_ai_connection(s, Mode::Review, false).unwrap();
    submit.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    assert_eq!(
        app.handle_ai_request("client", submit.clone())
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert_eq!(
        call(&mut app, s, p, "receipt", json!({"id":submit.id}))
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    permit(&mut app, s, Mode::Review);
    submit.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    let other = app.create_comparison(s, "無関係な後続計算".into()).unwrap();
    app.start_study_calculation(s, other).unwrap();
    crate::app::study::tests::wait(&mut app);
    assert_eq!(
        accepted(app.handle_ai_request("client", submit.clone())),
        submitted
    );
    app.studies.items[s].ai.tasks[0].submissions[0].quantity_references[0].contract = 0;
    assert_eq!(
        app.handle_ai_request("client", submit.clone())
            .error
            .unwrap()
            .code,
        "result_unavailable"
    );
    assert_eq!(
        call(&mut app, s, p, "receipt", json!({"id":submit.id}))
            .error
            .unwrap()
            .code,
        "result_unavailable"
    );
}

#[test]
fn successful_reads_are_persisted_replayed_and_cannot_reuse_ids_for_mutations() {
    let (mut app, s) = fixture();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let read = request(&app, s, 0, "study", Value::Null);
    let result = accepted(app.handle_ai_request("client", read.clone()));
    app.studies.items[s].purpose = "人による目的の更新".into();
    assert_eq!(
        accepted(app.handle_ai_request("client", read.clone())),
        result
    );
    let mut mutation = read.clone();
    mutation.operation = "create_comparison".into();
    mutation.arguments = json!({"name":"拒否する比較"});
    assert_eq!(
        app.handle_ai_request("client", mutation)
            .error
            .unwrap()
            .code,
        "request_id_conflict"
    );
    assert_eq!(app.studies.items[s].proposals.len(), 1);
    let receipt = accepted(call(&mut app, s, 0, "receipt", json!({"id":read.id})));
    assert_eq!(receipt["content"]["operation"], "study");
    assert_eq!(receipt["result"], result);
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    permit(&mut app, s, Mode::Comparison);
    let mut replay = read;
    replay.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    assert_eq!(accepted(app.handle_ai_request("client", replay)), result);
}

#[test]
fn cached_reads_recheck_history_permission_and_result_validity_and_mutations_recheck_mode() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "再送の比較".into()).unwrap();
    app.start_study_calculation(s, p).unwrap();
    crate::app::study::tests::wait(&mut app);
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "begin_request", Value::Null));
    let mut read = request(&app, s, p, "members", json!({"offset":0,"limit":1}));
    let original = accepted(app.handle_ai_request("client", read.clone()));
    let fixed = app.studies.items[s].fixed.clone();
    let mut assign = request(
        &app,
        s,
        p,
        "assign_section",
        json!({"targets":fixed,"section":2}),
    );
    let assigned = accepted(app.handle_ai_request("client", assign.clone()));
    app.ai_connection.pending = Some("client".into());
    app.approve_ai_connection(s, Mode::Review, false).unwrap();
    read.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    assign.session = read.session.clone();
    assert_eq!(
        app.handle_ai_request("client", assign.clone())
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert_eq!(
        app.handle_ai_request("client", read.clone())
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert_eq!(
        call(&mut app, s, p, "receipt", json!({"id":read.id}))
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert_eq!(
        accepted(call(&mut app, s, p, "receipt", json!({"id":assign.id})))["result"],
        assigned
    );
    permit(&mut app, s, Mode::Comparison);
    read.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    assert_eq!(
        accepted(app.handle_ai_request("client", read.clone())),
        original
    );
    assert_eq!(
        accepted(call(&mut app, s, p, "receipt", json!({"id":read.id})))["result"],
        original
    );
    let mut result = request(
        &app,
        s,
        p,
        "results",
        json!({"run_index":0,"offset":0,"limit":1}),
    );
    result.input = read.input.clone();
    accepted(app.handle_ai_request("client", result.clone()));
    app.studies.items[s].runs[0]
        .results
        .as_mut()
        .unwrap()
        .analysis_contract_version = Some(0);
    for evidence in &mut app.studies.items[s].runs[0].comparison_checks {
        evidence.bundle.analysis_contract_version = Some(0);
    }
    assert_eq!(
        app.handle_ai_request("client", result.clone())
            .error
            .unwrap()
            .code,
        "result_unavailable"
    );
    assert_eq!(
        call(&mut app, s, p, "receipt", json!({"id":result.id}))
            .error
            .unwrap()
            .code,
        "result_unavailable"
    );
}

#[test]
fn unavailable_and_partial_check_counts_are_not_zero_and_known_ng_survives_unrated_values() {
    let mut app = crate::app::study::tests::app();
    app.core
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "長期＋地震X".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        });
    let s = app
        .create_study("検定範囲の取得".into(), vec![ElemId(2)])
        .unwrap();
    app.start_study_calculation(s, 0).unwrap();
    crate::app::study::tests::wait(&mut app);
    permit(&mut app, s, Mode::Review);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let original = app.studies.items[s].runs[0].clone();
    assert!(original.comparison_checks.len() > 1);
    app.studies.items[s].runs[0].comparison_checks.clear();
    let none = accepted(call(
        &mut app,
        s,
        0,
        "results",
        json!({"run_index":0,"offset":0,"limit":1}),
    ));
    assert!(none["whole_checks"]["checked"].is_null());
    assert!(none["whole_checks"]["unchecked"].is_null());
    assert!(none["whole_checks"]["confirmed_ng"].is_null());
    assert!(none["whole_checks"]["observed_counts"].is_null());
    let input = app.ai_proposal_reference(s, 0);
    assert!(app.ai_verification(s, &input).observed_counts.is_none());
    app.studies.items[s].runs[0] = original.clone();
    app.studies.items[s].runs[0].comparison_checks[0]
        .bundle
        .member_checks
        .clear();
    let partial = accepted(call(
        &mut app,
        s,
        0,
        "results",
        json!({"run_index":0,"offset":0,"limit":1}),
    ));
    assert!(partial["whole_checks"]["unchecked"].is_null());
    assert!(partial["whole_checks"]["observed_counts"]["scope"]
        .as_str()
        .unwrap()
        .contains("部分集計"));
    assert_eq!(
        partial["whole_checks"]["observed_counts"]["count_unit"],
        CHECK_COUNT_UNIT
    );
    app.studies.items[s].runs[0] = original;
    let check = app.studies.items[s].runs[0]
        .comparison_checks
        .iter_mut()
        .flat_map(|e| &mut e.bundle.member_checks)
        .flat_map(|m| &mut m.positions)
        .find_map(|position| match &mut position.outcome {
            sepika_design_jp::CheckOutcome::Checked(check) if check.components.len() >= 2 => {
                Some(check)
            }
            _ => None,
        })
        .unwrap();
    check.components[0].ratio = 2.0;
    check.components[1].ratio = f64::NAN;
    let mixed = accepted(call(
        &mut app,
        s,
        0,
        "results",
        json!({"run_index":0,"offset":0,"limit":1}),
    ));
    assert!(mixed["whole_checks"]["confirmed_ng"].as_u64().unwrap() > 0);
    assert!(mixed["whole_checks"]["unchecked"].as_u64().unwrap() > 0);
    assert!(mixed["whole_checks"]["maximum"].is_null());
    assert!(mixed["whole_checks"]["evaluated_maximum"].as_f64().unwrap() >= 2.0);
    let verification = app.ai_verification(s, &input);
    assert!(verification.confirmed_ng.unwrap() > 0);
    assert!(verification.unchecked.unwrap() > 0);
}

#[test]
fn review_request_remains_read_only_after_new_comparison_permission_and_reload() {
    let (mut app, s) = fixture();
    permit(&mut app, s, Mode::Review);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let before = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
    for operation in ["create_comparison", "assign_section", "calculate"] {
        let response = call(&mut app, s, 0, operation, Value::Null);
        assert_eq!(response.error.unwrap().code, "permission_denied");
        assert_eq!(
            rmp_serde::to_vec_named(&app.saved_studies()).unwrap(),
            before
        );
    }
    let quantity = accepted(call(
        &mut app,
        s,
        0,
        "quantity",
        json!({"offset":0,"limit":1}),
    ));
    assert_eq!(call(&mut app,s,0,"submit",json!({"explanation":"不正な数量参照","unchecked":[],"references":[],"quantity_references":["missing"]})).error.unwrap().code,"quantity_reference_invalid");
    let quantity_id = quantity["identity"].clone();
    accepted(call(
        &mut app,
        s,
        0,
        "submit",
        json!({"explanation":"基準の確認。未計算です。","unchecked":["解析・検定"],"references":[],"quantity_references":[quantity_id]}),
    ));
    assert_eq!(
        app.ai_observations(s).unwrap().tasks[0].submissions[0].calculation_state,
        "not_calculated"
    );
    assert_eq!(
        app.ai_observations(s).unwrap().tasks[0].submissions[0]
            .quantity_references
            .len(),
        1
    );
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    assert!(app.ai_connection.permission.is_none());
    permit(&mut app, s, Mode::Comparison);
    assert_eq!(
        call(&mut app, s, 0, "create_comparison", json!({"name":"比較"}))
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert_eq!(app.studies.items[s].proposals.len(), 1);
}

#[test]
fn rejected_scopes_versions_targets_and_replays_leave_models_unchanged() {
    let (mut app, s) = fixture();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let create = request(&app, s, 0, "create_comparison", json!({"name":"比較"}));
    let first = accepted(app.handle_ai_request("client", create.clone()));
    assert_eq!(
        accepted(app.handle_ai_request("client", create.clone())),
        first
    );
    assert_eq!(app.studies.items[s].proposals.len(), 2);
    let mut conflict = create;
    conflict.arguments = json!({"name":"違う比較"});
    assert_eq!(
        app.handle_ai_request("client", conflict)
            .error
            .unwrap()
            .code,
        "request_id_conflict"
    );
    let fixed = app.studies.items[s].fixed.clone();
    let good = request(
        &app,
        s,
        1,
        "assign_section",
        json!({"targets":fixed,"section":2}),
    );
    let mut rejected = vec![];
    let mut main = good.clone();
    main.proposal = Some(app.studies.items[s].baseline.references.proposal.clone());
    rejected.push(main);
    let mut wrong = good.clone();
    wrong.project = Some("別プロジェクト".into());
    rejected.push(wrong);
    let mut old = good.clone();
    old.input.as_mut().unwrap()["generation"] = json!(900);
    rejected.push(old);
    let mut outside = good.clone();
    outside.arguments["targets"][0]["identity"] = json!("invalid");
    rejected.push(outside);
    let mut unknown = good.clone();
    unknown.operation = "adopt".into();
    rejected.push(unknown);
    let before = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
    for mut req in rejected {
        req.id = uuid::Uuid::new_v4().to_string();
        assert!(app.handle_ai_request("client", req).error.is_some());
        assert_eq!(
            rmp_serde::to_vec_named(&app.saved_studies()).unwrap(),
            before
        );
    }
    accepted(app.handle_ai_request("client", good.clone()));
    let after = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
    accepted(app.handle_ai_request("client", good.clone()));
    assert_eq!(
        rmp_serde::to_vec_named(&app.saved_studies()).unwrap(),
        after
    );
    let mut stale = good;
    stale.id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        app.handle_ai_request("client", stale.clone())
            .error
            .unwrap()
            .code,
        "input_conflict"
    );
    app.disconnect_ai_connection();
    assert_eq!(
        app.handle_ai_request("client", stale).error.unwrap().code,
        "permission_denied"
    );
}

#[cfg(windows)]
#[test]
fn real_pipe_edits_existing_comparison_calculates_and_submits_the_same_gui_evidence() {
    use sepika_ai_connection::{
        pipe,
        protocol::{read_frame, write_frame},
    };
    use tokio::io::BufReader;
    let (mut app, s) = fixture();
    let endpoint = app.enable_ai_connection().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        async fn exchange(app:&mut App,reader:&mut BufReader<tokio::net::windows::named_pipe::NamedPipeClient>,request:&Request)->Response {
            write_frame(reader.get_mut(),request).await.unwrap();
            let response=read_frame::<Response>(reader);
            tokio::pin!(response);
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
            loop {
                tokio::select! {
                    response=&mut response=>return response.unwrap().unwrap(),
                    _=tokio::time::sleep(std::time::Duration::from_millis(2))=>{
                        app.poll_study_calculation();app.poll_ai_connection();
                        assert!(std::time::Instant::now()<deadline,"実通信応答がありません");
                    }
                }
            }
        }
        let mut reader=BufReader::new(pipe::connect(&endpoint).unwrap());
        let connect:Request=serde_json::from_value(json!({"version":1,"id":"connect-1","operation":"connect"})).unwrap();
        assert_eq!(accepted(exchange(&mut app,&mut reader,&connect).await)["status"],"awaiting_gui_approval");
        app.approve_ai_connection(s,Mode::Comparison,true).unwrap();
        let connected=accepted(exchange(&mut app,&mut reader,&connect).await);
        assert_eq!(connected["status"],"approved");
        let main=app.input_reference();
        let begin=request(&app,s,0,"begin_request",Value::Null);
        accepted(exchange(&mut app,&mut reader,&begin).await);
        let create=request(&app,s,0,"create_comparison",json!({"name":"実通信比較"}));
        accepted(exchange(&mut app,&mut reader,&create).await);
        let fixed=app.studies.items[s].fixed.clone();
        let assign=request(&app,s,1,"assign_section",json!({"targets":fixed,"section":2}));
        let edit=accepted(exchange(&mut app,&mut reader,&assign).await);
        assert_eq!(edit["applied"],true);
        assert_eq!(app.studies.active,Location::Main);
        assert_eq!(app.input_reference(),main);
        let calculate=request(&app,s,1,"calculate",Value::Null);
        let job=accepted(exchange(&mut app,&mut reader,&calculate).await);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        loop {
            let status=request(&app,s,1,"job",json!({"id":job["id"]}));
            let status=accepted(exchange(&mut app,&mut reader,&status).await);
            if status["state"]=="calculated" {break;}
            assert_eq!(status["state"],"running","{status}");
            assert!(std::time::Instant::now()<deadline);
        }
        let result=request(&app,s,1,"results",json!({"run_index":0,"offset":0,"limit":1}));
        let first=accepted(exchange(&mut app,&mut reader,&result).await);
        assert_eq!(first["page"]["omitted"],true);
        let total=first["page"]["total"].as_u64().unwrap() as usize;
        let mut rows=Vec::new();
        for offset in (0..total).step_by(100) {
            let result=request(&app,s,1,"results",json!({"run_index":0,"offset":offset,"limit":100}));
            rows.extend(accepted(exchange(&mut app,&mut reader,&result).await)["page"]["items"].as_array().unwrap().clone());
        }
        assert_eq!(rows.len(),total);
        let displacement=rows.iter().find(|r|r["kind"]=="displacement" && r["node"]==2 && r["direction"]=="X").unwrap();
        let reference:references::EvidenceReference=serde_json::from_value(displacement["reference"].clone()).unwrap();
        let references::EvidenceReference::Calculation(reference)=reference else {panic!()};
        let run=&app.studies.items[s].runs[0];
        let gui=comparison::projection::result_app(&run.prepared,run.results.as_ref().unwrap(),run.last_static);
        let key=serde_json::from_value(displacement["case"].clone()).unwrap();
        let (expected,_)=comparison::projection::displacement(&gui,key,NodeId(2),0).unwrap();
        let actual=displacement["value"].as_f64().unwrap();
        assert!((actual-expected).abs()<1e-10);
        let calculation_nodes=gui.recorded_result_input(&reference).unwrap().model.nodes.len();
        assert!(calculation_nodes>app.core.model.nodes.len());
        let mismatch=request(&app,s,1,"evidence",json!({"reference":reference,"table":"design_details","offset":0,"limit":1}));
        assert_eq!(exchange(&mut app,&mut reader,&mismatch).await.error.unwrap().code,"result_kind_mismatch");
        let evidence=request(&app,s,1,"evidence",json!({"reference":reference,"table":"nodes","offset":0,"limit":1}));
        let evidence=accepted(exchange(&mut app,&mut reader,&evidence).await);
        assert_eq!(evidence["page"]["total"],calculation_nodes);
        let submit=request(&app,s,1,"submit",json!({"explanation":"既存結果に基づく比較","unchecked":["人による採用"],"references":[reference]}));
        let submitted=accepted(exchange(&mut app,&mut reader,&submit).await);
        assert_eq!(submitted["submission"]["calculation_state"],"calculated");
        assert_eq!(app.input_reference(),main);
        let old_input=app.ai_proposal_reference(s,1);
        let edit_again=request(&app,s,1,"assign_section",json!({"targets":fixed,"section":1}));
        accepted(exchange(&mut app,&mut reader,&edit_again).await);
        let mut old_result=request(&app,s,1,"results",json!({"run_index":0,"offset":0,"limit":1}));
        old_result.input=Some(json!(old_input));
        assert!(exchange(&mut app,&mut reader,&old_result).await.error.is_none());
        let save=rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        app.restore_studies(rmp_serde::from_slice(&save).unwrap()).unwrap();
        assert!(app.ai_connection.permission.is_none());
        assert_eq!(app.ai_observations(s).unwrap().tasks[0].submissions.len(),1);
        assert!(app.studies.items[s].ai.receipts.len()>6);
    });
}

#[cfg(feature = "gui")]
#[test]
fn confirmation_wait_is_observable_and_cannot_be_acknowledged_by_ai_or_resumed_by_reload() {
    let mut app = crate::app::study::tests::app();
    let mut bottom = app.core.model.elements[2].clone();
    bottom.id = ElemId(3);
    bottom.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    app.core.model.elements.push(bottom);
    app.core.model.rebuild_assignment_regions().unwrap();
    let s = app
        .create_study("確認待ちの拒否".into(), vec![ElemId(2)])
        .unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, 0, "begin_request", Value::Null));
    let job = accepted(call(&mut app, s, 0, "calculate", Value::Null));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        app.poll_study_calculation();
        app.sync_ai_jobs();
        if app.studies.items[s].ai.jobs[0].state == "awaiting_gui_confirmation" {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        accepted(call(&mut app, s, 0, "job", json!({"id":job["id"]})))["state"],
        "awaiting_gui_confirmation"
    );
    for operation in ["resume", "acknowledge_warning", "cancel"] {
        assert_eq!(
            call(&mut app, s, 0, operation, Value::Null)
                .error
                .unwrap()
                .code,
            "unsupported_operation"
        );
        assert!(app
            .studies
            .running
            .as_ref()
            .unwrap()
            .worker
            .core
            .scoped
            .pending_unset_analysis
            .is_some());
    }
    accepted(call(
        &mut app,
        s,
        0,
        "submit",
        json!({"explanation":"GUIの確認を待っています","unchecked":["警告の確認と解析"],"references":[]}),
    ));
    assert_eq!(
        app.ai_observations(s).unwrap().tasks[0].submissions[0].calculation_state,
        "awaiting_gui_confirmation"
    );
    app.disconnect_ai_connection();
    assert!(app.studies.running.is_some());
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    assert!(app.studies.running.is_none());
    assert_eq!(
        app.studies.items[s].ai.jobs[0].state,
        "interrupted_no_result"
    );
    assert!(app.ai_connection.permission.is_none());
}
