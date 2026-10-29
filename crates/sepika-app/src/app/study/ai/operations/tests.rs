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
        let evidence=request(&app,s,1,"evidence",json!({"reference":reference,"table":"nodes","offset":0,"limit":1}));
        let evidence=accepted(exchange(&mut app,&mut reader,&evidence).await);
        assert_eq!(evidence["page"]["total"],app.core.model.nodes.len());
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
        assert_eq!(app.studies.items[s].ai.receipts.len(),6);
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
