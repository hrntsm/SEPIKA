use super::*;
use sepika_ai_connection::{
    pipe,
    protocol::{read_frame, write_frame, Request, Response},
};
use sepika_app::app::study::ai::Mode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::windows::named_pipe::NamedPipeClient;

async fn exchange(
    app: &mut App,
    reader: &mut BufReader<NamedPipeClient>,
    request: &Request,
) -> Response {
    write_frame(reader.get_mut(), request).await.unwrap();
    let response = read_frame::<Response>(reader);
    tokio::pin!(response);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        tokio::select! {
            response = &mut response => {
                let response=response.unwrap().unwrap();
                assert_eq!(response.id,request.id);
                return response;
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                app.poll_study_calculation();
                app.poll_ai_connection();
                assert!(std::time::Instant::now()<deadline,"ローカル実通信の応答期限");
            }
        }
    }
}

fn accepted(response: Response) -> Value {
    assert!(response.error.is_none(), "{:?}", response.error);
    response.result.unwrap()
}

fn request(connection: &Value, input: &Value, operation: &str, arguments: Value) -> Request {
    serde_json::from_value(json!({"version":1,"id":uuid::Uuid::new_v4().to_string(),"operation":operation,
        "session":connection["session"],"project":connection["project"],"study":connection["study"],
        "request":"issue985-comparison","proposal":input["proposal"],"input":input,"arguments":arguments})).unwrap()
}

fn close_numbers(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() <= b.abs() * 1e-9 + 1e-9, "{a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close_numbers(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (key, b) in b {
                close_numbers(a.get(key).unwrap(), b);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn representative_real_pipe_matches_manual_inputs_checks_quantity_and_displacement() {
    let mut app = fixture();
    let target = target(&app);
    let baseline = app.core.model.clone();
    let candidates = [section(&app, "GY2", Some("2")), section(&app, "b6", None)];
    let s = app
        .create_study("Issue985 実通信と手作業の同条件比較".into(), vec![target])
        .unwrap();
    let endpoint = app.enable_ai_connection().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut reader=BufReader::new(pipe::connect(&endpoint).unwrap());
        let connect:Request=serde_json::from_value(json!({"version":1,"id":"issue985-connect","operation":"connect"})).unwrap();
        assert_eq!(accepted(exchange(&mut app,&mut reader,&connect).await)["status"],"awaiting_gui_approval");
        app.approve_ai_connection(s,Mode::Comparison,true).unwrap();
        let connected=accepted(exchange(&mut app,&mut reader,&connect).await);
        let baseline_input=connected["baseline"].clone();
        let begin=request(&connected,&baseline_input,"begin_request",Value::Null);
        accepted(exchange(&mut app,&mut reader,&begin).await);
        let mut captured=Vec::new();
        for index in 0..3 {
            let mut current=baseline_input.clone();
            if index>0 {
                let create=request(&connected,&current,"create_comparison",json!({"name":format!("実通信 {}",index)}));
                current=accepted(exchange(&mut app,&mut reader,&create).await)["input"].clone();
                let assign=request(&connected,&current,"assign_section",json!({"targets":app.studies.items[s].fixed,"section":candidates[index-1]}));
                current=accepted(exchange(&mut app,&mut reader,&assign).await)["input"].clone();
                let changes=request(&connected,&current,"changes",json!({"offset":0,"limit":100}));
                let changes=accepted(exchange(&mut app,&mut reader,&changes).await);
                assert_eq!(changes["total"],1);
                assert_eq!(changes["items"][0]["member"],target.0);
            }
            let calculate=request(&connected,&current,"calculate",Value::Null);
            let job=accepted(exchange(&mut app,&mut reader,&calculate).await);
            if index>0 {
                let busy=request(&connected,&current,"assign_section",json!({"targets":app.studies.items[s].fixed,"section":candidates[0]}));
                assert_eq!(exchange(&mut app,&mut reader,&busy).await.error.unwrap().code,"calculation_busy");
            }
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(180);
            let run_index=loop {
                let status=request(&connected,&current,"job",json!({"id":job["id"]}));
                let status=accepted(exchange(&mut app,&mut reader,&status).await);
                if status["state"]=="calculated" {break status["run_index"].as_u64().unwrap() as usize;}
                assert_eq!(status["state"],"running","{status}");
                assert!(std::time::Instant::now()<deadline);
            };
            let result=request(&connected,&current,"results",json!({"run_index":run_index,"offset":0,"limit":100}));
            let result=accepted(exchange(&mut app,&mut reader,&result).await);
            assert_eq!(result["state"],"calculated");
            assert!(result["whole_checks"]["observed_counts"]["checked"].as_u64().unwrap() > result["fixed_checks"]["observed_counts"]["checked"].as_u64().unwrap());
            let reference=result["page"]["items"].as_array().unwrap().iter().find(|r|r["kind"]=="check" && !r["reference"].is_null()).unwrap()["reference"].clone();
            let evidence=request(&connected,&current,"evidence",json!({"reference":reference,"table":"nodes","offset":0,"limit":100}));
            let evidence=accepted(exchange(&mut app,&mut reader,&evidence).await);
            assert!(evidence["page"]["total"].as_u64().unwrap() >= 166);
            let quantity_request=request(&connected,&current,"quantity",json!({"offset":0,"limit":100}));
            let quantity_output=accepted(exchange(&mut app,&mut reader,&quantity_request).await);
            assert_eq!(accepted(exchange(&mut app,&mut reader,&quantity_request).await),quantity_output);
            assert!(quantity_output["whole_quantity_error"].as_str().unwrap().contains("基礎梁の型枠対象側面"));
            close_numbers(&quantity_output["whole_scope_totals"]["steel_volume_m3"],&result["fixed_steel_volume"]["value_m3"]);
            let submit=request(&connected,&current,"submit",json!({"run_index":run_index,"explanation":"G43比較の計算時入力と既存検定を参照。全体型枠は未算定","unchecked":["人による採用","全体の型枠数量"],"references":[reference],"quantity_references":[quantity_output["identity"]]}));
            assert_eq!(accepted(exchange(&mut app,&mut reader,&submit).await)["submission"]["calculation_state"],"calculated");
            captured.push(result);
            let run=&app.studies.items[s].runs[run_index];
            let mut expected=fixture();
            if index>0 {
                assert!(expected.core.scoped.undo.run(&mut expected.core.model,Box::new(sepika_edit::SetElementSection {elem:target,section:Some(candidates[index-1])})));
            }
            assert!(run.input.model.eq_ignoring_dofmap(&expected.core.model));
            expected.run_preparation();
            assert_no_error(&expected,"手作業相当の準備計算");
            expected.run_static_all();
            assert_no_error(&expected,"手作業相当の全静的解析");
            let combo=expected.core.model.combinations.iter().position(|c|c.name=="保存長期架構G+P").unwrap();
            expected.run_combination(combo);
            expected.run_design_check();
            assert_no_error(&expected,"手作業相当の長期架構検定");
            let actual=run.results.as_ref().unwrap();
            let expected_results=expected.core.scoped.results.as_ref().unwrap();
            close_numbers(&json!(actual.combos),&json!(expected_results.combos));
            let checks=run.comparison_checks.iter().find(|e|serde_json::to_value(&e.bundle).unwrap()["design_source"]==json!({"Combo":"保存長期架構G+P"})).unwrap();
            close_numbers(&json!(checks.bundle.member_checks),&json!(expected_results.member_checks));
            let error=sepika_design_jp::quantity::try_compute_quantity_takeoff(&expected.core.model,&expected.core.model.quantity_cfg).unwrap_err();
            assert!(error.contains("基礎梁の型枠対象側面"));
            let quantity=sepika_design_jp::quantity::steel_members::try_compute_steel_members(&expected.core.model,&[target]).unwrap();
            let volume=quantity.iter().map(|item|item.volume_m3).sum::<f64>();
            assert_eq!(captured[index]["fixed_steel_volume"]["whole_quantity_error"],error);
            close_numbers(&captured[index]["fixed_steel_volume"]["value_m3"],&json!(volume));
            assert!(app.core.model.eq_ignoring_dofmap(&baseline));
        }
        let create=request(&connected,&baseline_input,"create_comparison",json!({"name":"実計算中の切断"}));
        let disconnected_input=accepted(exchange(&mut app,&mut reader,&create).await)["input"].clone();
        let calculate=request(&connected,&disconnected_input,"calculate",Value::Null);
        let disconnected_job=accepted(exchange(&mut app,&mut reader,&calculate).await);
        assert!(app.studies.running.is_some());
        drop(reader);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(180);
        while app.studies.running.is_some() || app.ai_connection.listener.is_some() {
            app.poll_study_calculation();
            app.poll_ai_connection();
            assert!(std::time::Instant::now()<deadline);
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(app.studies.items[s].runs.last().unwrap().ai_job.as_ref().unwrap(),disconnected_job["id"].as_str().unwrap());
        assert!(app.studies.items[s].runs.last().unwrap().error.is_none());
        assert!(app.ai_connection.permission.is_none());
        let review_endpoint=app.enable_ai_connection().unwrap();
        let mut reader=BufReader::new(pipe::connect(&review_endpoint).unwrap());
        assert_eq!(accepted(exchange(&mut app,&mut reader,&connect).await)["status"],"awaiting_gui_approval");
        app.approve_ai_connection(s,Mode::Review,true).unwrap();
        let review=accepted(exchange(&mut app,&mut reader,&connect).await);
        let old=request(&connected,&baseline_input,"study",Value::Null);
        assert_eq!(exchange(&mut app,&mut reader,&old).await.error.unwrap().code,"disconnected");
        let mut begin=request(&review,&baseline_input,"begin_request",Value::Null);
        begin.request=Some("issue985-review".into());
        accepted(exchange(&mut app,&mut reader,&begin).await);
        let mut forbidden=request(&review,&baseline_input,"create_comparison",json!({"name":"Reviewでは拒否"}));
        forbidden.request=Some("issue985-review".into());
        let before=app.studies.items[s].proposals.len();
        assert_eq!(exchange(&mut app,&mut reader,&forbidden).await.error.unwrap().code,"permission_denied");
        assert_eq!(app.studies.items[s].proposals.len(),before);
        let reference=captured[0]["page"]["items"].as_array().unwrap().iter().find(|r|r["kind"]=="check" && !r["reference"].is_null()).unwrap()["reference"].clone();
        let mut submit=request(&review,&baseline_input,"submit",json!({"run_index":0,"explanation":"同じ計算時入力・図表の根拠を確認。新しい計算と編集はしていない","unchecked":["設計全体の適否"],"references":[reference]}));
        submit.request=Some("issue985-review".into());
        assert_eq!(accepted(exchange(&mut app,&mut reader,&submit).await)["submission"]["run_index"],0);
        app.disconnect_ai_connection();
        let saved=rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        app.restore_studies(rmp_serde::from_slice(&saved).unwrap()).unwrap();
        assert_eq!(app.studies.items[s].runs.len(),4);
        assert_eq!(app.ai_observations(s).unwrap().tasks[0].submissions.len(),3);
        assert!(app.ai_observations(s).unwrap().tasks[0].submissions.iter().all(|submission|submission.quantity_references.len()==1));
        assert_eq!(app.ai_observations(s).unwrap().tasks[1].submissions.len(),1);
        assert!(app.ai_connection.permission.is_none());
        assert_eq!(app.handle_ai_request("old",old).error.unwrap().code,"permission_denied");
    });
}
