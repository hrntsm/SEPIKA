use super::*;

#[cfg(feature = "gui")]
fn prepared_call(app: &mut App, s: usize, id: &str, operation: &str, arguments: Value) -> Response {
    let input = app.studies.items[s]
        .ai
        .tasks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .started_input
        .clone();
    let mut req = request(app, s, 0, operation, arguments);
    req.request = Some(id.into());
    req.proposal = Some(input.proposal.clone());
    req.input = Some(json!(input));
    app.handle_ai_request("client", req)
}

#[cfg(feature = "gui")]
#[test]
fn prepared_review_keeps_generic_targets_unstarted_and_read_only_after_copy_connection_and_reload()
{
    let mut app = crate::app::study::tests::app();
    app.select_node(NodeId(0));
    app.prepare_ai_request("節点の条件を確認".into(), Mode::Review)
        .unwrap();
    let original = app.input_reference();
    let (s, id) = app.confirm_ai_request().unwrap();
    app.select_member(ElemId(2));
    let task = &app.studies.items[s].ai.tasks[0];
    assert_eq!(task.fixed[0].target, ReferenceTarget::Node(NodeId(0)));
    assert_eq!(task.started_input, original);
    assert_eq!(task.status(&[]), "未開始");
    assert!(app.studies.items[s].fixed.is_empty());
    assert_eq!(app.studies.items[s].proposals.len(), 1);
    let text = app.ai_request_text(s, &id).unwrap();
    assert!(text.contains(&id));
    assert!(text.contains(&app.studies.items[s].ai.id));
    assert!(!text.contains("young"));
    permit(&mut app, s, Mode::Comparison);
    assert_eq!(
        prepared_call(&mut app, s, &id, "begin_request", Value::Null)
            .error
            .unwrap()
            .code,
        "request_conflict"
    );
    assert!(app.studies.items[s].ai.tasks[0].started.is_none());
    app.disconnect_ai_connection();
    permit(&mut app, s, Mode::Review);
    assert_eq!(app.studies.items[s].ai.tasks[0].status(&[]), "未開始");
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "begin_request",
        Value::Null,
    ));
    assert_eq!(
        app.studies.items[s].ai.tasks[0].status(&[]),
        "作業中（内部処理は未観測）"
    );
    assert_eq!(
        prepared_call(
            &mut app,
            s,
            &id,
            "create_comparison",
            json!({"name":"禁止"})
        )
        .error
        .unwrap()
        .code,
        "permission_denied"
    );
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "submit",
        json!({"explanation":"元の節点条件を確認しました","unchecked":["設計上の採否"],"references":[]}),
    ));
    assert_eq!(app.studies.items[s].ai.tasks[0].status(&[]), "成果提出済み");
    app.studies.dirty = false;
    app.set_ai_request_note(s, &id, "人が追加の根拠を確認する".into())
        .unwrap();
    assert!(app.studies.dirty);
    app.disconnect_ai_connection();
    assert!(app.studies.items[s].ai.tasks[0]
        .status(&[])
        .contains("接続中断"));
    let saved = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
    app.restore_studies(rmp_serde::from_slice(&saved).unwrap())
        .unwrap();
    assert!(app.ai_connection.permission.is_none());
    let task = &app.studies.items[s].ai.tasks[0];
    assert_eq!(task.note, "人が追加の根拠を確認する");
    assert_eq!(
        task.submissions[0].explanation,
        "元の節点条件を確認しました"
    );
    let target = task.fixed[0].clone();
    app.show_reference(&target, references::ReferenceView::Table, true)
        .unwrap();
    assert_eq!(app.studies.active, Location::RequestInput(s, 0));
    assert!(app.study_read_only());
    assert_eq!(app.input_reference(), original);
}

#[cfg(feature = "gui")]
#[test]
fn prepared_request_rechecks_changed_input_without_following_later_selection() {
    let mut app = crate::app::study::tests::app();
    app.select_member(ElemId(2));
    app.prepare_ai_request("断面を比べる".into(), Mode::Comparison)
        .unwrap();
    app.select_node(NodeId(1));
    assert_eq!(
        app.ai_connection.draft.as_ref().unwrap().fixed[0].target,
        ReferenceTarget::Member(ElemId(2))
    );
    assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
        node: NodeId(0),
        coord: [10., 0., 0.]
    })));
    assert!(app.confirm_ai_request().unwrap_err().contains("再確認"));
    assert!(app.studies.items.is_empty());
    app.select_member(ElemId(2));
    app.prepare_ai_request("断面を比べる".into(), Mode::Comparison)
        .unwrap();
    let (s, id) = app.confirm_ai_request().unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "begin_request",
        Value::Null,
    ));
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "create_comparison",
        json!({"name":"比較案A"}),
    ));
    app.disconnect_ai_connection();
    app.switch_study(Location::Proposal(s, 1)).unwrap();
    assert!(crate::app::study::tests::assign(&mut app, 2));
    assert!(app.core.scoped.undo.can_undo());
}

#[cfg(feature = "gui")]
#[test]
fn review_from_retained_results_keeps_original_reference_and_allows_only_explicit_old_input() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "結果元".into()).unwrap();
    app.start_study_calculation(s, p).unwrap();
    crate::app::study::tests::wait(&mut app);
    app.switch_study(Location::History(s, 0)).unwrap();
    app.select_node(NodeId(0));
    let explicit = app
        .recorded_target_result_reference(
            ResultInputKey::Static(
                app.core
                    .scoped
                    .last_static
                    .and_then(|key| match key {
                        StaticKey::Case(key) => Some(key),
                        _ => None,
                    })
                    .unwrap(),
            ),
            ReferenceTarget::Node(NodeId(0)),
            references::DisplayConditions {
                direction: Some("X".into()),
                component: Some("節点変位".into()),
                unit: Some("mm".into()),
                ..Default::default()
            },
        )
        .unwrap();
    app.open_result_evidence(&explicit).unwrap();
    app.ai_connection.request_evidence = Some(references::EvidenceReference::from(&explicit));
    app.prepare_ai_request("計算時の節点変位を確認".into(), Mode::Review)
        .unwrap();
    let source = app.input_reference();
    let (same, id) = app.confirm_ai_request().unwrap();
    assert_eq!(same, s);
    let reference = app.studies.items[s].ai.tasks[0]
        .results
        .iter()
        .find(|r| r.kind != ResultInputKey::Design)
        .unwrap()
        .clone();
    assert_eq!(reference.source.proposal, source.proposal);
    app.switch_study(Location::Proposal(s, p)).unwrap();
    assert!(crate::app::study::tests::assign(&mut app, 2));
    app.ai_connection.pending = Some("client".into());
    app.approve_ai_connection(s, Mode::Review, false).unwrap();
    assert_eq!(
        prepared_call(&mut app, s, &id, "begin_request", Value::Null)
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    app.disconnect_ai_connection();
    permit(&mut app, s, Mode::Review);
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "begin_request",
        Value::Null,
    ));
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "submit",
        json!({"run_index":0,"explanation":"保持した旧版の変位を確認","unchecked":["現在案の再計算"],"references":[reference]}),
    ));
    let path = std::env::temp_dir().join(format!("sepika-request-{}.ovika", uuid::Uuid::new_v4()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let task = &reopened.studies.items[s].ai.tasks[0];
    assert_eq!(task.started_input, source);
    assert!(task.interrupted.is_some());
    reopened.open_result_evidence(&reference).unwrap();
    assert!(reopened.ui.scoped.result_evidence.is_some());
    let target = reopened.studies.items[s].ai.tasks[0].fixed[0].clone();
    reopened
        .show_reference(&target, references::ReferenceView::Table, true)
        .unwrap();
    assert!(reopened.study_read_only());
    assert_eq!(reopened.input_reference(), source);
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn cancelling_uncommitted_request_inputs_preserves_view_selection_and_unrelated_ui_data() {
    let mut app = crate::app::study::tests::app();
    app.select_member(ElemId(2));
    app.ui.view.camera.zoom = 4.5;
    app.ui.view.workspace.placement.arrangement = crate::app::workspace::Arrangement::Vertical;
    app.ui.scoped.node_edit = vec![["-".into(), "".into(), "".into()]];
    app.ui.scoped.axis_name_draft.editing = Some((0, 0));
    app.ui.scoped.axis_name_draft.name = "未確定".into();
    app.ui.scoped.load_cfg_draft.steel_factor = 1.7;
    app.ui.scoped.load_cfg_draft.steel_factor_active = true;
    let model = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let ctx = egui::Context::default();
    let unrelated = egui::Id::new("unrelated-ui-state");
    let numeric = egui::Id::new("test-numeric-cell");
    ctx.data_mut(|d| {
        d.insert_temp(unrelated, "維持".to_string());
        d.insert_temp(numeric, (Some(1.0), "-".to_string()));
        d.insert_temp(egui::Id::new("numeric_input_cells"), vec![numeric]);
    });
    let _ = ctx.run_ui(Default::default(), |ui| app.cancel_ai_request_inputs(ui));
    assert_eq!(app.ui.view.camera.zoom, 4.5);
    assert_eq!(
        app.ui.view.workspace.placement.arrangement,
        crate::app::workspace::Arrangement::Vertical
    );
    assert_eq!(app.ui.scoped.selection.members(), &[ElemId(2)]);
    assert!(app.ui.scoped.axis_name_draft.editing.is_none());
    assert!(!app.ui.scoped.load_cfg_draft.steel_factor_active);
    assert!(ctx
        .data(|d| d.get_temp::<(Option<f64>, String)>(numeric))
        .is_none());
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(unrelated)).as_deref(),
        Some("維持")
    );
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), model);
}

#[cfg(feature = "gui")]
#[test]
fn main_review_preserves_explicit_result_source_while_shape_selection_does_not_invent_results() {
    let mut app = crate::app::study::tests::app();
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.select_node(NodeId(0));
    app.prepare_ai_request("節点入力の確認".into(), Mode::Review)
        .unwrap();
    assert!(app.ai_connection.draft.as_ref().unwrap().results.is_empty());
    let key = ResultInputKey::Static(app.core.scoped.results.as_ref().unwrap().statics[0].0);
    let reference = app
        .recorded_target_result_reference(
            key,
            ReferenceTarget::Node(NodeId(0)),
            references::DisplayConditions {
                direction: Some("Z".into()),
                component: Some("節点変位".into()),
                unit: Some("mm".into()),
                ..Default::default()
            },
        )
        .unwrap();
    app.ai_connection.request_evidence = Some(references::EvidenceReference::from(&reference));
    app.prepare_ai_request("指定Z変位の確認".into(), Mode::Review)
        .unwrap();
    let (s, id) = app.confirm_ai_request().unwrap();
    assert_eq!(
        app.studies.items[s].ai.tasks[0].results,
        vec![reference.clone()]
    );
    assert_eq!(
        app.studies.items[s].ai.tasks[0].started_input,
        reference.source
    );
    permit(&mut app, s, Mode::Review);
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "begin_request",
        Value::Null,
    ));
    accepted(prepared_call(
        &mut app,
        s,
        &id,
        "submit",
        json!({"run_index":0,"explanation":"指定したZ変位の元入力を確認","unchecked":[],"references":[reference]}),
    ));
    let path =
        std::env::temp_dir().join(format!("sepika-main-review-{}.ovika", uuid::Uuid::new_v4()));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    reopened.open_result_evidence(&reference).unwrap();
    assert_eq!(
        reopened.studies.items[s].ai.tasks[0].results[0]
            .display
            .direction
            .as_deref(),
        Some("Z")
    );
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn explicit_quantity_scope_without_selection_survives_copy_replay_receipt_submission_and_reload() {
    for members in [false, true] {
        let mut app = crate::app::study::tests::app();
        let quantity = if members {
            app.member_quantity_evidence_reference(vec![ElemId(2)])
        } else {
            let references::EvidenceReference::Quantity(quantity) =
                app.quantity_evidence_reference()
            else {
                unreachable!()
            };
            *quantity
        };
        app.ai_connection.request_evidence = Some(references::EvidenceReference::Quantity(
            Box::new(quantity.clone()),
        ));
        assert!(app.ui.scoped.selection.members().is_empty());
        assert!(app.ui.scoped.selection.nodes().is_empty());
        assert!(app
            .prepare_ai_request("数量範囲の確認".into(), Mode::Comparison)
            .is_err());
        app.prepare_ai_request("数量範囲の確認".into(), Mode::Review)
            .unwrap();
        assert!(app.ai_connection.draft.as_ref().unwrap().fixed.is_empty());
        app.select_member(ElemId(0));
        let (s, id) = app.confirm_ai_request().unwrap();
        assert!(app.studies.items[s].fixed.is_empty());
        assert!(app.studies.items[s].ai.tasks[0].fixed.is_empty());
        assert_eq!(app.studies.items[s].proposals.len(), 1);
        let text = app.ai_request_text(s, &id).unwrap();
        assert!(text.contains(&quantity.identity));
        assert!(text.contains(&quantity.scope.label()));
        assert!(!text.contains("固定対象 0 件"));
        assert!(!text.contains("\"model\":"));
        permit(&mut app, s, Mode::Review);
        let begun = accepted(prepared_call(
            &mut app,
            s,
            &id,
            "begin_request",
            Value::Null,
        ));
        assert_eq!(begun["quantity_identity"], quantity.identity);
        assert_eq!(begun["quantity_scope"], json!(quantity.scope));
        let mut read = request(&app, s, 0, "quantity", json!({"offset":0,"limit":100}));
        read.request = Some(id.clone());
        let first = accepted(app.handle_ai_request("client", read.clone()));
        assert_eq!(first["identity"], quantity.identity);
        assert_eq!(first["scope"], quantity.scope.label());
        assert_eq!(
            first["scope_members"],
            if members { json!([2]) } else { Value::Null }
        );
        let count = app.studies.items[s].ai.receipts.len();
        assert_eq!(
            accepted(app.handle_ai_request("client", read.clone())),
            first
        );
        assert_eq!(app.studies.items[s].ai.receipts.len(), count);
        let receipt = accepted(prepared_call(
            &mut app,
            s,
            &id,
            "receipt",
            json!({"id":read.id}),
        ));
        assert_eq!(receipt["result"], first);
        let fresh = accepted(prepared_call(
            &mut app,
            s,
            &id,
            "quantity",
            json!({"offset":0,"limit":100}),
        ));
        assert_eq!(fresh["identity"], quantity.identity);
        accepted(prepared_call(
            &mut app,
            s,
            &id,
            "submit",
            json!({"explanation":"同じ固定範囲の数量を確認","unchecked":["設計上の採否"],"references":[],"quantity_references":[quantity.identity]}),
        ));
        let submitted = app.studies.items[s].ai.tasks[0].submissions[0]
            .receipt_id
            .clone();
        let path = std::env::temp_dir().join(format!(
            "sepika-quantity-request-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert!(reopened.ai_connection.permission.is_none());
        let task = &reopened.studies.items[s].ai.tasks[0];
        assert_eq!(task.quantity.as_ref().unwrap().identity, quantity.identity);
        assert_eq!(task.quantity.as_ref().unwrap().source, quantity.source);
        assert_eq!(
            task.submissions[0].quantity_references[0].identity,
            quantity.identity
        );
        permit(&mut reopened, s, Mode::Review);
        read.session = Some(
            reopened
                .ai_connection
                .permission
                .as_ref()
                .unwrap()
                .session
                .clone(),
        );
        assert_eq!(
            accepted(reopened.handle_ai_request("client", read.clone())),
            first
        );
        assert_eq!(
            accepted(prepared_call(
                &mut reopened,
                s,
                &id,
                "receipt",
                json!({"id":read.id})
            ))["result"],
            first
        );
        accepted(prepared_call(
            &mut reopened,
            s,
            &id,
            "receipt",
            json!({"id":submitted}),
        ));
        assert_eq!(
            accepted(prepared_call(
                &mut reopened,
                s,
                &id,
                "quantity",
                json!({"offset":0,"limit":100})
            ))["identity"],
            quantity.identity
        );
        reopened.studies.items[s].ai.tasks[0]
            .quantity
            .as_mut()
            .unwrap()
            .contract += 1;
        assert_eq!(
            reopened
                .handle_ai_request("client", read)
                .error
                .unwrap()
                .code,
            "quantity_reference_invalid"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(feature = "gui")]
#[test]
fn quantity_request_rejects_edited_input_before_and_after_reload_until_saved_input_is_explicit() {
    let mut app = crate::app::study::tests::app();
    let references::EvidenceReference::Quantity(quantity) = app.quantity_evidence_reference()
    else {
        unreachable!()
    };
    app.ai_connection.request_evidence =
        Some(references::EvidenceReference::Quantity(quantity.clone()));
    app.prepare_ai_request("元数量の確認".into(), Mode::Review)
        .unwrap();
    let (s, _) = app.confirm_ai_request().unwrap();
    assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
        node: NodeId(0),
        coord: [10., 0., 0.]
    })));
    app.ai_connection.request_evidence =
        Some(references::EvidenceReference::Quantity(quantity.clone()));
    assert!(app
        .prepare_ai_request("古い数量の確認".into(), Mode::Review)
        .unwrap_err()
        .contains("元の保存入力"));
    assert!(app.ai_connection.draft.is_none());
    let path = std::env::temp_dir().join(format!(
        "sepika-old-quantity-request-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    let saved = reopened.studies.items[s].ai.tasks[0]
        .quantity
        .clone()
        .unwrap();
    reopened.ai_connection.request_evidence =
        Some(references::EvidenceReference::Quantity(Box::new(saved)));
    assert!(reopened
        .prepare_ai_request("古い数量の確認".into(), Mode::Review)
        .unwrap_err()
        .contains("元の保存入力"));
    reopened.switch_study(Location::RequestInput(s, 0)).unwrap();
    reopened
        .prepare_ai_request("元入力を明示して数量を確認".into(), Mode::Review)
        .unwrap();
    let (same, id) = reopened.confirm_ai_request().unwrap();
    assert_eq!(same, s);
    permit(&mut reopened, s, Mode::Review);
    accepted(prepared_call(
        &mut reopened,
        s,
        &id,
        "begin_request",
        Value::Null,
    ));
    assert_eq!(
        accepted(prepared_call(
            &mut reopened,
            s,
            &id,
            "quantity",
            json!({"offset":0,"limit":100})
        ))["identity"],
        quantity.identity
    );
    assert_eq!(reopened.studies.items[s].ai.tasks.len(), 2);
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn comparison_request_keeps_the_modified_source_input_separate_from_the_baseline() {
    let (mut app, s) = fixture();
    let p = app
        .create_comparison(s, "依頼元の変更済み案".into())
        .unwrap();
    app.switch_study(Location::Proposal(s, p)).unwrap();
    assert!(crate::app::study::tests::assign(&mut app, 2));
    app.select_member(ElemId(2));
    let source = app.input_reference();
    let baseline = app.studies.items[s].baseline.clone();
    let fixed = app.studies.items[s].fixed.clone();
    app.prepare_ai_request("変更済み断面から比較を依頼".into(), Mode::Comparison)
        .unwrap();
    let (same, _) = app.confirm_ai_request().unwrap();
    assert_eq!(same, s);
    let task = &app.studies.items[s].ai.tasks[0];
    assert_eq!(task.started_input, source);
    assert_eq!(task.origin.input_reference(), source);
    assert_eq!(
        task.origin.model.element(ElemId(2)).unwrap().section,
        Some(SectionId(2))
    );
    assert_eq!(task.fixed, fixed);
    assert_ne!(task.source_targets[0].source, task.fixed[0].source);
    let target = task.source_targets[0].clone();
    assert!(crate::app::study::tests::assign(&mut app, 1));
    app.show_reference(&target, references::ReferenceView::Table, true)
        .unwrap();
    assert!(app.study_read_only());
    assert_eq!(app.input_reference(), source);
    assert_eq!(
        app.core.model.element(ElemId(2)).unwrap().section,
        Some(SectionId(2))
    );
    assert_eq!(
        app.studies.items[s]
            .baseline
            .model
            .element(ElemId(2))
            .unwrap()
            .section,
        baseline.model.element(ElemId(2)).unwrap().section
    );
    let path = std::env::temp_dir().join(format!(
        "sepika-comparison-request-origin-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    reopened
        .show_reference(&target, references::ReferenceView::Table, true)
        .unwrap();
    assert_eq!(reopened.input_reference(), source);
    assert_eq!(
        reopened.core.model.element(ElemId(2)).unwrap().section,
        Some(SectionId(2))
    );
    assert_eq!(reopened.studies.items[s].fixed, fixed);
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn main_review_rechecks_live_main_before_start_and_marks_submissions_old_after_edit_and_reload() {
    for started in [false, true] {
        let mut app = crate::app::study::tests::app();
        app.select_node(NodeId(0));
        app.prepare_ai_request("本案の節点条件を確認".into(), Mode::Review)
            .unwrap();
        let (s, id) = app.confirm_ai_request().unwrap();
        let source = app.input_reference();
        permit(&mut app, s, Mode::Review);
        if started {
            accepted(prepared_call(
                &mut app,
                s,
                &id,
                "begin_request",
                Value::Null,
            ));
            accepted(prepared_call(
                &mut app,
                s,
                &id,
                "submit",
                json!({"explanation":"作成時の本案を確認","unchecked":[],"references":[]}),
            ));
            let task = &app.studies.items[s].ai.tasks[0];
            assert!(app.ai_submission_current(s, &task.submissions[0]));
        }
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [10., 0., 0.]
        })));
        let task = &app.studies.items[s].ai.tasks[0];
        assert!(task.from_main);
        assert_eq!(task.origin.input_reference(), source);
        assert!(!app.ai_task_source_current(task));
        if started {
            assert!(!app.ai_submission_current(s, &task.submissions[0]));
        } else {
            assert_eq!(
                prepared_call(&mut app, s, &id, "begin_request", Value::Null)
                    .error
                    .unwrap()
                    .code,
                "input_conflict"
            );
            assert!(app.studies.items[s].ai.tasks[0].started.is_none());
        }
        let path = std::env::temp_dir().join(format!(
            "sepika-main-request-version-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        let task = &reopened.studies.items[s].ai.tasks[0];
        assert!(task.from_main);
        assert!(!reopened.ai_task_source_current(task));
        if started {
            assert!(!reopened.ai_submission_current(s, &task.submissions[0]));
        } else {
            permit(&mut reopened, s, Mode::Review);
            assert_eq!(
                prepared_call(&mut reopened, s, &id, "begin_request", Value::Null)
                    .error
                    .unwrap()
                    .code,
                "input_conflict"
            );
        }
        reopened.switch_study(Location::RequestInput(s, 0)).unwrap();
        assert!(reopened.study_read_only());
        assert_eq!(reopened.input_reference(), source);
        assert_eq!(reopened.core.model.node(NodeId(0)).unwrap().coord[0], 0.);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(feature = "gui")]
#[test]
fn request_list_target_action_opens_material_load_case_and_unused_section_in_their_saved_tables() {
    for target in [
        ReferenceTarget::Material(MaterialId(0)),
        ReferenceTarget::LoadCase(LoadCaseId(0)),
        ReferenceTarget::Section(SectionId(2)),
    ] {
        let mut app = crate::app::study::tests::app();
        match target {
            ReferenceTarget::Material(id) => app.ui.scoped.nav.focus_material = Some(id),
            ReferenceTarget::LoadCase(id) => app.ui.scoped.nav.focus_load_case = Some(id),
            ReferenceTarget::Section(id) => app.ui.scoped.nav.focus_section = Some(id),
            _ => unreachable!(),
        }
        app.prepare_ai_request("選んだ入力の根拠確認".into(), Mode::Review)
            .unwrap();
        let (s, _) = app.confirm_ai_request().unwrap();
        let reference = app.studies.items[s].ai.tasks[0].source_targets[0].clone();
        let source = reference.source.clone();
        assert_eq!(reference.target, target);
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [10., 0., 0.]
        })));
        app.open_ai_request_target(s, 0, &reference).unwrap();
        assert!(app.study_read_only());
        assert_eq!(app.input_reference(), source);
        assert_eq!(app.ui.scoped.opened_reference, Some(reference));
        match target {
            ReferenceTarget::Material(id) => {
                assert_eq!(app.ui.scoped.nav.focus_material, Some(id));
                assert_eq!(app.ui.view.model_tab, crate::app::ModelTab::Materials);
                assert_eq!(
                    app.active_workspace_content(),
                    crate::app::workspace::Content::Model
                );
            }
            ReferenceTarget::LoadCase(id) => {
                assert_eq!(app.ui.scoped.nav.focus_load_case, Some(id));
                assert_eq!(
                    app.active_workspace_content(),
                    crate::app::workspace::Content::Loads
                );
            }
            ReferenceTarget::Section(id) => {
                assert_eq!(app.ui.scoped.nav.focus_section, Some(id));
                assert_eq!(app.ui.view.model_tab, crate::app::ModelTab::Sections);
                assert_eq!(
                    app.active_workspace_content(),
                    crate::app::workspace::Content::Model
                );
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn request_target_action_keeps_the_chosen_request_when_sources_are_shared() {
    let mut app = crate::app::study::tests::app();
    app.select_member(ElemId(2));
    let source = app.input_reference();
    let section = app.core.model.element(ElemId(2)).unwrap().section;
    app.prepare_ai_request("先行する確認依頼".into(), Mode::Review)
        .unwrap();
    let (review, _) = app.confirm_ai_request().unwrap();
    app.prepare_ai_request("後から作る比較依頼".into(), Mode::Comparison)
        .unwrap();
    let (comparison, _) = app.confirm_ai_request().unwrap();
    let target = app.studies.items[comparison].ai.tasks[0].source_targets[0].clone();
    assert_eq!(
        app.studies.items[review].ai.tasks[0].source_targets[0],
        target
    );

    for previous in [Location::Main, Location::RequestInput(review, 0)] {
        app.switch_study(previous).unwrap();
        app.open_ai_request_target(comparison, 0, &target).unwrap();
        assert_eq!(app.studies.active, Location::RequestInput(comparison, 0));
        assert!(app.study_read_only());
        assert_eq!(app.input_reference(), source);
        assert_eq!(app.ui.scoped.opened_reference, Some(target.clone()));
        assert_eq!(app.core.model.element(ElemId(2)).unwrap().section, section);
    }
}

#[cfg(feature = "gui")]
#[test]
fn request_input_derived_review_requires_current_history_permission_for_start_reads_and_saved_replies(
) {
    fn permit_history(app: &mut App, s: usize, history: bool) {
        app.ai_connection.pending = Some("client".into());
        app.approve_ai_connection(s, Mode::Review, history).unwrap();
    }
    fn replay(app: &mut App, request: &Request) -> Response {
        let mut request = request.clone();
        request.session = Some(
            app.ai_connection
                .permission
                .as_ref()
                .unwrap()
                .session
                .clone(),
        );
        app.handle_ai_request("client", request)
    }
    let mut app = crate::app::study::tests::app();
    let quantity = app.quantity_evidence_reference();
    app.ai_connection.request_evidence = Some(quantity.clone());
    app.prepare_ai_request("本案数量の確認".into(), Mode::Review)
        .unwrap();
    let (s, _) = app.confirm_ai_request().unwrap();
    assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
        node: NodeId(0),
        coord: [10., 0., 0.]
    })));
    app.switch_study(Location::RequestInput(s, 0)).unwrap();
    app.ai_connection.request_evidence = Some(quantity);
    app.prepare_ai_request("保存元の数量を明示確認".into(), Mode::Review)
        .unwrap();
    let (same, id) = app.confirm_ai_request().unwrap();
    assert_eq!(same, s);
    let task = &app.studies.items[s].ai.tasks[1];
    assert!(task.historical);
    assert!(!task.from_main);
    assert_eq!(task.started_input, app.ai_proposal_reference(s, 0));
    assert!(!app.ai_input_is_current(s, &task.started_input));
    permit_history(&mut app, s, false);
    assert_eq!(
        prepared_call(&mut app, s, &id, "begin_request", Value::Null)
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    assert!(app.studies.items[s].ai.tasks[1].started.is_none());
    permit_history(&mut app, s, true);
    let mut begin = request(&app, s, 0, "begin_request", Value::Null);
    begin.request = Some(id.clone());
    accepted(replay(&mut app, &begin));
    let mut read = request(&app, s, 0, "quantity", json!({"offset":0,"limit":100}));
    read.request = Some(id.clone());
    let original = accepted(replay(&mut app, &read));
    let mut receipt = request(&app, s, 0, "receipt", json!({"id":read.id}));
    receipt.request = Some(id.clone());
    let original_receipt = accepted(replay(&mut app, &receipt));
    assert_eq!(original_receipt["result"], original);
    let path = std::env::temp_dir().join(format!(
        "sepika-request-history-permission-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    for reload in [false, true] {
        if reload {
            app = App::default();
            app.open_project_from(path.clone());
            assert!(app.core.scoped.last_error.is_none());
        }
        permit_history(&mut app, s, false);
        let count = app.studies.items[s].ai.receipts.len();
        for request in [&begin, &read, &receipt] {
            assert_eq!(
                replay(&mut app, request).error.unwrap().code,
                "permission_denied"
            );
        }
        assert_eq!(
            prepared_call(
                &mut app,
                s,
                &id,
                "quantity",
                json!({"offset":0,"limit":100})
            )
            .error
            .unwrap()
            .code,
            "permission_denied"
        );
        assert_eq!(
            prepared_call(&mut app, s, &id, "receipt", json!({"id":read.id}))
                .error
                .unwrap()
                .code,
            "permission_denied"
        );
        assert_eq!(app.studies.items[s].ai.receipts.len(), count);
        permit_history(&mut app, s, true);
        accepted(replay(&mut app, &begin));
        assert_eq!(accepted(replay(&mut app, &read)), original);
        assert_eq!(accepted(replay(&mut app, &receipt)), original_receipt);
        assert_eq!(
            accepted(prepared_call(
                &mut app,
                s,
                &id,
                "receipt",
                json!({"id":read.id})
            ))["result"],
            original
        );
        assert_eq!(
            accepted(prepared_call(
                &mut app,
                s,
                &id,
                "quantity",
                json!({"offset":0,"limit":100})
            ))["identity"],
            original["identity"]
        );
    }
    std::fs::remove_file(path).unwrap();
}

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

fn retained_check_reference(app: &App, s: usize, index: usize) -> ResultReference {
    comparison::projection::check_rows(&app.studies.items[s].runs[index].comparison_checks[0])
        .unwrap()
        .into_iter()
        .find_map(|row| row.evidence)
        .unwrap()
}

#[test]
fn retained_results_and_submissions_keep_their_run_state_during_recalculation_or_confirmation() {
    for confirmation in [false, true] {
        let (mut app, s) = fixture();
        let p = app.create_comparison(s, "元計算と新ジョブ".into()).unwrap();
        app.start_study_calculation(s, p).unwrap();
        crate::app::study::tests::wait(&mut app);
        permit(&mut app, s, Mode::Comparison);
        accepted(call(&mut app, s, p, "begin_request", Value::Null));
        let reference = retained_check_reference(&app, s, 0);
        let mut original_results = request(
            &app,
            s,
            p,
            "results",
            json!({"run_index":0,"offset":0,"limit":1}),
        );
        let before = accepted(app.handle_ai_request("client", original_results.clone()));
        assert_eq!(before["state"], "calculated");
        let job = accepted(call(&mut app, s, p, "calculate", Value::Null));
        if confirmation {
            let mut worker = app.proposal_input(s, p).core();
            worker.scoped.pending_unset_analysis = Some(PendingAnalysis::StaticAll);
            let (tx, rx) = std::sync::mpsc::channel();
            app.studies.running.as_mut().unwrap().preparing = Some(rx);
            tx.send(Ok(StudyWorkerOutcome::Confirmation(Box::new(worker))))
                .unwrap();
            app.poll_study_calculation();
        }
        let observed = if confirmation {
            "awaiting_gui_confirmation"
        } else {
            "running"
        };
        assert_eq!(
            accepted(call(&mut app, s, p, "job", json!({"id":job["id"]})))["state"],
            observed
        );
        assert_eq!(
            accepted(call(
                &mut app,
                s,
                p,
                "results",
                json!({"run_index":0,"offset":0,"limit":1})
            ))["state"],
            "calculated"
        );
        assert_eq!(
            accepted(app.handle_ai_request("client", original_results.clone())),
            before
        );
        let missing = call(
            &mut app,
            s,
            p,
            "submit",
            json!({"explanation":"元計算の指定が必要","unchecked":[],"references":[reference]}),
        );
        assert_eq!(missing.error.unwrap().code, "run_required");
        let mut submit = request(
            &app,
            s,
            p,
            "submit",
            json!({"run_index":0,"explanation":"保持run0の数値","unchecked":["採用"],"references":[reference]}),
        );
        let selected = accepted(app.handle_ai_request("client", submit.clone()));
        assert_eq!(selected["submission"]["source"], "run");
        assert_eq!(selected["submission"]["calculation_state"], "calculated");
        assert_eq!(selected["submission"]["run_index"], 0);
        assert!(selected["submission"]["job_id"].is_null());
        let mut explanation = request(
            &app,
            s,
            p,
            "submit",
            json!({"explanation":"進行jobの観測だけ","unchecked":["完了結果"],"references":[]}),
        );
        let observed_submission = accepted(app.handle_ai_request("client", explanation.clone()));
        assert_eq!(observed_submission["submission"]["source"], "job");
        assert_eq!(observed_submission["submission"]["job_id"], job["id"]);
        assert_eq!(
            observed_submission["submission"]["calculation_state"],
            observed
        );
        assert!(observed_submission["submission"]["run_index"].is_null());
        assert!(observed_submission["submission"]["verification"]["checked"].is_null());
        if !confirmation {
            crate::app::study::tests::wait(&mut app);
            let other_reference = retained_check_reference(&app, s, 1);
            assert_ne!(reference.run, other_reference.run);
            assert_eq!(call(&mut app,s,p,"submit",json!({"run_index":0,"explanation":"異なるrunを混ぜない","unchecked":[],"references":[reference,other_reference]})).error.unwrap().code,"submission_run_mismatch");
        }
        let saved = app.saved_studies();
        app.restore_studies(saved).unwrap();
        permit(&mut app, s, Mode::Review);
        let session = Some(
            app.ai_connection
                .permission
                .as_ref()
                .unwrap()
                .session
                .clone(),
        );
        submit.session = session.clone();
        explanation.session = session.clone();
        original_results.session = session;
        assert_eq!(accepted(app.handle_ai_request("client", submit)), selected);
        assert_eq!(
            accepted(app.handle_ai_request("client", explanation)),
            observed_submission
        );
        assert_eq!(
            accepted(app.handle_ai_request("client", original_results)),
            before
        );
    }
}

#[test]
fn a_human_successful_run_is_not_marked_failed_by_an_earlier_ai_job() {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "失敗後の人の計算".into()).unwrap();
    permit(&mut app, s, Mode::Comparison);
    accepted(call(&mut app, s, p, "begin_request", Value::Null));
    let job = accepted(call(&mut app, s, p, "calculate", Value::Null));
    let (tx, rx) = std::sync::mpsc::channel();
    app.studies.running.as_mut().unwrap().preparing = Some(rx);
    tx.send(Err("先行AI worker失敗".into())).unwrap();
    app.poll_study_calculation();
    assert_eq!(app.studies.items[s].ai.jobs[0].state, "failed");
    app.start_study_calculation(s, p).unwrap();
    crate::app::study::tests::wait(&mut app);
    assert!(app.studies.items[s].runs[0].ai_job.is_none());
    assert_eq!(
        accepted(call(&mut app, s, p, "job", json!({"id":job["id"]})))["state"],
        "failed"
    );
    assert_eq!(
        accepted(call(
            &mut app,
            s,
            p,
            "results",
            json!({"run_index":0,"offset":0,"limit":1})
        ))["state"],
        "calculated"
    );
    let reference = retained_check_reference(&app, s, 0);
    let mut submit = request(
        &app,
        s,
        p,
        "submit",
        json!({"run_index":0,"explanation":"人が完了したrun","unchecked":["採用"],"references":[reference]}),
    );
    let selected = accepted(app.handle_ai_request("client", submit.clone()));
    assert_eq!(selected["submission"]["source"], "run");
    assert_eq!(selected["submission"]["calculation_state"], "calculated");
    assert!(selected["submission"]["job_id"].is_null());
    let observed = accepted(call(
        &mut app,
        s,
        p,
        "submit",
        json!({"explanation":"先行失敗jobの観測","unchecked":["結果未指定"],"references":[]}),
    ));
    assert_eq!(observed["submission"]["source"], "job");
    assert_eq!(observed["submission"]["job_id"], job["id"]);
    assert_eq!(observed["submission"]["calculation_state"], "failed");
    assert!(observed["submission"]["verification"]["checked"].is_null());
    let saved = app.saved_studies();
    app.restore_studies(saved).unwrap();
    permit(&mut app, s, Mode::Review);
    submit.session = Some(
        app.ai_connection
            .permission
            .as_ref()
            .unwrap()
            .session
            .clone(),
    );
    assert_eq!(accepted(app.handle_ai_request("client", submit)), selected);
}

#[test]
fn a_fresh_review_connection_discovers_saved_manual_runs_and_history_permission_controls_every_read(
) {
    let (mut app, s) = fixture();
    let p = app.create_comparison(s, "手計算の比較".into()).unwrap();
    let other = app.create_comparison(s, "別案の手計算".into()).unwrap();
    app.start_study_calculation(s, p).unwrap();
    crate::app::study::tests::wait(&mut app);
    app.switch_study(Location::Proposal(s, p)).unwrap();
    assert!(crate::app::study::tests::assign(&mut app, 2));
    app.switch_study(Location::Main).unwrap();
    app.start_study_calculation(s, p).unwrap();
    crate::app::study::tests::wait(&mut app);
    app.start_study_calculation(s, other).unwrap();
    crate::app::study::tests::wait(&mut app);
    app.switch_study(Location::Proposal(s, p)).unwrap();
    assert!(crate::app::study::tests::assign(&mut app, 1));
    app.switch_study(Location::Main).unwrap();
    let path = std::env::temp_dir().join(format!(
        "sepika-ai-discovery-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut app = App::default();
    app.open_project_from(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    std::fs::remove_file(path).unwrap();
    assert!(app.studies.items[s].ai.tasks.is_empty());
    assert!(app.studies.items[s].ai.receipts.is_empty());
    let connect: Request =
        serde_json::from_value(json!({"version":1,"id":"fresh-connect","operation":"connect"}))
            .unwrap();
    accepted(app.handle_ai_request("client", connect.clone()));
    app.approve_ai_connection(s, Mode::Review, true).unwrap();
    let connected = accepted(app.handle_ai_request("client", connect));
    let begin = request(&app, s, 0, "begin_request", Value::Null);
    assert_eq!(begin.input.as_ref().unwrap(), &connected["baseline"]);
    accepted(app.handle_ai_request("client", begin));
    let proposals = accepted(call(
        &mut app,
        s,
        0,
        "proposals",
        json!({"offset":0,"limit":100}),
    ));
    let current: InputReference = serde_json::from_value(
        proposals["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "手計算の比較")
            .unwrap()["input"]
            .clone(),
    )
    .unwrap();
    let mut runs = request(&app, s, 0, "runs", json!({"offset":0,"limit":1}));
    runs.proposal = Some(current.proposal.clone());
    runs.input = Some(json!(current));
    let discovered = accepted(app.handle_ai_request("client", runs.clone()));
    assert_eq!(discovered["history_included"], true);
    assert_eq!(discovered["page"]["total"], 2);
    assert_eq!(discovered["page"]["omitted"], true);
    assert_eq!(discovered["page"]["next_offset"], 1);
    let row = &discovered["page"]["items"][0];
    assert_eq!(row["recorded_calculation_state"], "calculated");
    assert_eq!(row["matches_requested_input"], false);
    assert!(row["ai_job"].is_null());
    assert!(row.get("result_available").is_none());
    let old: InputReference = serde_json::from_value(row["input"].clone()).unwrap();
    assert_ne!(old, current);
    let mut second = runs.clone();
    second.id = uuid::Uuid::new_v4().to_string();
    second.arguments = json!({"offset":1,"limit":1});
    let second = accepted(app.handle_ai_request("client", second));
    assert_eq!(second["page"]["items"][0]["run_index"], 1);
    assert_ne!(second["page"]["items"][0]["input"], row["input"]);
    assert!(second["page"]["next_offset"].is_null());
    let mut results = request(
        &app,
        s,
        0,
        "results",
        json!({"run_index":row["run_index"],"offset":0,"limit":1}),
    );
    results.proposal = Some(old.proposal.clone());
    results.input = Some(json!(old));
    let numerical = accepted(app.handle_ai_request("client", results.clone()));
    assert_eq!(numerical["conditions"], row["conditions"]);
    let reference = numerical["page"]["items"][0]["reference"].clone();
    assert!(!reference.is_null());
    let mut evidence = request(
        &app,
        s,
        0,
        "evidence",
        json!({"reference":reference,"table":"nodes","offset":0,"limit":1}),
    );
    evidence.proposal = Some(old.proposal.clone());
    evidence.input = Some(json!(old));
    accepted(app.handle_ai_request("client", evidence.clone()));
    app.ai_connection.pending = Some("client".into());
    app.approve_ai_connection(s, Mode::Review, false).unwrap();
    let session = app
        .ai_connection
        .permission
        .as_ref()
        .unwrap()
        .session
        .clone();
    runs.session = Some(session.clone());
    assert_eq!(
        app.handle_ai_request("client", runs.clone())
            .error
            .unwrap()
            .code,
        "permission_denied"
    );
    let mut current_only = runs.clone();
    current_only.id = uuid::Uuid::new_v4().to_string();
    let visible = accepted(app.handle_ai_request("client", current_only));
    assert_eq!(visible["history_included"], false);
    assert_eq!(visible["page"]["total"], 0);
    assert!(visible["page"]["items"].as_array().unwrap().is_empty());
    let other_current: InputReference = serde_json::from_value(
        proposals["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "別案の手計算")
            .unwrap()["input"]
            .clone(),
    )
    .unwrap();
    let mut current_run = request(&app, s, 0, "runs", json!({"offset":0,"limit":1}));
    current_run.proposal = Some(other_current.proposal.clone());
    current_run.input = Some(json!(other_current));
    let current_run = accepted(app.handle_ai_request("client", current_run));
    assert_eq!(current_run["page"]["total"], 1);
    assert_eq!(current_run["page"]["items"][0]["run_index"], 2);
    assert_eq!(
        current_run["page"]["items"][0]["matches_requested_input"],
        true
    );
    let mut receipt = request(&app, s, 0, "receipt", json!({"id":runs.id}));
    receipt.proposal = Some(current.proposal.clone());
    receipt.input = Some(json!(current));
    assert_eq!(
        app.handle_ai_request("client", receipt).error.unwrap().code,
        "permission_denied"
    );
    for mut req in [results, evidence] {
        req.session = Some(session.clone());
        assert_eq!(
            app.handle_ai_request("client", req.clone())
                .error
                .unwrap()
                .code,
            "permission_denied"
        );
        req.id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            app.handle_ai_request("client", req).error.unwrap().code,
            "input_conflict"
        );
    }
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
        #[cfg(feature = "gui")]
        assert_eq!(
            app.studies.items[s].ai.tasks[0].status(&app.studies.items[s].ai.jobs),
            "失敗"
        );
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
            json!({"run_index":0,"explanation":"提出時の計算を参照","unchecked":["採用"],"references":references}),
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
                    response=&mut response=>{
                        let response=response.unwrap().unwrap();
                        assert_eq!(response.id,request.id);
                        return response;
                    },
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
        let submit=request(&app,s,1,"submit",json!({"run_index":0,"explanation":"既存結果に基づく比較","unchecked":["人による採用"],"references":[reference]}));
        let submitted=accepted(exchange(&mut app,&mut reader,&submit).await);
        assert_eq!(submitted["submission"]["calculation_state"],"calculated");
        assert_eq!(app.input_reference(),main);
        let old_input=app.ai_proposal_reference(s,1);
        let edit_again=request(&app,s,1,"assign_section",json!({"targets":fixed,"section":1}));
        accepted(exchange(&mut app,&mut reader,&edit_again).await);
        let mut old_result=request(&app,s,1,"results",json!({"run_index":0,"offset":0,"limit":1}));
        old_result.input=Some(json!(old_input));
        assert!(exchange(&mut app,&mut reader,&old_result).await.error.is_none());
        drop(reader);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while app.ai_connection.listener.is_some() {
            app.poll_ai_connection();
            assert!(std::time::Instant::now()<deadline,"切断後の有効化が終了しません");
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(app.ai_connection.permission.is_none());
        assert!(pipe::connect(&endpoint).is_err());
        assert_eq!(app.handle_ai_request("old",old_result.clone()).error.unwrap().code,"permission_denied");
        let new_endpoint=app.enable_ai_connection().unwrap();
        assert_ne!(endpoint,new_endpoint);
        let mut reader=BufReader::new(pipe::connect(&new_endpoint).unwrap());
        assert_eq!(accepted(exchange(&mut app,&mut reader,&connect).await)["status"],"awaiting_gui_approval");
        assert_eq!(exchange(&mut app,&mut reader,&old_result).await.error.unwrap().code,"permission_denied");
        app.approve_ai_connection(s,Mode::Review,true).unwrap();
        let reconnected=accepted(exchange(&mut app,&mut reader,&connect).await);
        assert_ne!(reconnected["session"],connected["session"]);
        assert_eq!(exchange(&mut app,&mut reader,&old_result).await.error.unwrap().code,"disconnected");
        let mut new_begin=request(&app,s,0,"begin_request",Value::Null);
        new_begin.request=Some("new-review-request".into());
        assert_eq!(accepted(exchange(&mut app,&mut reader,&new_begin).await)["status"],"started");
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
    #[cfg(feature = "gui")]
    assert_eq!(
        app.studies.items[s].ai.tasks[0].status(&app.studies.items[s].ai.jobs),
        "人の確認待ち"
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
