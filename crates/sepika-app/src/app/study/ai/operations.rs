use super::*;
use crate::app::{study::*, App};
use references::{InputReference, ReferenceTarget, ResultReference, TargetReference};
use sepika_ai_connection::protocol::{Rejection, Request, Response, VERSION};
use serde_json::json;

fn reject(code: &str, message: impl Into<String>) -> Rejection {
    Rejection { code: code.into(), message: message.into() }
}
fn args<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T, Rejection> {
    serde_json::from_value(request.arguments.clone()).map_err(|e|reject("invalid_arguments",e.to_string()))
}
fn page(rows: Vec<Value>, offset: usize, limit: usize) -> Result<Value, Rejection> {
    if limit == 0 || limit > 100 || offset > rows.len() {
        return Err(reject("invalid_page","ページは0以上のoffset、1〜100のlimitで指定してください"));
    }
    let total=rows.len();
    let items:Vec<_>=rows.into_iter().skip(offset).take(limit).collect();
    Ok(json!({"total":total,"offset":offset,"limit":limit,"omitted": offset>0 || offset+items.len()<total,"next_offset":(offset+items.len()<total).then_some(offset+items.len()),"items":items}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page { offset: usize, limit: usize }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name { name: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assign { targets: Vec<TargetReference>, section: SectionId }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id { id: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunPage { run_index: usize, offset: usize, limit: usize }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submit { explanation: String, unchecked: Vec<String>, references: Vec<ResultReference> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence { reference: ResultReference, table: String, offset: usize, limit: usize }

impl App {
    #[cfg(feature="gui")]
    pub(crate) fn ai_connection_panel(&mut self,ui:&mut egui::Ui) {
        ui.collapsing("外部AIへのローカル接続",|ui| {
            ui.label("外部クライアントがクラウドAIを利用すると、取得した情報が外部送信され得ます。クライアント側の保存・送信はSEPIKAの管理範囲外です。");
            ui.label("読取範囲: 許可した検討の基準・比較案の全体モデル、候補断面、条件、保持結果。固定対象以外の情報も含みます。");
            ui.label("編集範囲: 指定検討の比較案にある固定鉄骨大梁への既存有効断面の割当だけ。本案更新・採用は人が行います。");
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
                ui.label(format!("許可中の検討: {purpose} / クライアント: {} / {:?} / 保存旧版閲覧: {}",permission.connection,permission.mode,permission.history));
            }
        });
    }
    /// 検討に保存された依頼・提出・受理台帳を返します。接続許可は含みません。
    pub fn ai_observations(&self, study: usize) -> Option<&StudyAi> {
        self.studies.items.get(study).map(|s|&s.ai)
    }

    #[cfg(windows)]
    pub fn enable_ai_connection(&mut self) -> Result<String,String> {
        self.disconnect_ai_connection();
        let listener=sepika_ai_connection::listener::Listener::start().map_err(|e|e.to_string())?;
        let name=listener.name.clone();
        self.ai_connection.listener=Some(listener);
        Ok(name)
    }

    pub fn disconnect_ai_connection(&mut self) {
        self.ai_connection=AiConnection::default();
    }

    /// GUIが受信済みクライアントと対象検討を確認した後にだけ呼び出します。
    pub fn approve_ai_connection(&mut self, study: usize, mode: Mode, history: bool) -> Result<(),String> {
        let connection=self.ai_connection.pending.take().ok_or("接続確認を待つクライアントがありません")?;
        let item=self.studies.items.get(study).ok_or("検討がありません")?;
        self.ai_connection.permission=Some(Permission {
            connection,session:uuid::Uuid::new_v4().to_string(),project:item.baseline.references.project.clone(),study:item.ai.id.clone(),mode,history,
        });
        Ok(())
    }

    #[cfg(windows)]
    pub fn poll_ai_connection(&mut self) {
        self.sync_ai_jobs();
        loop {
            let event=self.ai_connection.listener.as_ref().and_then(|l|l.events.try_recv().ok());
            let Some(event)=event else {break;};
            match event {
                sepika_ai_connection::listener::Event::Request(incoming)=> {
                    let response=self.handle_ai_request(&incoming.connection,incoming.request);
                    let _=incoming.response.send(response);
                }
                sepika_ai_connection::listener::Event::Disconnected(connection)=> {
                    if self.ai_connection.permission.as_ref().is_some_and(|p|p.connection==connection) {self.ai_connection.permission=None;}
                    if self.ai_connection.pending.as_ref()==Some(&connection) {self.ai_connection.pending=None;}
                }
            }
        }
    }

    pub fn handle_ai_request(&mut self, connection: &str, request: Request) -> Response {
        let id=request.id.clone();
        match self.process_ai_request(connection,request) {
            Ok(value)=>Response::accepted(id,value),
            Err(error)=>Response {id,result:None,error:Some(error)},
        }
    }

    fn process_ai_request(&mut self, connection: &str, request: Request) -> Result<Value,Rejection> {
        self.sync_ai_jobs();
        if request.version!=VERSION {return Err(reject("unsupported_version","プロトコル版が一致しません"));}
        if request.id.is_empty() || request.id.len()>128 {return Err(reject("invalid_id","要求IDは1〜128バイトで指定してください"));}
        if request.operation=="connect" {
            if let Some(permission)=self.ai_connection.permission.as_ref().filter(|p|p.connection==connection) {
                let study=self.studies.items.iter().position(|s|s.ai.id==permission.study).ok_or_else(||reject("study_missing","許可した検討がありません"))?;
                return Ok(json!({"version":VERSION,"status":"approved","session":permission.session,"project":permission.project,"study":permission.study,"mode":permission.mode,"history":permission.history,"read_scope":"許可した検討の基準・比較案の全体モデル、条件、候補断面、保持結果","edit_scope":"比較案の固定鉄骨大梁の既存有効断面割当のみ","proposals":self.ai_proposals(study)}));
            }
            self.ai_connection.pending=Some(connection.into());
            return Ok(json!({"version":VERSION,"status":"awaiting_gui_approval"}));
        }
        let permission=self.ai_connection.permission.as_ref().ok_or_else(||reject("permission_denied","GUIの接続許可がありません"))?;
        if permission.connection!=connection || request.session.as_ref()!=Some(&permission.session) {
            return Err(reject("disconnected","接続セッションが失効しています"));
        }
        if request.project.as_ref()!=Some(&permission.project) || request.study.as_ref()!=Some(&permission.study) || self.core.scoped.references.project!=permission.project {
            return Err(reject("scope_mismatch","許可したプロジェクト・検討と一致しません"));
        }
        let s=self.studies.items.iter().position(|s|s.ai.id==permission.study).ok_or_else(||reject("study_missing","検討がありません"))?;
        let mode=permission.mode;
        let history=permission.history;
        let mut content=serde_json::to_value(&request).unwrap();
        content.as_object_mut().unwrap().remove("session");
        if let Some(receipt)=self.studies.items[s].ai.receipts.iter().find(|r|r.id==request.id) {
            return if receipt.content==content {Ok(receipt.result.clone())} else {Err(reject("request_id_conflict","同じ要求IDを異なる内容に再利用できません"))};
        }
        let p=self.ai_proposal_index(s,request.proposal.as_deref())?;
        let current=self.ai_proposal_reference(s,p);
        let expected:InputReference=serde_json::from_value(request.input.clone().ok_or_else(||reject("input_required","対象の入力版を明示してください"))?).map_err(|e|reject("invalid_input",e.to_string()))?;
        if expected.project!=current.project || expected.proposal!=current.proposal {return Err(reject("scope_mismatch","対象の案・プロジェクトが一致しません"));}
        let mutates=matches!(request.operation.as_str(),"create_comparison"|"assign_section"|"calculate");
        if mutates && mode!=Mode::Comparison {return Err(reject("permission_denied","確認のみの依頼では作成・編集・計算を実行できません"));}
        let old=expected!=current;
        if old && (mutates || !history) {return Err(reject("input_conflict","対象の入力版が現在版と一致しません。許可した保存旧版の明示閲覧だけが可能です"));}
        if old && !self.studies.items[s].runs.iter().any(|r|Self::ai_input_reference(&r.input)==expected || Self::ai_input_reference(&r.prepared)==expected) {return Err(reject("input_missing","明示した保存入力版がありません"));}
        let task_id=request.request.as_deref().filter(|id|!id.is_empty() && id.len()<=128).ok_or_else(||reject("request_required","依頼IDを指定してください"))?;
        if request.operation!="begin_request" && request.operation!="receipt" && !self.studies.items[s].ai.tasks.iter().any(|t|t.id==task_id) {return Err(reject("request_not_started","先に依頼開始を明示してください"));}
        if mutates {
            let task=self.studies.items[s].ai.tasks.iter().find(|t|t.id==task_id).ok_or_else(||reject("request_not_started","先に依頼開始を明示してください"))?;
            if task.mode!=Mode::Comparison || task.fixed!=self.studies.items[s].fixed || task.baseline!=Self::ai_input_reference(&self.studies.items[s].baseline) {
                return Err(reject("permission_denied","この依頼は比較編集を許可されていないか、開始時の固定対象・条件が変わっています"));
            }
        }
        let output=match request.operation.as_str() {
            "begin_request"=> {
                if old {return Err(reject("input_conflict","依頼開始は現在入力版を指定してください"));}
                if self.studies.items[s].ai.tasks.iter().any(|t|t.id==task_id) {return Err(reject("request_exists","依頼IDは既に開始済みです"));}
                let baseline=Self::ai_input_reference(&self.studies.items[s].baseline);
                let fixed=self.studies.items[s].fixed.clone();
                self.studies.items[s].ai.tasks.push(Task{id:task_id.into(),mode,baseline,fixed,started:SystemTime::now(),last_operation:SystemTime::now(),submissions:Vec::new()});
                json!({"status":"started","request":task_id,"mode":mode,"input":current})
            }
            "study"=>json!({"purpose":self.studies.items[s].purpose,"fixed":self.studies.items[s].fixed,"proposals":self.ai_proposals(s),"input":expected,"read_scope":"当該検討の全体モデル・条件・結果","edit_scope":"比較案の固定鉄骨大梁の既存有効断面割当のみ"}),
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
                let job=AiJob {id:request.id.clone(),request:task_id.into(),input:current.clone(),state:"running".into(),run_index:None,previous_runs:self.studies.items[s].runs.len()};
                self.studies.items[s].ai.jobs.push(job.clone());
                json!(job)
            }
            "job"=> {
                let id:Id=args(&request)?;
                let job=self.studies.items[s].ai.jobs.iter().find(|j|j.id==id.id && j.request==task_id && j.input.proposal==expected.proposal).ok_or_else(||reject("job_missing","この依頼・案に属するジョブがありません"))?;
                json!(job)
            }
            "receipt"=> {
                let id:Id=args(&request)?;
                let receipt=self.studies.items[s].ai.receipts.iter().find(|r|r.id==id.id && r.content.get("request").and_then(Value::as_str)==Some(task_id)).ok_or_else(||reject("receipt_missing","受理済み要求がありません。未受理か応答不明であり変更の成否を推定しないでください"))?;
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
            "evidence"=> {
                let evidence:Evidence=args(&request)?;
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
                    "design_details"=>view.core.scoped.results.as_ref().map(|b|b.member_checks.iter().map(|v|json!(v)).collect()).unwrap_or_default(),
                    _=>return Err(reject("unsupported_table","根拠表の種類が未対応です")),
                };
                json!({"reference":evidence.reference,"scope":"計算時の全体モデル","units":{"length":"mm","force":"N","stress":"N/mm²"},"page":page(rows,evidence.offset,evidence.limit)?})
            }
            "submit"=> {
                let submission:Submit=args(&request)?;
                if submission.explanation.trim().is_empty() {return Err(reject("invalid_arguments","説明を指定してください"));}
                for reference in &submission.references {self.ai_validate_reference(s,&expected,reference,history)?;}
                let state=self.ai_calculation_state(s,&expected);
                let submitted=Submission{input:expected.clone(),explanation:submission.explanation,unchecked:submission.unchecked,references:submission.references,calculation_state:state,submitted:SystemTime::now()};
                self.studies.items[s].ai.tasks.iter_mut().find(|t|t.id==task_id).unwrap().submissions.push(submitted.clone());
                json!({"status":"submitted","submission":submitted,"notice":"成果提出は計算成功・検定合格・人による採用とは別です"})
            }
            _=>return Err(reject("unsupported_operation","この操作は公開していません。本案更新・採用・警告承諾・取消・任意ファイル操作は実行できません")),
        };
        if let Some(task)=self.studies.items[s].ai.tasks.iter_mut().find(|t|t.id==task_id) {task.last_operation=SystemTime::now();}
        if matches!(request.operation.as_str(),"begin_request"|"create_comparison"|"assign_section"|"calculate"|"submit") {
            self.studies.items[s].ai.receipts.push(Receipt{id:request.id,content,result:output.clone()});
        }
        self.mark_study_dirty();
        Ok(output)
    }

    fn ai_proposal_index(&self,s:usize,proposal:Option<&str>)->Result<usize,Rejection> {
        self.studies.items[s].proposals.iter().enumerate().find_map(|(p,_)|(Some(self.ai_proposal_reference(s,p).proposal.as_str())==proposal).then_some(p)).ok_or_else(||reject("proposal_missing","この検討に属する対象案がありません。本案は操作できません"))
    }
    fn ai_proposal_reference(&self,s:usize,p:usize)->InputReference {
        if self.studies.active==Location::Proposal(s,p) {self.input_reference()} else {
            let app=App{core:self.proposal_input(s,p).core(),..Default::default()};
            app.input_reference()
        }
    }
    fn ai_input_reference(input:&StudyInput)->InputReference {
        App{core:input.core(),..Default::default()}.input_reference()
    }
    fn ai_proposals(&self,s:usize)->Value {
        Value::Array(self.studies.items[s].proposals.iter().enumerate().map(|(p,proposal)|json!({"name":proposal.name,"baseline":p==0,"input":self.ai_proposal_reference(s,p)})).collect())
    }
    fn ai_read_input(&self,s:usize,p:usize,expected:&InputReference)->Result<StudyInput,Rejection> {
        if self.ai_proposal_reference(s,p)==*expected {return Ok(self.proposal_input(s,p));}
        self.studies.items[s].runs.iter().flat_map(|r|[&r.input,&r.prepared]).find(|input|Self::ai_input_reference(input)==*expected).cloned().ok_or_else(||reject("input_missing","保存入力版がありません"))
    }

    fn ai_assign(&mut self,s:usize,p:usize,request:&Request)->Result<Value,Rejection> {
        if p==0 {return Err(reject("permission_denied","基準案の編集は許可されていません"));}
        if self.studies.running.as_ref().is_some_and(|r|r.study==s) {
            return Err(reject("calculation_busy","この検討は計算中です"));
        }
        let assign:Assign=args(request)?;
        if assign.targets.is_empty() {return Err(reject("target_invalid","固定対象を明示してください"));}
        let input=self.proposal_input(s,p);
        if !comparison::projection::same_conditions(&self.studies.items[s].baseline,&input,&comparison::projection::fixed_members(&self.studies.items[s])) {
            return Err(reject("conditions_mismatch","固定対象の断面以外に基準条件との差があります"));
        }
        if !input.model.section(assign.section).is_some_and(|section|sepika_edit::valid_study_section(&input.model,section)) {
            return Err(reject("section_invalid","既存の有効な鉄骨大梁断面ではありません"));
        }
        let mut ids=Vec::new();
        for target in &assign.targets {
            let ReferenceTarget::Member(id)=self.resolve_study_fixed_target(s,target).map_err(|e|reject("target_invalid",e))? else {
                return Err(reject("target_invalid","固定鉄骨大梁以外は編集できません"));
            };
            if ids.contains(&id) {return Err(reject("target_invalid","固定対象が重複しています"));}
            ids.push(id);
        }
        let command=Box::new(sepika_edit::CompositeCommand{label:"AIによる比較案の断面割当".into(),children:ids.iter().map(|id|Box::new(sepika_edit::SetElementSection{elem:*id,section:Some(assign.section)}) as Box<dyn sepika_edit::EditCommand>).collect()});
        let core=if self.studies.active==Location::Proposal(s,p) {&mut self.core} else {&mut self.studies.items[s].proposals[p].state.as_mut().unwrap().core};
        let applied=core.scoped.undo.run(&mut core.model,command);
        if let Some(reason)=core.scoped.undo.last_error() {return Err(reject("edit_rejected",reason));}
        if applied {core.scoped.staleness.mark_edited();}
        Ok(json!({"applied":applied,"members":ids,"section":assign.section,"input":self.ai_proposal_reference(s,p)}))
    }

    pub(super) fn sync_ai_jobs(&mut self) {
        for (s,study) in self.studies.items.iter_mut().enumerate() {
            for job in &mut study.ai.jobs {
                if !matches!(job.state.as_str(),"running"|"awaiting_gui_confirmation") {continue;}
                if let Some(run_index)=(job.previous_runs..study.runs.len()).find(|r|Self::ai_input_reference(&study.runs[*r].input)==job.input) {
                    let run=&study.runs[run_index];
                    job.run_index=Some(run_index);
                    job.state=if run.error.is_some() || run.results.is_none() {"failed"} else {"calculated"}.into();
                } else if let Some(running)=self.studies.running.as_ref().filter(|r|r.study==s) {
                    job.state=if running.worker.core.scoped.pending_unset_analysis.is_some() {"awaiting_gui_confirmation"} else {"running"}.into();
                } else {job.state="interrupted_no_result".into();}
            }
        }
    }

    fn ai_calculation_state(&self,s:usize,input:&InputReference)->String {
        self.studies.items[s].ai.jobs.iter().rev().find(|j|j.input==*input).map(|j|j.state.clone()).unwrap_or_else(|| {
            self.studies.items[s].runs.iter().rev().find(|run|Self::ai_input_reference(&run.input)==*input || Self::ai_input_reference(&run.prepared)==*input).map(|r|if r.error.is_some() || r.results.is_none() {"failed".into()} else {"calculated".into()}).unwrap_or_else(||"not_calculated".into())
        })
    }

    fn ai_result_view(&self,s:usize,reference:&ResultReference)->Result<App,Rejection> {
        let run=self.studies.items[s].runs.iter().find(|r|r.referenced_result(reference).is_some()).ok_or_else(||reject("result_missing","この検討に保存された結果参照がありません"))?;
        if let Some(reason)=run.result_scope_error(reference) {return Err(reject("result_unavailable",reason));}
        let (input,bundle,last)=run.referenced_result(reference).unwrap();
        Ok(comparison::projection::result_app(input,bundle,last))
    }

    fn ai_validate_reference(&self,s:usize,input:&InputReference,reference:&ResultReference,history:bool)->Result<(),Rejection> {
        let run=self.studies.items[s].runs.iter().find(|r|r.referenced_result(reference).is_some()).ok_or_else(||reject("result_missing","結果参照がこの検討にありません"))?;
        if Self::ai_input_reference(&run.input)!=*input && Self::ai_input_reference(&run.prepared)!=*input {
            return Err(reject("result_scope_mismatch","提出・閲覧対象の入力版と結果の計算時入力が一致しません"));
        }
        let current=self.studies.items[s].proposals.iter().enumerate().any(|(p,_)|self.ai_proposal_reference(s,p)==*input);
        if !current && !history {return Err(reject("permission_denied","保存旧版の閲覧が許可されていません"));}
        let view=self.ai_result_view(s,reference)?;
        view.resolve_result_reference(reference).map_err(|e|reject("result_unavailable",e))?;
        Ok(())
    }

    fn ai_results(&self,s:usize,_p:usize,input:&InputReference,request:&Request,_history:bool)->Result<Value,Rejection> {
        let args:RunPage=args(request)?;
        let run=self.studies.items[s].runs.get(args.run_index).ok_or_else(||reject("result_missing","保持計算がありません"))?;
        if Self::ai_input_reference(&run.input)!=*input && Self::ai_input_reference(&run.prepared)!=*input {
            return Err(reject("result_scope_mismatch","明示した入力版と保持計算が一致しません"));
        }
        let Some(bundle)=&run.results else {
            return Ok(json!({"state":"failed","error":run.error,"notice":run.notice,"page":page(Vec::new(),args.offset,args.limit)?}));
        };
        let app=comparison::projection::result_app(&run.prepared,bundle,run.last_static);
        let fixed=comparison::projection::fixed_members(&self.studies.items[s]);
        let quantity=app.member_quantity_evidence_reference(fixed.clone());
        let volume=quantity.takeoff().map(|q|q.totals().steel_t/sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3);
        let mut rows=Vec::new();
        let mut checks=Vec::new();
        let mut unavailable=Vec::new();
        if let Some(reason)=run.original_scope_error() {unavailable.push(reason);}
        if run.comparison_checks.is_empty() {unavailable.push("ケース別検定根拠がありません。未検定を0や合格へ置き換えません".into());}
        for evidence in &run.comparison_checks {
            if let Err(reason)=comparison::projection::fixed_coverage(evidence,&fixed) {unavailable.push(reason);}
            match comparison::projection::check_rows(evidence) {Ok(mut r)=>checks.append(&mut r),Err(reason)=>unavailable.push(reason)}
        }
        for check in &checks {
            rows.push(json!({"kind":"check","case":check.case,"target":check.target,"position":check.position,"ratio":check.ratio,"complete":check.complete,"reason":check.reason,"reference":check.evidence,"unit":"無次元","scope":"既存の全ケース・全組合せ主架構検定（非対象・未対応・未検定を含む）"}));
        }
        let keys:Vec<_>=bundle.statics.iter().map(|(key,_)|StaticKey::Case(*key)).chain((0..bundle.combos.len()).map(StaticKey::Combo)).collect();
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
        let summary=comparison::projection::summarize(checks.iter());
        let fixed_summary=comparison::projection::summarize(comparison::projection::fixed_rows(checks.iter(),&fixed));
        let summary_json=|summary:comparison::projection::Summary|json!({"checked":summary.checked,"unchecked":summary.skipped,"confirmed_ng":summary.ng,"maximum":if unavailable.is_empty() && summary.skipped==0 {summary.maximum.and_then(|r|r.ratio)} else {None},"scope_complete":unavailable.is_empty(),"unit":"無次元"});
        Ok(json!({"state":self.ai_calculation_state(s,input),"input":input,"run_index":args.run_index,"conditions":run.input.settings,"error":run.error,"notice":run.notice,"unavailable":unavailable,"whole_checks":summary_json(summary),"fixed_checks":summary_json(fixed_summary),"fixed_steel_volume":{"value_m3":volume.as_ref().ok(),"error":volume.as_ref().err(),"members":fixed,"identity":quantity.identity},"page":page(rows,args.offset,args.limit)?}))
    }
}
