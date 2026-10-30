use super::*;
use crate::app::{study::*, App};
use references::{InputReference, ReferenceTarget, ResultReference, TargetReference};
use sepika_ai_connection::protocol::{Rejection, Request, Response, VERSION};
use serde_json::json;
mod requests;
#[cfg(test)]
mod tests;

fn reject(code: &str, message: impl Into<String>) -> Rejection {
    Rejection {
        code: code.into(),
        message: message.into(),
    }
}
fn args<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T, Rejection> {
    serde_json::from_value(request.arguments.clone())
        .map_err(|e| reject("invalid_arguments", e.to_string()))
}

const CHECK_COUNT_UNIT: &str = "既存検定の評価行（部材×位置×ケース、接合部・壁）";

fn verification_counts(
    summary: &comparison::projection::Summary,
    scope_available: bool,
    has_rows: bool,
) -> Verification {
    let scope_available = scope_available && has_rows;
    Verification {
        scope_available,
        checked: scope_available.then_some(summary.checked),
        unchecked: scope_available.then_some(summary.skipped),
        confirmed_ng: scope_available.then_some(summary.ng),
        observed_counts: has_rows.then(|| ObservedCounts {
            scope: if scope_available {
                "全取得範囲"
            } else {
                "取得できた有効根拠だけの部分集計。取得不能範囲は件数不明"
            }
            .into(),
            count_unit: CHECK_COUNT_UNIT.into(),
            checked: summary.checked,
            unchecked: summary.skipped,
            confirmed_ng: summary.ng,
        }),
    }
}

fn ai_check_rows(
    run: &StudyRun,
    fixed: &[ElemId],
) -> (Vec<comparison::projection::CheckRow>, Vec<String>) {
    let mut checks = Vec::new();
    let mut unavailable = Vec::new();
    if let Some(reason) = run.original_scope_error() {
        unavailable.push(reason);
    }
    if run.comparison_checks.is_empty() {
        unavailable.push("ケース別検定根拠がありません。未検定を0や合格へ置き換えません".into());
    }
    for evidence in &run.comparison_checks {
        match comparison::projection::fixed_coverage(evidence, fixed)
            .and_then(|()| comparison::projection::check_rows(evidence))
        {
            Ok(mut rows) => checks.append(&mut rows),
            Err(reason) => unavailable.push(reason),
        }
    }
    (checks, unavailable)
}
fn page(rows: Vec<Value>, offset: usize, limit: usize) -> Result<Value, Rejection> {
    if limit == 0 || limit > 100 || offset > rows.len() {
        return Err(reject(
            "invalid_page",
            "ページは0以上のoffset、1〜100のlimitで指定してください",
        ));
    }
    let total = rows.len();
    let items: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    Ok(
        json!({"total":total,"offset":offset,"limit":limit,"omitted": offset>0 || offset+items.len()<total,"next_offset":(offset+items.len()<total).then_some(offset+items.len()),"items":items}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    offset: usize,
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assign {
    targets: Vec<TargetReference>,
    section: SectionId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunPage {
    run_index: usize,
    offset: usize,
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submit {
    #[serde(default)]
    run_index: Option<usize>,
    explanation: String,
    unchecked: Vec<String>,
    references: Vec<ResultReference>,
    #[serde(default)]
    quantity_references: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    reference: ResultReference,
    table: String,
    offset: usize,
    limit: usize,
}

impl App {
    #[cfg(feature = "gui")]
    pub(crate) fn ai_connection_panel(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("外部AIへのローカル接続",|ui| {
            ui.label("接続手順: ①依頼を保存し依頼文をコピー ②ローカル接続を有効化 ③外部MCPクライアントにsepika-mcp.exeとコピーした --gui-pipe 引数を設定 ④クライアントから接続し、ここで検討と許可を確認 ⑤コピーした依頼文をクライアントへ渡す。依頼開始・成果提出は実際の受理後に表示します。");
            ui.label("外部クライアントがクラウドAIを利用すると、取得した情報が外部送信され得ます。クライアント側の保存・送信はSEPIKAの管理範囲外です。");
            ui.label("読取範囲: 許可した検討の基準・比較案の全体モデル、候補断面、条件、保持結果。固定対象以外の情報も含みます。");
            ui.label("編集範囲: 指定検討の比較案にある固定鉄骨大梁への既存有効断面の割当だけ。本案更新・採用は人が行います。");
            ui.label("クライアント切断で接続先と許可を終了します。再接続は手動で再有効化し、新しい起動引数とGUI承認を使ってください。受理済み計算は停止しません。");
            #[cfg(windows)]
            {
                if self.ai_connection.listener.is_none() {
                    if ui.add_enabled(!self.studies.items.is_empty(),egui::Button::new("ローカル接続を有効化")).clicked() {
                        if let Err(reason)=self.enable_ai_connection() {self.report_error(reason);}
                    }
                } else {
                    ui.strong("ローカル接続: 有効");
                    let endpoint=self.ai_connection.listener.as_ref().unwrap().name.clone();
                    ui.label(format!("接続先: {endpoint}"));
                    if ui.button("MCP起動引数をコピー").clicked() {ui.ctx().copy_text(format!("sepika-mcp --gui-pipe {endpoint}"));}
                    if ui.button("接続を解除").clicked() {self.disconnect_ai_connection();}
                    ui.label("解除後の新規要求は拒否します。受理済み計算は停止しません。");
                }
            }
            #[cfg(not(windows))]
            ui.label("ローカル接続はWindowsで利用できます。");
            if !self.studies.items.is_empty() {
                self.ai_connection.selected_study=self.ai_connection.selected_study.min(self.studies.items.len()-1);
                egui::ComboBox::from_id_salt("ai-permission-study").selected_text(&self.studies.items[self.ai_connection.selected_study].purpose).show_ui(ui,|ui|{
                    for (s,study) in self.studies.items.iter().enumerate() {ui.selectable_value(&mut self.ai_connection.selected_study,s,&study.purpose);}
                });
                ui.checkbox(&mut self.ai_connection.comparison,"比較案の作成・断面割当・既存計算を許可（未選択: 確認のみ）");
                ui.checkbox(&mut self.ai_connection.history,"この検討に保存された旧版の明示閲覧を許可");
            }
            if let Some(pending)=self.ai_connection.pending.clone() {
                ui.label(format!("承認待ちクライアント接続: {pending}"));
                if ui.button("このクライアントと対象検討を確認して許可").clicked() {
                    let mode=if self.ai_connection.comparison {Mode::Comparison} else {Mode::Review};
                    if let Err(reason)=self.approve_ai_connection(self.ai_connection.selected_study,mode,self.ai_connection.history) {self.report_error(reason);}
                }
            }
            if let Some(permission)=&self.ai_connection.permission {
                let purpose=self.studies.items.iter().find(|s|s.ai.id==permission.study).map(|s|s.purpose.as_str()).unwrap_or("取得不能");
                let mode=if permission.mode==Mode::Review {"確認のみ"} else {"比較案の作成・編集・計算"};
                ui.label(format!("許可中の検討: {purpose} / クライアント: {} / {mode} / 保存旧版閲覧: {}",permission.connection,permission.history));
            }
        });
    }
    /// 検討に保存された依頼・提出・受理台帳を返します。接続許可は含みません。
    pub fn ai_observations(&self, study: usize) -> Option<&StudyAi> {
        self.studies.items.get(study).map(|s| &s.ai)
    }

    #[cfg(windows)]
    pub fn enable_ai_connection(&mut self) -> Result<String, String> {
        self.disconnect_ai_connection();
        let listener =
            sepika_ai_connection::listener::Listener::start().map_err(|e| e.to_string())?;
        let name = listener.name.clone();
        self.ai_connection.listener = Some(listener);
        Ok(name)
    }

    pub fn disconnect_ai_connection(&mut self) {
        if let Some(permission) = &self.ai_connection.permission {
            let study_id = permission.study.clone();
            if let Some(study) = self.studies.items.iter_mut().find(|s| s.ai.id == study_id) {
                for task in &mut study.ai.tasks {
                    if task.started.is_some() {
                        task.interrupted =
                            Some((SystemTime::now(), "接続終了・新規操作の許可なし".into()));
                    }
                }
                self.mark_study_dirty();
            }
        }
        self.ai_connection = AiConnection::default();
    }

    /// GUIが受信済みクライアントと対象検討を確認した後にだけ呼び出します。
    pub fn approve_ai_connection(
        &mut self,
        study: usize,
        mode: Mode,
        history: bool,
    ) -> Result<(), String> {
        let connection = self
            .ai_connection
            .pending
            .take()
            .ok_or("接続確認を待つクライアントがありません")?;
        let item = self.studies.items.get(study).ok_or("検討がありません")?;
        self.ai_connection.permission = Some(Permission {
            connection,
            session: uuid::Uuid::new_v4().to_string(),
            project: item.baseline.references.project.clone(),
            study: item.ai.id.clone(),
            mode,
            history,
        });
        Ok(())
    }

    #[cfg(windows)]
    pub fn poll_ai_connection(&mut self) {
        self.sync_ai_jobs();
        loop {
            let event = self
                .ai_connection
                .listener
                .as_ref()
                .and_then(|l| l.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            match event {
                sepika_ai_connection::listener::Event::Request(incoming) => {
                    let response = self.handle_ai_request(&incoming.connection, incoming.request);
                    let _ = incoming.response.send(response);
                }
                sepika_ai_connection::listener::Event::Disconnected(_) => {
                    self.disconnect_ai_connection();
                }
            }
        }
    }

    pub fn handle_ai_request(&mut self, connection: &str, request: Request) -> Response {
        let id = request.id.clone();
        match self.process_ai_request(connection, request) {
            Ok(value) => Response::accepted(id, value),
            Err(error) => Response {
                id,
                result: None,
                error: Some(error),
            },
        }
    }

    fn process_ai_request(
        &mut self,
        connection: &str,
        request: Request,
    ) -> Result<Value, Rejection> {
        self.sync_ai_jobs();
        if request.version != VERSION {
            return Err(reject("unsupported_version", "プロトコル版が一致しません"));
        }
        if request.id.is_empty() || request.id.len() > 128 {
            return Err(reject(
                "invalid_id",
                "要求IDは1〜128バイトで指定してください",
            ));
        }
        if request.operation == "connect" {
            if let Some(permission) = self
                .ai_connection
                .permission
                .as_ref()
                .filter(|p| p.connection == connection)
            {
                let study = self
                    .studies
                    .items
                    .iter()
                    .position(|s| s.ai.id == permission.study)
                    .ok_or_else(|| reject("study_missing", "許可した検討がありません"))?;
                return Ok(
                    json!({"version":VERSION,"status":"approved","session":permission.session,"project":permission.project,"study":permission.study,"mode":permission.mode,"history":permission.history,"read_scope":"許可した検討の基準・比較案の全体モデル、条件、候補断面、保持結果","edit_scope":"比較案の固定鉄骨大梁の既存有効断面割当のみ","baseline":self.ai_proposal_reference(study,0),"proposal_count":self.studies.items[study].proposals.len()}),
                );
            }
            self.ai_connection.pending = Some(connection.into());
            return Ok(json!({"version":VERSION,"status":"awaiting_gui_approval"}));
        }
        let permission = self
            .ai_connection
            .permission
            .as_ref()
            .ok_or_else(|| reject("permission_denied", "GUIの接続許可がありません"))?;
        if permission.connection != connection
            || request.session.as_ref() != Some(&permission.session)
        {
            return Err(reject("disconnected", "接続セッションが失効しています"));
        }
        if request.project.as_ref() != Some(&permission.project)
            || request.study.as_ref() != Some(&permission.study)
            || self.core.scoped.references.project != permission.project
        {
            return Err(reject(
                "scope_mismatch",
                "許可したプロジェクト・検討と一致しません",
            ));
        }
        let s = self
            .studies
            .items
            .iter()
            .position(|s| s.ai.id == permission.study)
            .ok_or_else(|| reject("study_missing", "検討がありません"))?;
        let mode = permission.mode;
        let history = permission.history;
        if !history && self.ai_history_required(s, request.request.as_deref(), None) {
            return Err(reject(
                "permission_denied",
                "保存入力を明示した依頼には、現在の接続で保存旧版の閲覧許可が必要です",
            ));
        }
        let mutates = matches!(
            request.operation.as_str(),
            "create_comparison" | "assign_section" | "calculate"
        );
        if mutates && mode != Mode::Comparison {
            return Err(reject(
                "permission_denied",
                "確認のみの接続では作成・編集・計算を実行できません",
            ));
        }
        let mut content = serde_json::to_value(&request).unwrap();
        content.as_object_mut().unwrap().remove("session");
        if let Some(receipt) = self.studies.items[s]
            .ai
            .receipts
            .iter()
            .find(|r| r.id == request.id)
        {
            if receipt.content != content {
                return Err(reject(
                    "request_id_conflict",
                    "同じ要求IDを異なる内容に再利用できません",
                ));
            }
            self.ai_validate_receipt(s, receipt, history)?;
            return Ok(receipt.result.clone());
        }
        let p = self.ai_proposal_index(s, request.proposal.as_deref())?;
        let current = self.ai_proposal_reference(s, p);
        let expected: InputReference = serde_json::from_value(
            request
                .input
                .clone()
                .ok_or_else(|| reject("input_required", "対象の入力版を明示してください"))?,
        )
        .map_err(|e| reject("invalid_input", e.to_string()))?;
        if expected.project != current.project || expected.proposal != current.proposal {
            return Err(reject(
                "scope_mismatch",
                "対象の案・プロジェクトが一致しません",
            ));
        }
        let old = !self.ai_input_is_current(s, &expected);
        if (old && mutates)
            || (!history
                && self.ai_history_required(s, request.request.as_deref(), Some(&expected)))
        {
            return Err(reject(
                "input_conflict",
                "対象の入力版が現在版と一致しません。許可した保存旧版の明示閲覧だけが可能です",
            ));
        }
        if old
            && !self.studies.items[s].runs.iter().any(|r| {
                Self::ai_input_reference(&r.input) == expected
                    || Self::ai_input_reference(&r.prepared) == expected
            })
            && !self.studies.items[s]
                .ai
                .tasks
                .iter()
                .any(|task| Self::ai_input_reference(&task.origin) == expected)
        {
            return Err(reject("input_missing", "明示した保存入力版がありません"));
        }
        let task_id = request
            .request
            .as_deref()
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| reject("request_required", "依頼IDを指定してください"))?;
        if request.operation != "begin_request"
            && request.operation != "receipt"
            && !self.studies.items[s]
                .ai
                .tasks
                .iter()
                .any(|t| t.id == task_id && t.started.is_some())
        {
            return Err(reject(
                "request_not_started",
                "先に依頼開始を明示してください",
            ));
        }
        if mutates {
            let task = self.studies.items[s]
                .ai
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .ok_or_else(|| reject("request_not_started", "先に依頼開始を明示してください"))?;
            if task.mode != Mode::Comparison
                || task.fixed != self.studies.items[s].fixed
                || task.baseline != Self::ai_input_reference(&self.studies.items[s].baseline)
            {
                return Err(reject("permission_denied","この依頼は比較編集を許可されていないか、開始時の固定対象・条件が変わっています"));
            }
        }
        let output=match request.operation.as_str() {
            "begin_request"=> {
                if old && !self.studies.items[s].ai.tasks.iter().any(|t|t.id==task_id && t.started.is_none() && t.mode==Mode::Review && t.historical && t.started_input==expected) {return Err(reject("input_conflict","対象の入力版が変わりました。保存入力を明示して依頼を再確認してください"));}
                let baseline=Self::ai_input_reference(&self.studies.items[s].baseline);
                let fixed=self.studies.items[s].fixed.clone();
                if self.studies.items[s].ai.tasks.iter().find(|task|task.id==task_id && task.started.is_none()).is_some_and(|task|!self.ai_task_source_current(task)) {return Err(reject("input_conflict","依頼元の本案の入力版・条件が変わりました。元入力と対象を再確認して依頼を作成してください"));}
                if let Some(task)=self.studies.items[s].ai.tasks.iter_mut().find(|t|t.id==task_id) {
                    if task.started.is_some() {return Err(reject("request_exists","依頼IDは既に開始済みです"));}
                    if task.mode != mode || task.started_input != expected || task.baseline != baseline || (task.mode == Mode::Comparison && task.fixed != fixed) {return Err(reject("request_conflict","準備した依頼の種別・元案版・基準・対象と許可が一致しません。依頼を再確認してください"));}
                    task.started=Some(SystemTime::now());
                    task.interrupted=None;
                } else {
                    let purpose=self.studies.items[s].purpose.clone();
                    let origin=self.proposal_input(s,p);
                    let source_app=App{core:origin.core(),..Default::default()};
                    let source_targets=fixed.iter().map(|target|source_app.target_reference(target.target).map_err(|reason|reject("target_invalid",reason))).collect::<Result<Vec<_>,_>>()?;
                    let proposal_name=self.studies.items[s].proposals[p].name.clone();
                    self.studies.items[s].ai.tasks.push(Task{id:task_id.into(),mode,baseline,started_input:current.clone(),origin,from_main:false,source_targets,historical:false,fixed,purpose,note:String::new(),proposal_name,conditions:"保存した案の解析・検定設定、全ケース・全組合せ".into(),frame:"指定なし".into(),results:Vec::new(),display:Default::default(),quantity:None,prepared:SystemTime::now(),started:Some(SystemTime::now()),interrupted:None,last_operation:SystemTime::now(),last_operation_name:"begin_request".into(),submissions:Vec::new()});
                }
                let task=self.studies.items[s].ai.tasks.iter().find(|t|t.id==task_id).unwrap();
                json!({"status":"started","request":task_id,"mode":task.mode,"input":expected,"purpose":task.purpose,"fixed":task.fixed,"result_references":task.results,"display":task.display,"quantity_identity":task.quantity.as_ref().map(|q|&q.identity),"quantity_scope":task.quantity.as_ref().map(|q|&q.scope)})
            }
            "study"=> {
                let task=self.studies.items[s].ai.tasks.iter().find(|t|t.id==task_id).unwrap();
                json!({"purpose":task.purpose,"notes":self.studies.items[s].notes,"fixed_count":task.fixed.len(),"proposal_count":self.studies.items[s].proposals.len(),"decision_count":self.studies.items[s].decisions.len(),"input":expected,"request":task.id,"mode":task.mode,"read_scope":"当該検討の全体モデル・条件・結果","edit_scope":if task.mode==Mode::Review {"編集・計算不可"}else{"比較案の固定鉄骨大梁の既存有効断面割当のみ"}})
            },
            "proposals" | "fixed_targets" | "decisions" => {
                let pagination:Page=args(&request)?;
                let rows=match request.operation.as_str() {
                    "proposals"=>self.ai_proposals(s).as_array().unwrap().clone(),
                    "fixed_targets"=>self.studies.items[s].ai.tasks.iter().find(|t|t.id==task_id).unwrap().fixed.iter().map(|v|json!(v)).collect(),
                    _=>self.studies.items[s].decisions.iter().map(|v|json!({"accepted":v.accepted,"reason":v.reason,"created":v.created,"applied":v.is_applied()})).collect(),
                };
                page(rows,pagination.offset,pagination.limit)?
            }
            "conditions"=> {
                let input=self.ai_read_input(s,p,&expected)?;
                json!({"input":expected,"settings":input.settings,"load_case_count":input.model.load_cases.len(),"combination_count":input.model.combinations.len(),"calculation":"全ケース・全組合せの既存準備計算・線形静的解析・対応検定・数量。断面依存自重等は再算定"})
            }
            "members"=> {
                let pagination:Page=args(&request)?;
                let input=self.ai_read_input(s,p,&expected)?;
                page(input.model.elements.iter().map(|e|json!(e)).collect(),pagination.offset,pagination.limit)?
            }
            "nodes" | "materials" | "load_cases" | "combinations" => {
                let pagination:Page=args(&request)?;
                let input=self.ai_read_input(s,p,&expected)?;
                let rows=match request.operation.as_str() {
                    "nodes"=>input.model.nodes.iter().map(|v|json!(v)).collect(),
                    "materials"=>input.model.materials.iter().map(|v|json!(v)).collect(),
                    "load_cases"=>input.model.load_cases.iter().map(|v|json!(v)).collect(),
                    _=>input.model.combinations.iter().map(|v|json!(v)).collect(),
                };
                page(rows,pagination.offset,pagination.limit)?
            }
            "sections"=> {
                let pagination:Page=args(&request)?;
                let input=self.ai_read_input(s,p,&expected)?;
                page(input.model.sections.iter().filter(|section|sepika_edit::valid_study_section(&input.model,section)).map(|section|json!(section)).collect(),pagination.offset,pagination.limit)?
            }
            "create_comparison"=> {
                if p!=0 {return Err(reject("scope_mismatch","比較案作成元には基準案を指定してください"));}
                let name:Name=args(&request)?;
                if name.name.trim().is_empty() || name.name.len()>1024 {return Err(reject("invalid_arguments","案名を1〜1024バイトで指定してください"));}
                let new=self.create_comparison(s,name.name).map_err(|e|reject("edit_rejected",e))?;
                json!({"input":self.ai_proposal_reference(s,new)})
            }
            "assign_section"=>self.ai_assign(s,p,&request)?,
            "calculate"=> {
                if old {return Err(reject("input_conflict","計算開始には現在入力版を指定してください"));}
                if !request.arguments.is_null() && request.arguments!=json!({}) {return Err(reject("unsupported","計算条件の上書きには対応していません"));}
                self.start_study_calculation(s,p).map_err(|e|reject("calculation_busy",e))?;
                self.studies.running.as_mut().expect("開始した計算").ai_job=Some(request.id.clone());
                let job=AiJob {id:request.id.clone(),request:task_id.into(),input:current.clone(),state:"running".into(),error:None,run_index:None};
                self.studies.items[s].ai.jobs.push(job.clone());
                json!(job)
            }
            "job"=> {
                let id:Id=args(&request)?;
                let job=self.studies.items[s].ai.jobs.iter().find(|j|j.id==id.id && j.request==task_id && j.input==expected).ok_or_else(||reject("job_missing","この依頼・案・入力版に属するジョブがありません"))?;
                json!(job)
            }
            "receipt"=> {
                let id:Id=args(&request)?;
                let receipt=self.studies.items[s].ai.receipts.iter().find(|r|r.id==id.id && r.content.get("request").and_then(Value::as_str)==Some(task_id)).ok_or_else(||reject("receipt_missing","受理済み要求がありません。未受理か応答不明であり変更の成否を推定しないでください"))?;
                self.ai_validate_receipt(s,receipt,history)?;
                json!(receipt)
            }
            "changes"=> {
                let pagination:Page=args(&request)?;
                let input=self.ai_read_input(s,p,&expected)?;
                let baseline=&self.studies.items[s].baseline;
                let rows=input.model.elements.iter().filter_map(|e|baseline.model.element(e.id).filter(|base|base.section!=e.section).map(|base|json!({"member":e.id,"before":base.section,"after":e.section,"unit":"断面識別"}))).collect();
                page(rows,pagination.offset,pagination.limit)?
            }
            "results"=>self.ai_results(s,p,&expected,&request,history)?,
            "runs"=> {
                let pagination:Page=args(&request)?;
                self.ai_run_metadata(s,p,&expected,pagination,history)?
            }
            "quantity"=> {
                let pagination:Page=args(&request)?;
                let quantity=self.ai_quantity_reference(s,p,task_id,&expected)?;
                if matches!(quantity.scope,references::QuantityEvidenceScope::Members(_)) {
                    let steel=quantity.steel_takeoff().map_err(|e|reject("quantity_unavailable",e))?;
                    let volume=steel.volume_m3();
                    let rows=steel.items.iter().map(|item|json!({"member":item.elem,"section":item.section,"steel_t":item.steel.weight_t,"label":item.steel.section_name,"steel_volume_m3":item.volume_m3,"area_mm2":item.area_mm2,"area_basis":item.area_basis,"length_mm":item.length_mm})).collect();
                    json!({"source":quantity.source,"identity":quantity.identity,"scope":quantity.scope.label(),"scope_members":steel.items.iter().map(|item|item.elem).collect::<Vec<_>>(),"contract":quantity.contract,"components":"鋼材のみ","whole_quantity_error":steel.whole_quantity_error,"whole_scope_totals":{"steel_t":volume*sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3,"steel_volume_m3":volume,"members":steel.items.len()},"page":page(rows,pagination.offset,pagination.limit)?})
                } else {
                let takeoff=quantity.takeoff().map_err(|e|reject("quantity_unavailable",e))?;
                let totals=takeoff.totals();
                let rows=takeoff.items.iter().map(|item|json!({"member":item.elem,"section":item.section,"steel_t":item.steel_weight_t(),"label":item.label,"steel_volume_m3":item.steel_weight_t()/sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3})).collect();
                let scope_members=match &quantity.scope {references::QuantityEvidenceScope::WholeModel=>None,references::QuantityEvidenceScope::Members(members)=>Some(members)};
                json!({"source":quantity.source,"identity":quantity.identity,"scope":quantity.scope.label(),"scope_members":scope_members,"contract":quantity.contract,"whole_scope_totals":{"steel_t":totals.steel_t,"steel_volume_m3":totals.steel_t/sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3,"members":totals.member_count},"page":page(rows,pagination.offset,pagination.limit)?})
                }
            }
            "evidence"=> {
                let evidence:Evidence=args(&request)?;
                if matches!(evidence.table.as_str(),"design_details"|"joint_details"|"wall_details") && evidence.reference.kind!=ResultInputKey::Design {
                    return Err(reject("result_kind_mismatch","検定詳細には対応するDesign結果参照を指定してください"));
                }
                self.ai_validate_reference(s,&expected,&evidence.reference,history)?;
                let view=self.ai_result_view(s,&evidence.reference)?;
                let recorded=view.recorded_result_input(&evidence.reference).map_err(|e|reject("result_unavailable",e))?;
                let rows=match evidence.table.as_str() {
                    "nodes"=>recorded.model.nodes.iter().map(|v|json!(v)).collect(),
                    "members"=>recorded.model.elements.iter().map(|v|json!(v)).collect(),
                    "sections"=>recorded.model.sections.iter().map(|v|json!(v)).collect(),
                    "materials"=>recorded.model.materials.iter().map(|v|json!(v)).collect(),
                    "load_cases"=>recorded.model.load_cases.iter().map(|v|json!(v)).collect(),
                    "combinations"=>recorded.model.combinations.iter().map(|v|json!(v)).collect(),
                    "design_details"=>view.core.scoped.results.as_ref().map(|b|b.member_checks.iter().flat_map(|v|v.positions.iter().map(move |position|json!({"member":v.elem,"xi":position.xi,"outcome":position.outcome}))).collect()).unwrap_or_default(),
                    "joint_details"=>view.core.scoped.results.as_ref().map(|b|b.joint_checks.iter().map(|v|json!(v)).collect()).unwrap_or_default(),
                    "wall_details"=>view.core.scoped.results.as_ref().map(|b|b.wall_checks.iter().map(|v|json!(v)).collect()).unwrap_or_default(),
                    _=>return Err(reject("unsupported_table","根拠表の種類が未対応です")),
                };
                json!({"reference":evidence.reference,"scope":"計算時の全体モデル","units":{"length":"mm","force":"N","stress":"N/mm²"},"page":page(rows,evidence.offset,evidence.limit)?})
            }
            "submit"=> {
                let submission:Submit=args(&request)?;
                if submission.explanation.trim().is_empty() {return Err(reject("invalid_arguments","説明を指定してください"));}
                for reference in &submission.references {self.ai_validate_reference(s,&expected,reference,history)?;}
                let (state,job_id)=if let Some(index)=submission.run_index {
                    let run=self.studies.items[s].runs.get(index).ok_or_else(||reject("result_missing","提出する保持計算がありません"))?;
                    if Self::ai_input_reference(&run.input)!=expected && Self::ai_input_reference(&run.prepared)!=expected {return Err(reject("result_scope_mismatch","提出する保持計算の案・入力版が一致しません"));}
                    if submission.references.iter().any(|reference|run.referenced_result(reference).is_none()) {return Err(reject("submission_run_mismatch","結果参照は指定した一つの保持計算にそろえてください"));}
                    (run.calculation_state().to_string(),run.ai_job.clone())
                } else {
                    if !submission.references.is_empty() {return Err(reject("run_required","結果参照を提出する場合は元のrun_indexを明示してください"));}
                    self.studies.items[s].ai.jobs.iter().rev().find(|job|job.request==task_id && job.input==expected).map(|job|(job.state.clone(),Some(job.id.clone()))).unwrap_or_else(||("not_calculated".into(),None))
                };
                let mut quantities=Vec::new();
                for identity in &submission.quantity_references {
                    let quantity=self.ai_quantity_reference(s,p,task_id,&expected)?;
                    if *identity!=quantity.identity {return Err(reject("quantity_reference_invalid","対象入力版と固定対象の数量根拠が一致しません"));}
                    quantity.validate_available().map_err(|e|reject("quantity_unavailable",e))?;
                    quantities.push(quantity);
                }
                let run_index=submission.run_index;
                let verification=self.ai_verification_at(s,run_index);
                let submitted=Submission{receipt_id:request.id.clone(),run_index,job_id,input:expected.clone(),explanation:submission.explanation,unchecked:submission.unchecked,references:submission.references,quantity_references:quantities,calculation_state:state,verification,submitted:SystemTime::now()};
                self.studies.items[s].ai.tasks.iter_mut().find(|t|t.id==task_id).unwrap().submissions.push(submitted.clone());
                json!({"status":"submitted","submission":{"input":submitted.input,"source":if submitted.run_index.is_some(){"run"}else if submitted.job_id.is_some(){"job"}else{"none"},"run_index":submitted.run_index,"job_id":submitted.job_id,"calculation_state":submitted.calculation_state,"verification":submitted.verification,"result_reference_count":submitted.references.len(),"quantity_reference_count":submitted.quantity_references.len(),"submitted":submitted.submitted},"notice":"状態は提出時に選んだ元の観測です。成果提出は計算成功・検定合格・人による採用とは別です"})
            }
            _=>return Err(reject("unsupported_operation","この操作は公開していません。本案更新・採用・警告承諾・取消・任意ファイル操作は実行できません")),
        };
        if let Some(task) = self.studies.items[s]
            .ai
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
        {
            task.last_operation = SystemTime::now();
            task.last_operation_name = request.operation.clone();
            task.interrupted = None;
        }
        self.studies.items[s].ai.receipts.push(Receipt {
            id: request.id,
            content,
            result: output.clone(),
        });
        self.mark_study_dirty();
        Ok(output)
    }

    fn ai_proposal_index(&self, s: usize, proposal: Option<&str>) -> Result<usize, Rejection> {
        self.studies.items[s]
            .proposals
            .iter()
            .enumerate()
            .find_map(|(p, _)| {
                (Some(self.ai_proposal_reference(s, p).proposal.as_str()) == proposal).then_some(p)
            })
            .ok_or_else(|| {
                reject(
                    "proposal_missing",
                    "この検討に属する対象案がありません。本案は操作できません",
                )
            })
    }
    fn ai_proposal_reference(&self, s: usize, p: usize) -> InputReference {
        if self.studies.active == Location::Proposal(s, p) {
            self.input_reference()
        } else {
            let app = App {
                core: self.proposal_input(s, p).core(),
                ..Default::default()
            };
            app.input_reference()
        }
    }
    fn ai_input_reference(input: &StudyInput) -> InputReference {
        input.input_reference()
    }
    fn ai_proposals(&self, s: usize) -> Value {
        Value::Array(self.studies.items[s].proposals.iter().enumerate().map(|(p,proposal)|json!({"name":proposal.name,"baseline":p==0,"input":self.ai_proposal_reference(s,p)})).collect())
    }
    fn ai_read_input(
        &self,
        s: usize,
        p: usize,
        expected: &InputReference,
    ) -> Result<StudyInput, Rejection> {
        if self.ai_proposal_reference(s, p) == *expected {
            return Ok(self.proposal_input(s, p));
        }
        self.studies.items[s]
            .runs
            .iter()
            .flat_map(|r| [&r.input, &r.prepared])
            .chain(
                self.studies.items[s]
                    .ai
                    .tasks
                    .iter()
                    .map(|task| &task.origin),
            )
            .find(|input| Self::ai_input_reference(input) == *expected)
            .cloned()
            .ok_or_else(|| reject("input_missing", "保存入力版がありません"))
    }

    fn ai_input_is_current(&self, s: usize, input: &InputReference) -> bool {
        if !self.studies.items[s]
            .proposals
            .iter()
            .enumerate()
            .any(|(p, _)| self.ai_proposal_reference(s, p) == *input)
        {
            return false;
        }
        let main = self.main_study_input().input_reference();
        input.proposal != main.proposal || *input == main
    }

    fn ai_history_required(
        &self,
        s: usize,
        task_id: Option<&str>,
        input: Option<&InputReference>,
    ) -> bool {
        self.studies.items[s]
            .ai
            .tasks
            .iter()
            .find(|task| Some(task.id.as_str()) == task_id)
            .is_some_and(|task| task.historical)
            || input.is_some_and(|input| !self.ai_input_is_current(s, input))
    }

    fn ai_quantity_reference(
        &self,
        s: usize,
        p: usize,
        task_id: &str,
        expected: &InputReference,
    ) -> Result<references::QuantityEvidenceReference, Rejection> {
        if let Some(task) = self.studies.items[s]
            .ai
            .tasks
            .iter()
            .find(|task| task.id == task_id)
        {
            if let Some(quantity) = &task.quantity {
                if task.mode != Mode::Review
                    || quantity.source != *expected
                    || quantity.contract != references::QUANTITY_EVIDENCE_CONTRACT_VERSION
                    || quantity.identity() != quantity.identity
                {
                    return Err(reject(
                        "quantity_reference_invalid",
                        "依頼に固定した数量根拠の入力版・範囲・契約が一致しません",
                    ));
                }
                return Ok(quantity.clone());
            }
        }
        let input = self.ai_read_input(s, p, expected)?;
        let app = App {
            core: input.core(),
            ..Default::default()
        };
        Ok(
            app.member_quantity_evidence_reference(comparison::projection::fixed_members(
                &self.studies.items[s],
            )),
        )
    }

    fn ai_validate_receipt(
        &self,
        s: usize,
        receipt: &Receipt,
        history: bool,
    ) -> Result<(), Rejection> {
        let mut receipt = receipt;
        for _ in 0..=self.studies.items[s].ai.receipts.len() {
            let request: Request = serde_json::from_value(receipt.content.clone())
                .map_err(|e| reject("receipt_invalid", e.to_string()))?;
            if !history && self.ai_history_required(s, request.request.as_deref(), None) {
                return Err(reject(
                    "permission_denied",
                    "保存入力由来の受理記録には、現在の保存旧版閲覧許可が必要です",
                ));
            }
            if matches!(
                request.operation.as_str(),
                "begin_request" | "create_comparison" | "assign_section" | "calculate"
            ) {
                return Ok(());
            }
            let p = self.ai_proposal_index(s, request.proposal.as_deref())?;
            let expected: InputReference = serde_json::from_value(
                request
                    .input
                    .clone()
                    .ok_or_else(|| reject("receipt_invalid", "受理記録の入力版がありません"))?,
            )
            .map_err(|e| reject("receipt_invalid", e.to_string()))?;
            if !history && self.ai_history_required(s, request.request.as_deref(), Some(&expected))
            {
                return Err(reject(
                    "permission_denied",
                    "この接続では受理済み応答内の保存旧版を閲覧できません",
                ));
            }
            if !matches!(request.operation.as_str(), "submit" | "runs") {
                self.ai_read_input(s, p, &expected)?;
            }
            match request.operation.as_str() {
                "runs" => {
                    if receipt.result.get("history_included") == Some(&Value::Bool(true))
                        && !history
                    {
                        return Err(reject(
                            "permission_denied",
                            "この接続では保存旧版を含む保持計算一覧を閲覧できません",
                        ));
                    }
                    let rows = receipt
                        .result
                        .get("page")
                        .and_then(|page| page.get("items"))
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            reject("receipt_invalid", "保持計算一覧の受理記録が不正です")
                        })?;
                    for row in rows {
                        let index = row
                            .get("run_index")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| reject("receipt_invalid", "保持計算位置が不正です"))?;
                        let run = self.studies.items[s].runs.get(index).ok_or_else(|| {
                            reject("result_missing", "保持計算一覧の元計算がありません")
                        })?;
                        if run.proposal != expected.proposal
                            || Self::ai_run_metadata_row(index, run, &expected) != *row
                        {
                            return Err(reject(
                                "result_unavailable",
                                "保持計算一覧の元の案・入力版・実行記録が変わっています",
                            ));
                        }
                        if !history
                            && Self::ai_input_reference(&run.input)
                                != self.ai_proposal_reference(s, p)
                            && Self::ai_input_reference(&run.prepared)
                                != self.ai_proposal_reference(s, p)
                        {
                            return Err(reject(
                                "permission_denied",
                                "この接続では保存旧版の保持計算を閲覧できません",
                            ));
                        }
                    }
                    return Ok(());
                }
                "submit" => {
                    self.ai_validate_submission(s, receipt, &expected, history)?;
                    return Ok(());
                }
                "receipt" => {
                    let id: Id = args(&request)?;
                    receipt = self.studies.items[s]
                        .ai
                        .receipts
                        .iter()
                        .find(|r| r.id == id.id)
                        .ok_or_else(|| reject("receipt_missing", "照会先の受理記録がありません"))?;
                }
                "results" => {
                    let current = self.ai_results(s, p, &expected, &request, history)?;
                    for key in [
                        "state",
                        "conditions",
                        "error",
                        "notice",
                        "unavailable",
                        "whole_checks",
                        "fixed_checks",
                        "fixed_steel_volume",
                        "page",
                    ] {
                        if current.get(key) != receipt.result.get(key) {
                            return Err(reject(
                                "result_unavailable",
                                "受理済み応答の元の計算根拠が失効・変化しています",
                            ));
                        }
                    }
                    return Ok(());
                }
                "evidence" => {
                    let evidence: Evidence = args(&request)?;
                    if matches!(
                        evidence.table.as_str(),
                        "design_details" | "joint_details" | "wall_details"
                    ) && evidence.reference.kind != ResultInputKey::Design
                    {
                        return Err(reject(
                            "result_kind_mismatch",
                            "検定詳細には対応するDesign結果参照を指定してください",
                        ));
                    }
                    self.ai_validate_reference(s, &expected, &evidence.reference, history)?;
                    return Ok(());
                }
                "quantity" => {
                    let quantity = self.ai_quantity_reference(
                        s,
                        p,
                        request.request.as_deref().ok_or_else(|| {
                            reject("receipt_invalid", "受理記録の依頼IDがありません")
                        })?,
                        &expected,
                    )?;
                    if receipt.result.get("identity") != Some(&json!(quantity.identity))
                        || receipt.result.get("contract") != Some(&json!(quantity.contract))
                    {
                        return Err(reject(
                            "result_unavailable",
                            "受理済み数量根拠の入力・範囲・契約が失効しています",
                        ));
                    }
                    quantity
                        .validate_available()
                        .map_err(|e| reject("quantity_unavailable", e))?;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        Err(reject(
            "receipt_invalid",
            "受理記録の照会関係が循環しています",
        ))
    }

    fn ai_assign(&mut self, s: usize, p: usize, request: &Request) -> Result<Value, Rejection> {
        if p == 0 {
            return Err(reject(
                "permission_denied",
                "基準案の編集は許可されていません",
            ));
        }
        let proposal = self.ai_proposal_reference(s, p).proposal;
        if self
            .studies
            .running
            .as_ref()
            .is_some_and(|r| r.study == s && r.input.references.proposal == proposal)
        {
            return Err(reject("calculation_busy", "この案は計算中です"));
        }
        let assign: Assign = args(request)?;
        if assign.targets.is_empty() {
            return Err(reject("target_invalid", "固定対象を明示してください"));
        }
        let input = self.proposal_input(s, p);
        if !comparison::projection::same_conditions(
            &self.studies.items[s].baseline,
            &input,
            &comparison::projection::fixed_members(&self.studies.items[s]),
        ) {
            return Err(reject(
                "conditions_mismatch",
                "固定対象の断面以外に基準条件との差があります",
            ));
        }
        if !input
            .model
            .section(assign.section)
            .is_some_and(|section| sepika_edit::valid_study_section(&input.model, section))
        {
            return Err(reject(
                "section_invalid",
                "既存の有効な鉄骨大梁断面ではありません",
            ));
        }
        let mut ids = Vec::new();
        for target in &assign.targets {
            let ReferenceTarget::Member(id) = self
                .resolve_study_fixed_target(s, target)
                .map_err(|e| reject("target_invalid", e))?
            else {
                return Err(reject("target_invalid", "固定鉄骨大梁以外は編集できません"));
            };
            if ids.contains(&id) {
                return Err(reject("target_invalid", "固定対象が重複しています"));
            }
            ids.push(id);
        }
        let command = Box::new(sepika_edit::CompositeCommand {
            label: "AIによる比較案の断面割当".into(),
            children: ids
                .iter()
                .map(|id| {
                    Box::new(sepika_edit::SetElementSection {
                        elem: *id,
                        section: Some(assign.section),
                    }) as Box<dyn sepika_edit::EditCommand>
                })
                .collect(),
        });
        let core = if self.studies.active == Location::Proposal(s, p) {
            &mut self.core
        } else {
            &mut self.studies.items[s].proposals[p]
                .state
                .as_mut()
                .unwrap()
                .core
        };
        let applied = core.scoped.undo.run(&mut core.model, command);
        if let Some(reason) = core.scoped.undo.last_error() {
            return Err(reject("edit_rejected", reason));
        }
        if applied {
            core.scoped.staleness.mark_edited();
        }
        Ok(
            json!({"applied":applied,"members":ids,"section":assign.section,"input":self.ai_proposal_reference(s,p)}),
        )
    }

    #[cfg(test)]
    fn ai_calculation_state(&self, s: usize, input: &InputReference) -> String {
        self.studies.items[s]
            .ai
            .jobs
            .iter()
            .rev()
            .find(|j| j.input == *input)
            .map(|j| j.state.clone())
            .unwrap_or_else(|| {
                self.studies.items[s]
                    .runs
                    .iter()
                    .rev()
                    .find(|run| {
                        Self::ai_input_reference(&run.input) == *input
                            || Self::ai_input_reference(&run.prepared) == *input
                    })
                    .map(|r| {
                        if r.error.is_some() || r.results.is_none() {
                            "failed".into()
                        } else {
                            "calculated".into()
                        }
                    })
                    .unwrap_or_else(|| "not_calculated".into())
            })
    }

    #[cfg(test)]
    fn ai_latest_run_index(&self, s: usize, input: &InputReference) -> Option<usize> {
        self.studies.items[s].runs.iter().rposition(|r| {
            Self::ai_input_reference(&r.input) == *input
                || Self::ai_input_reference(&r.prepared) == *input
        })
    }

    #[cfg(test)]
    fn ai_verification(&self, s: usize, input: &InputReference) -> Verification {
        self.ai_verification_at(s, self.ai_latest_run_index(s, input))
    }

    fn ai_verification_at(&self, s: usize, run_index: Option<usize>) -> Verification {
        let Some(run) = run_index.and_then(|r| self.studies.items[s].runs.get(r)) else {
            return verification_counts(&comparison::projection::Summary::default(), false, false);
        };
        let fixed = comparison::projection::fixed_members(&self.studies.items[s]);
        let (checks, unavailable) = ai_check_rows(run, &fixed);
        verification_counts(
            &comparison::projection::summarize(checks.iter()),
            unavailable.is_empty(),
            !checks.is_empty(),
        )
    }

    fn ai_validate_submission(
        &self,
        s: usize,
        receipt: &Receipt,
        expected: &InputReference,
        history: bool,
    ) -> Result<(), Rejection> {
        let submission = self.studies.items[s]
            .ai
            .tasks
            .iter()
            .flat_map(|task| &task.submissions)
            .find(|submission| submission.receipt_id == receipt.id && submission.input == *expected)
            .ok_or_else(|| reject("submission_missing", "元の受理済み提出がありません"))?;
        for reference in &submission.references {
            self.ai_validate_reference(s, expected, reference, history)?;
        }
        for quantity in &submission.quantity_references {
            if quantity.source != *expected
                || quantity.contract != references::QUANTITY_EVIDENCE_CONTRACT_VERSION
                || quantity.identity() != quantity.identity
            {
                return Err(reject(
                    "result_unavailable",
                    "提出した数量根拠の版・範囲・契約が失効しています",
                ));
            }
            quantity
                .validate_available()
                .map_err(|e| reject("quantity_unavailable", e))?;
        }
        if let Some(index) = submission.run_index {
            let run = self.studies.items[s]
                .runs
                .get(index)
                .ok_or_else(|| reject("result_unavailable", "提出元の保持計算がありません"))?;
            if Self::ai_input_reference(&run.input) != *expected
                && Self::ai_input_reference(&run.prepared) != *expected
            {
                return Err(reject(
                    "result_scope_mismatch",
                    "提出元の保持計算と入力版が一致しません",
                ));
            }
            if run.ai_job != submission.job_id
                || run.calculation_state() != submission.calculation_state
                || submission
                    .references
                    .iter()
                    .any(|reference| run.referenced_result(reference).is_none())
            {
                return Err(reject(
                    "submission_run_mismatch",
                    "提出の保持計算・実行・結果参照が一致しません",
                ));
            }
            if let Some(bundle) = &run.results {
                let app =
                    comparison::projection::result_app(&run.prepared, bundle, run.last_static);
                for (case, _) in &bundle.statics {
                    app.require_result_input(ResultInputKey::Static(*case))
                        .map_err(|e| reject("result_unavailable", e))?;
                }
                for (name, _) in &bundle.combos {
                    app.require_result_input(ResultInputKey::Combo(name.clone()))
                        .map_err(|e| reject("result_unavailable", e))?;
                }
                if !bundle.member_checks.is_empty()
                    || !bundle.joint_checks.is_empty()
                    || !bundle.wall_checks.is_empty()
                {
                    app.require_design_input()
                        .map_err(|e| reject("result_unavailable", e))?;
                }
            } else if submission.calculation_state == "calculated" {
                return Err(reject("result_unavailable", "提出元の計算結果がありません"));
            }
        } else if let Some(job_id) = &submission.job_id {
            if !submission.references.is_empty()
                || !self.studies.items[s].ai.jobs.iter().any(|job| {
                    job.id == *job_id
                        && job.input == *expected
                        && receipt.content.get("request").and_then(Value::as_str)
                            == Some(job.request.as_str())
                })
            {
                return Err(reject(
                    "job_missing",
                    "提出時に観測した元ジョブがありません",
                ));
            }
        } else if submission.calculation_state != "not_calculated"
            || !submission.references.is_empty()
        {
            return Err(reject(
                "result_unavailable",
                "提出元の計算・ジョブがありません",
            ));
        }
        if self.ai_verification_at(s, submission.run_index) != submission.verification {
            return Err(reject(
                "result_unavailable",
                "提出時の検定根拠・評価範囲が失効・変化しています",
            ));
        }
        Ok(())
    }

    fn ai_result_view(&self, s: usize, reference: &ResultReference) -> Result<App, Rejection> {
        let run = self.studies.items[s]
            .runs
            .iter()
            .find(|r| r.referenced_result(reference).is_some())
            .ok_or_else(|| reject("result_missing", "この検討に保存された結果参照がありません"))?;
        if let Some(reason) = run.result_scope_error(reference) {
            return Err(reject("result_unavailable", reason));
        }
        let (input, bundle, last) = run.referenced_result(reference).unwrap();
        Ok(comparison::projection::result_app(input, bundle, last))
    }

    fn ai_validate_reference(
        &self,
        s: usize,
        input: &InputReference,
        reference: &ResultReference,
        history: bool,
    ) -> Result<(), Rejection> {
        let run = self.studies.items[s]
            .runs
            .iter()
            .find(|r| r.referenced_result(reference).is_some())
            .ok_or_else(|| reject("result_missing", "結果参照がこの検討にありません"))?;
        if Self::ai_input_reference(&run.input) != *input
            && Self::ai_input_reference(&run.prepared) != *input
        {
            return Err(reject(
                "result_scope_mismatch",
                "提出・閲覧対象の入力版と結果の計算時入力が一致しません",
            ));
        }
        let current = self.ai_input_is_current(s, input);
        if !current && !history {
            return Err(reject(
                "permission_denied",
                "保存旧版の閲覧が許可されていません",
            ));
        }
        let view = self.ai_result_view(s, reference)?;
        view.resolve_result_reference(reference)
            .map_err(|e| reject("result_unavailable", e))?;
        Ok(())
    }

    fn ai_results(
        &self,
        s: usize,
        _p: usize,
        input: &InputReference,
        request: &Request,
        _history: bool,
    ) -> Result<Value, Rejection> {
        let args: RunPage = args(request)?;
        let run = self.studies.items[s]
            .runs
            .get(args.run_index)
            .ok_or_else(|| reject("result_missing", "保持計算がありません"))?;
        if Self::ai_input_reference(&run.input) != *input
            && Self::ai_input_reference(&run.prepared) != *input
        {
            return Err(reject(
                "result_scope_mismatch",
                "明示した入力版と保持計算が一致しません",
            ));
        }
        let Some(bundle) = &run.results else {
            return Ok(
                json!({"state":"failed","error":run.error,"notice":run.notice,"page":page(Vec::new(),args.offset,args.limit)?}),
            );
        };
        let app = comparison::projection::result_app(&run.prepared, bundle, run.last_static);
        let fixed = comparison::projection::fixed_members(&self.studies.items[s]);
        let quantity = app.member_quantity_evidence_reference(fixed.clone());
        let steel = quantity.steel_takeoff();
        let whole_quantity_error = steel
            .as_ref()
            .ok()
            .and_then(|q| q.whole_quantity_error.clone());
        let volume = steel.map(|q| q.volume_m3());
        let mut rows = Vec::new();
        let (checks, unavailable) = ai_check_rows(run, &fixed);
        for check in &checks {
            rows.push(json!({"kind":"check","case":check.case,"target":check.target,"position":check.position,"ratio":check.ratio,"complete":check.complete,"reason":check.reason,"reference":check.evidence,"unit":"無次元","scope":"既存の全ケース・全組合せ主架構検定（非対象・未対応・未検定を含む）"}));
        }
        let keys: Vec<_> = bundle
            .statics
            .iter()
            .map(|(key, _)| StaticKey::Case(*key))
            .chain((0..bundle.combos.len()).map(StaticKey::Combo))
            .collect();
        for key in keys {
            for node in &app.core.model.nodes {
                for component in 0..3 {
                    match comparison::projection::displacement(&app,key,node.id,component) {
                        Ok((value,reference))=>rows.push(json!({"kind":"displacement","case":key,"node":node.id,"direction":(["X","Y","Z"][component]),"value":value,"unit":"mm","reference":reference})),
                        Err(reason)=>rows.push(json!({"kind":"displacement","case":key,"node":node.id,"direction":(["X","Y","Z"][component]),"value":null,"unit":"mm","reason":reason})),
                    }
                }
            }
        }
        let summary = comparison::projection::summarize(checks.iter());
        let fixed_summary = comparison::projection::summarize(comparison::projection::fixed_rows(
            checks.iter(),
            &fixed,
        ));
        let summary_json = |summary: comparison::projection::Summary| {
            let verification = verification_counts(
                &summary,
                unavailable.is_empty(),
                summary.checked + summary.skipped > 0,
            );
            json!({"checked":verification.checked,"unchecked":verification.unchecked,"confirmed_ng":verification.confirmed_ng,"observed_counts":verification.observed_counts,"count_unit":CHECK_COUNT_UNIT,"evaluated_maximum":summary.maximum.as_ref().and_then(|r|r.ratio),"maximum":if verification.scope_available && summary.skipped==0 {summary.maximum.and_then(|r|r.ratio)} else {None},"scope_complete":verification.scope_available,"unit":"無次元"})
        };
        Ok(
            json!({"state":run.calculation_state(),"input":input,"run_index":args.run_index,"conditions":run.input.settings,"error":run.error,"notice":run.notice,"unavailable":unavailable,"whole_checks":summary_json(summary),"fixed_checks":summary_json(fixed_summary),"fixed_steel_volume":{"value_m3":volume.as_ref().ok(),"error":volume.as_ref().err(),"members":fixed,"identity":quantity.identity,"whole_quantity_error":whole_quantity_error},"page":page(rows,args.offset,args.limit)?}),
        )
    }

    fn ai_run_metadata_row(index: usize, run: &StudyRun, requested: &InputReference) -> Value {
        let input = Self::ai_input_reference(&run.input);
        let prepared = Self::ai_input_reference(&run.prepared);
        json!({"run_index":index,"input":input,"prepared_input":prepared,"matches_requested_input":input==*requested || prepared==*requested,"created":run.created,"proposal_name":run.proposal_name,"recorded_calculation_state":run.calculation_state(),"error":run.error,"notice":run.notice,"conditions":run.input.settings,"app_version":run.app_version,"ai_job":run.ai_job})
    }

    fn ai_run_metadata(
        &self,
        s: usize,
        p: usize,
        requested: &InputReference,
        pagination: Page,
        history: bool,
    ) -> Result<Value, Rejection> {
        let current = self.ai_proposal_reference(s, p);
        let rows = self.studies.items[s]
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| {
                run.proposal == current.proposal
                    && (history
                        || Self::ai_input_reference(&run.input) == current
                        || Self::ai_input_reference(&run.prepared) == current)
            })
            .map(|(index, run)| Self::ai_run_metadata_row(index, run, requested))
            .collect();
        Ok(
            json!({"proposal":current.proposal,"requested_input":requested,"history_included":history,"state_meaning":"保持計算の実行時の状態です。現在の結果利用可否・検定合格・人の採用とは別です。数値と根拠はresults/evidenceで照合します","page":page(rows,pagination.offset,pagination.limit)?}),
        )
    }
}
