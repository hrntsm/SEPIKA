use super::*;

fn request_scope(
    fixed_count: usize,
    quantity: Option<&references::QuantityEvidenceReference>,
) -> String {
    quantity.map_or_else(
        || format!("固定対象 {fixed_count} 件"),
        |quantity| {
            format!(
                "数量の固定範囲: {}（画面の選択とは独立）",
                quantity.scope.label()
            )
        },
    )
}

#[cfg(feature = "gui")]
fn observed_time(time: SystemTime) -> String {
    time::OffsetDateTime::from(time)
        .replace_nanosecond(0)
        .ok()
        .and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| "時刻を取得不能".into())
}

#[cfg(feature = "gui")]
fn operation_label(operation: &str) -> &str {
    match operation {
        "begin_request" => "依頼開始を受理",
        "submit" => "成果提出を受理",
        "calculate" => "計算開始を受理",
        "create_comparison" => "比較案を作成",
        "assign_section" => "断面割当を適用",
        "job" => "計算ジョブを照会",
        "receipt" => "受理記録を照会",
        "results" | "evidence" | "runs" => "保持結果・根拠を参照",
        "study" | "proposals" | "fixed_targets" | "conditions" | "members" | "nodes"
        | "materials" | "load_cases" | "combinations" | "sections" | "changes" | "quantity"
        | "decisions" => "検討の入力・条件を参照",
        _ => operation,
    }
}

#[cfg(feature = "gui")]
fn calculation_label(state: &str) -> &str {
    match state {
        "running" => "計算中",
        "awaiting_gui_confirmation" => "人の確認待ち",
        "calculated" => "計算済み（検定合格・採用とは別）",
        "failed" => "計算失敗",
        "interrupted_no_result" => "中断・結果なし",
        "not_calculated" => "未計算",
        _ => "状態を取得不能",
    }
}

#[cfg(feature = "gui")]
fn target_label(target: ReferenceTarget) -> String {
    match target {
        ReferenceTarget::Node(id) => format!("節点 #{}", id.0),
        ReferenceTarget::Member(id) => format!("部材 #{}", id.0),
        ReferenceTarget::Section(id) => format!("断面 #{}", id.0),
        ReferenceTarget::Material(id) => format!("材料 #{}", id.0),
        ReferenceTarget::Story(id) => format!("階 #{}", id.0),
        ReferenceTarget::Axis { group, axis } => {
            format!("通り（群{}・通り{}）", group + 1, axis + 1)
        }
        ReferenceTarget::LoadCase(id) => format!("荷重ケース #{}", id.0),
        ReferenceTarget::Combination(index) => format!("荷重組合せ #{}", index),
        ReferenceTarget::Vibration(id) => format!("立体振動ケース #{}", id.0),
        ReferenceTarget::LumpedVibration(id) => format!("質点系振動ケース #{}", id.0),
    }
}

#[cfg(feature = "gui")]
fn result_label(reference: &ResultReference, model: &sepika_core::model::Model) -> String {
    match &reference.kind {
        ResultInputKey::Static(_) | ResultInputKey::Combo(_) => {
            comparison::projection::case_label(&reference.kind, model)
        }
        ResultInputKey::Design => "主架構検定".into(),
        ResultInputKey::Modal => "固有値解析".into(),
        ResultInputKey::Pushover(_) => "増分解析".into(),
        ResultInputKey::TimeHistory(_) => "立体時刻歴".into(),
        ResultInputKey::LumpedTimeHistory(_) => "質点系時刻歴".into(),
    }
}

impl App {
    /// 表示中の確定入力と共通参照を依頼準備へ固定する。
    #[cfg(feature = "gui")]
    pub fn prepare_ai_request(&mut self, purpose: String, mode: Mode) -> Result<(), String> {
        if mode == Mode::Comparison
            && matches!(
                self.studies.active,
                Location::History(..) | Location::StartedInput(..) | Location::RequestInput(..)
            )
        {
            return Err("比較作成は本案または比較案の現在入力から依頼してください".into());
        }
        if purpose.trim().is_empty() || purpose.len() > 1024 {
            return Err("目的を1〜1024バイトで入力してください".into());
        }
        if self.core.scoped.job.is_some() || self.core.scoped.pending_plate_loss_edit.is_some() {
            return Err("実行中の計算・保留中の入力を解決してください".into());
        }
        if !self.ui.scoped.pending_story_cmds.is_empty() {
            return Err("階の確定済み入力の反映が終わるまで待ってください".into());
        }
        let evidence = self.ai_connection.request_evidence.clone();
        let quantity = match &evidence {
            Some(references::EvidenceReference::Quantity(quantity)) => {
                if mode != Mode::Review {
                    return Err("数量根拠は「根拠の確認のみ」で依頼してください。比較編集の対象にはできません".into());
                }
                if quantity.source != self.input_reference() {
                    return Err("依頼元の数量根拠と表示中の案・入力版が異なります。元の保存入力を明示表示してから対象を再確認してください".into());
                }
                if quantity.contract != references::QUANTITY_EVIDENCE_CONTRACT_VERSION
                    || quantity.identity() != quantity.identity
                {
                    return Err("依頼元の数量根拠の範囲・契約が失効しています".into());
                }
                quantity.validate_available()?;
                Some(quantity.as_ref().clone())
            }
            _ => None,
        };
        let mut fixed = match &evidence {
            Some(references::EvidenceReference::Quantity(_)) => Vec::new(),
            Some(references::EvidenceReference::Calculation(reference)) => {
                vec![reference.target.clone()]
            }
            None => {
                self.freeze_selected_references()?;
                self.core.scoped.references.fixed.clone()
            }
        };
        if fixed.is_empty() && quantity.is_none() {
            return Err("図・表・結果で対象を選択してください".into());
        }
        let source_targets = fixed.clone();
        if mode == Mode::Comparison {
            for target in &fixed {
                let ReferenceTarget::Member(id) = target.target else {
                    return Err("比較作成の対象は既存の鉄骨大梁だけです".into());
                };
                let e = self.core.model.element(id).ok_or("対象がありません")?;
                if e.kind != sepika_core::model::ElementKind::Beam
                    || !e
                        .section
                        .and_then(|id| self.core.model.section(id))
                        .is_some_and(|s| sepika_edit::valid_study_section(&self.core.model, s))
                {
                    return Err("比較作成の対象は既存の鉄骨大梁だけです".into());
                }
            }
            if let Location::Proposal(s, p) = self.studies.active {
                let study = &self.studies.items[s];
                let selected: Vec<_> = fixed.iter().map(|target| target.target).collect();
                if selected.len() != study.fixed.len()
                    || !study
                        .fixed
                        .iter()
                        .all(|target| selected.contains(&target.target))
                    || !comparison::projection::same_conditions(
                        &study.baseline,
                        &self.proposal_input(s, p),
                        &comparison::projection::fixed_members(study),
                    )
                {
                    return Err("検討の固定鉄骨大梁すべてを選択し、基準条件を確認してください。別対象・別条件は本案から新しい検討を作成します".into());
                }
                fixed = study.fixed.clone();
            }
        }
        let mut display = if quantity.is_some() {
            Default::default()
        } else {
            self.current_display_conditions().unwrap_or_default()
        };
        if quantity.is_none() && display.frame.is_none() {
            display.frame = self
                .active_viewport()
                .frame_target
                .map(|frame| match frame {
                    sepika_core::frame::FrameTarget::Story(id) => ReferenceTarget::Story(id),
                    sepika_core::frame::FrameTarget::Axis { group, axis } => {
                        ReferenceTarget::Axis { group, axis }
                    }
                });
        }
        let mut results: Vec<_> = self
            .core
            .scoped
            .references
            .fixed_results
            .iter()
            .filter(|reference| fixed.contains(&reference.target))
            .cloned()
            .collect();
        if let Some(references::EvidenceReference::Calculation(reference)) =
            &self.ai_connection.request_evidence
        {
            if reference.target.source != self.input_reference() {
                return Err("依頼元の根拠と表示中の案・入力版が異なります。元の保持結果・図表を明示表示してから対象を再確認してください".into());
            }
            self.resolve_result_reference(reference)?;
            results = vec![reference.as_ref().clone()];
            display = reference.display.clone();
        }
        let cfg = &self.core.analysis_cfg;
        let calculation = if mode == Mode::Review {
            "確認のみ（新しい計算は許可しません。元結果の実行条件は根拠で確認）"
        } else {
            "全体の準備・線形静的解析・対応検定・数量"
        };
        let conditions=format!("全{}荷重ケース・全{}組合せ / {calculation} / 地域係数 {}・標準せん断力係数 {} / RC損傷制御 {} / 長期・短期は各ケースの定義に従う。解析・検定設定はこの入力版に固定",self.core.model.load_cases.len(),self.core.model.combinations.len(),cfg.z,cfg.c0,if cfg.rc_damage_control {"あり"}else{"なし"});
        let frame = display
            .frame
            .map(|target| self.reference_target_description(target))
            .unwrap_or_else(|| "全体表示（階・通りの指定なし）".into());
        self.ai_connection.draft = Some(RequestDraft {
            input: self.input_reference(),
            fixed,
            source_targets,
            from_main: self.study_is_main(),
            results,
            display,
            quantity,
            mode,
            purpose,
            proposal_name: self.study_display_name(),
            conditions,
            frame,
        });
        Ok(())
    }

    /// 準備した元入力版が変わった場合は再確認を要求する。依頼は未開始で保存する。
    pub fn confirm_ai_request(&mut self) -> Result<(usize, String), String> {
        let draft = self
            .ai_connection
            .draft
            .as_ref()
            .ok_or("依頼の対象を確認してください")?;
        if self.input_reference() != draft.input {
            return Err("対象の入力版・条件が変わりました。対象を再確認してください".into());
        }
        let purpose = draft.purpose.clone();
        let mode = draft.mode;
        let fixed = draft.fixed.clone();
        let source = draft.input.clone();
        let origin = StudyInput::capture(&self.core);
        let historical = matches!(
            self.studies.active,
            Location::History(..) | Location::StartedInput(..) | Location::RequestInput(..)
        ) || match self.studies.active {
            Location::Proposal(s, _) => !self.ai_input_is_current(s, &source),
            _ => false,
        };
        let existing = match self.studies.active {
            Location::Main => None,
            Location::Proposal(s, _)
            | Location::History(s, _)
            | Location::StartedInput(s, _)
            | Location::RequestInput(s, _) => Some(s),
        };
        let new_review = mode == Mode::Review
            && !existing.is_some_and(|s| {
                self.studies.items[s]
                    .proposals
                    .iter()
                    .enumerate()
                    .any(|(p, _)| self.ai_proposal_reference(s, p).proposal == source.proposal)
            });
        let s = if new_review {
            let baseline = origin.clone();
            self.insert_study(purpose.clone(), Vec::new(), baseline.clone(), baseline)
        } else if let Some(s) = existing {
            s
        } else {
            let ids = fixed
                .iter()
                .filter_map(|target| match target.target {
                    ReferenceTarget::Member(id) => Some(id),
                    _ => None,
                })
                .collect();
            self.create_study(purpose.clone(), ids)?
        };
        if new_review {
            if let Some(bundle) = &self.core.scoped.results {
                let baseline = origin.clone();
                let run = StudyRun {
                    ai_job: None,
                    proposal: baseline.references.proposal.clone(),
                    proposal_name: self
                        .ai_connection
                        .draft
                        .as_ref()
                        .unwrap()
                        .proposal_name
                        .clone(),
                    input: baseline.clone(),
                    prepared: baseline,
                    expected_result_checks: Some(comparison::ExpectedChecks::capture(bundle)),
                    results: Some(bundle.clone()),
                    comparison_checks: Vec::new(),
                    input_view_references: ViewReferences::default(),
                    result_view_references: ViewReferences {
                        fixed: fixed.clone(),
                        results: self.ai_connection.draft.as_ref().unwrap().results.clone(),
                        evidence: Vec::new(),
                    },
                    preparation: self.core.scoped.preparation.clone(),
                    last_static: self.core.scoped.last_static,
                    app_version: env!("CARGO_PKG_VERSION").into(),
                    created: SystemTime::now(),
                    notice: self.core.scoped.last_notice.clone(),
                    error: self.core.scoped.last_error.clone(),
                    quantity_csv: crate::summary::build_quantity_csv(&self.core.model),
                    log: self.core.log.entries.clone(),
                    diagnostics: self.core.scoped.diagnostics.clone(),
                };
                self.studies.items[s].runs.push(run);
            }
        }
        let p = match self.studies.active {
            Location::Main => 0,
            _ => self.studies.items[s]
                .proposals
                .iter()
                .enumerate()
                .find(|(p, _)| self.ai_proposal_reference(s, *p).proposal == source.proposal)
                .map(|(p, _)| p)
                .ok_or("元の案が取得できません")?,
        };
        if mode == Mode::Comparison && fixed != self.studies.items[s].fixed {
            return Err(
                "選択と検討の編集対象が異なります。本案で新しい検討を作成してください".into(),
            );
        }
        let draft = self.ai_connection.draft.take().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let task = Task {
            id: id.clone(),
            mode,
            baseline: Self::ai_input_reference(&self.studies.items[s].baseline),
            started_input: if mode == Mode::Review {
                draft.input
            } else {
                self.ai_proposal_reference(s, p)
            },
            origin,
            from_main: draft.from_main,
            source_targets: draft.source_targets,
            historical,
            fixed,
            purpose,
            note: String::new(),
            proposal_name: draft.proposal_name,
            conditions: draft.conditions,
            frame: draft.frame,
            results: draft.results,
            display: draft.display,
            quantity: draft.quantity,
            prepared: SystemTime::now(),
            started: None,
            interrupted: None,
            last_operation: SystemTime::now(),
            last_operation_name: "依頼準備".into(),
            submissions: Vec::new(),
        };
        self.studies.items[s].ai.tasks.push(task);
        self.ai_connection.selected_study = s;
        self.ai_connection.comparison = mode == Mode::Comparison;
        self.ai_connection.request_evidence = None;
        self.mark_study_dirty();
        Ok((s, id))
    }

    /// 依頼文・検討と依頼の識別・固定対象だけを返す。全体モデルは含めない。
    pub fn ai_request_text(&self, s: usize, id: &str) -> Result<String, String> {
        let study = self.studies.items.get(s).ok_or("検討がありません")?;
        let task = study
            .ai
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or("依頼がありません")?;
        let quantity=task.quantity.as_ref().map(|quantity|json!({"source":quantity.source,"scope":quantity.scope,"identity":quantity.identity,"contract":quantity.contract}));
        Ok(format!("SEPIKAへの依頼: {}\n種別: {}\n検討ID: {}\n依頼ID: {}\n操作対象案・入力版: {}\n依頼元入力: {}\n基準: {}\n固定範囲: {}\n依頼元対象参照: {}\n比較編集の固定対象参照: {}\n対象結果参照: {}\n数量根拠: {}\n案名: {}\n階・通り: {}\n計算条件: {}\n接続承認後にこの依頼IDでbegin_requestを明示開始してください。確認のみの場合は比較案作成・入力変更・計算を要求しません。成果はsubmitで説明・未確認事項・元の根拠参照を提出してください。採用は人が行います。",
            task.purpose, if task.mode == Mode::Review { "根拠の確認のみ（編集・計算不可）" } else { "断面の比較案を作成" }, study.ai.id, task.id,
            serde_json::to_string(&task.started_input).unwrap(),serde_json::to_string(&task.origin.input_reference()).unwrap(), serde_json::to_string(&task.baseline).unwrap(),request_scope(task.source_targets.len(),task.quantity.as_ref()),serde_json::to_string(&task.source_targets).unwrap(), serde_json::to_string(&task.fixed).unwrap(), serde_json::to_string(&task.results).unwrap(),serde_json::to_string(&quantity).unwrap(),task.proposal_name,task.frame,task.conditions))
    }

    /// 人による補足・判断理由を保存し、本文だけの変更でも未保存にする。
    pub fn set_ai_request_note(&mut self, s: usize, id: &str, note: String) -> Result<(), String> {
        let task = self
            .studies
            .items
            .get_mut(s)
            .and_then(|s| s.ai.tasks.iter_mut().find(|t| t.id == id))
            .ok_or("依頼がありません")?;
        if task.note != note {
            task.note = note;
            self.mark_study_dirty();
        }
        Ok(())
    }

    pub(super) fn ai_task_source_current(&self, task: &Task) -> bool {
        !task.from_main
            || self.main_study_input().input_reference() == task.origin.input_reference()
    }

    #[cfg(feature = "gui")]
    pub(super) fn ai_submission_current(&self, s: usize, submission: &Submission) -> bool {
        self.ai_input_is_current(s, &submission.input)
    }
}

#[cfg(feature = "gui")]
impl Task {
    pub(super) fn status(&self, jobs: &[AiJob]) -> &'static str {
        if self.started.is_none() {
            return "未開始";
        }
        if self.interrupted.is_some() {
            return "接続中断・継続未確認";
        }
        if let Some(job) = jobs.iter().rev().find(|j| j.request == self.id) {
            match job.state.as_str() {
                "awaiting_gui_confirmation" => return "人の確認待ち",
                "running" => return "計算中",
                "failed" => return "失敗",
                "interrupted_no_result" => return "中断・結果なし",
                _ => {}
            }
        }
        if !self.submissions.is_empty() {
            "成果提出済み"
        } else {
            "作業中（内部処理は未観測）"
        }
    }
}

#[cfg(feature = "gui")]
impl App {
    pub(super) fn open_ai_request_target(
        &mut self,
        study: usize,
        task: usize,
        reference: &TargetReference,
    ) -> Result<(), String> {
        self.switch_study(Location::RequestInput(study, task))?;
        let view = match reference.target {
            ReferenceTarget::Node(_)
            | ReferenceTarget::Member(_)
            | ReferenceTarget::Story(_)
            | ReferenceTarget::Axis { .. } => references::ReferenceView::Diagram,
            ReferenceTarget::Section(_)
            | ReferenceTarget::Material(_)
            | ReferenceTarget::LoadCase(_)
            | ReferenceTarget::Combination(_) => references::ReferenceView::Table,
            ReferenceTarget::Vibration(_) | ReferenceTarget::LumpedVibration(_) => {
                references::ReferenceView::Detail
            }
        };
        self.show_reference(reference, view, true)
    }

    pub(crate) fn cancel_ai_request_inputs(&mut self, ui: &mut egui::Ui) {
        self.sync_node_edit();
        self.ui.scoped.node_grid = Default::default();
        self.ui.scoped.node_draft = ["0".into(), "0".into(), "0".into()];
        self.ui.scoped.pending_duplicate_node_coord = None;
        self.ui.scoped.section_draft = Default::default();
        self.ui.scoped.catalog_draft = Default::default();
        self.ui.scoped.isolator_support_draft = Default::default();
        self.ui.scoped.isolator_member_draft = Default::default();
        self.ui.scoped.damper_def_draft = Default::default();
        self.ui.scoped.draw_section_assignment = None;
        self.ui.scoped.region_assign_dialog = None;
        self.ui.scoped.beam_draw_first = None;
        self.ui.scoped.member_place_first = None;
        self.ui.scoped.story_copy = Default::default();
        self.ui.scoped.load_editor = None;
        self.ui.scoped.combo_draft = Default::default();
        self.ui.scoped.slab_draft = Default::default();
        self.ui.scoped.secondary_draft = Default::default();
        self.ui.scoped.new_story_draft = Default::default();
        self.ui.scoped.wall_plate_draft = Default::default();
        self.ui.scoped.axis_name_draft = Default::default();
        self.ui.scoped.load_cfg_draft = Default::default();
        self.ui.scoped.member_detail_draft = Default::default();
        self.ui.scoped.steel_attr_draft = Default::default();
        self.core.scoped.pending_plate_loss_edit = None;
        crate::tables::materials::clear_material_inputs(ui);
        let registry = egui::Id::new("numeric_input_cells");
        let cells = ui.data(|data| data.get_temp::<Vec<egui::Id>>(registry).unwrap_or_default());
        ui.data_mut(|data| {
            for cell in cells {
                data.remove::<(Option<f64>, String)>(cell);
            }
            data.remove::<Vec<egui::Id>>(registry);
        });
        ui.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    pub(crate) fn ai_request_panel(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("図・表・結果からAIへ依頼", |ui| {
            ui.label("未確定の入力は元の表・フォームで適用してください。取消して進む場合は次の操作で編集中の入力を破棄します。");
            if self.ai_connection.request_evidence.is_some(){
                ui.label("依頼元: 根拠窓で明示した参照・方向・成分を固定します。");
                if ui.button("根拠指定を解除し画面選択から作る").clicked(){self.ai_connection.request_evidence=None;self.ai_connection.draft=None;}
            }else{ui.label("依頼元: 図・表の選択対象。数値の根拠も共有する場合は、結果の方向・成分の「根拠」を開き「この根拠からAIへの依頼を作る」を選んでください。未指定成分や別の結果は追加しません。");}
            ui.text_edit_singleline(&mut self.ai_connection.purpose);
            ui.checkbox(&mut self.ai_connection.review,"根拠の確認のみ（未選択: 断面の比較案を作成）");
            if !self.ui.scoped.pending_story_cmds.is_empty(){ui.label("階の確定済み入力の反映が終わるまで待ってください。");}
            if ui.add_enabled(self.ui.scoped.pending_story_cmds.is_empty(),egui::Button::new("未確定入力を取消して対象を確認")).clicked() {
                self.cancel_ai_request_inputs(ui);
                let mode = if self.ai_connection.review { Mode::Review } else { Mode::Comparison };
                if let Err(reason)=self.prepare_ai_request(self.ai_connection.purpose.clone(),mode) { self.report_error(reason); }
            }
            if let Some(draft)=&self.ai_connection.draft {
                ui.label(format!("目的: {} / {} / {} / 入力世代 {}",draft.purpose,if draft.mode==Mode::Review {"確認のみ"}else{"比較作成"},request_scope(draft.fixed.len(),draft.quantity.as_ref()),draft.input.generation));
                ui.label(format!("案: {} / 階・通り: {}",draft.proposal_name,draft.frame));
                for target in &draft.fixed { ui.label(self.reference_target_description(target.target)); }
                ui.label(format!("計算条件: {}",draft.conditions));
                if let Some(quantity)=&draft.quantity {
                    if let references::QuantityEvidenceScope::Members(members)=&quantity.scope {
                        for id in members {ui.label(format!("数量の固定対象: {}",target_label(ReferenceTarget::Member(*id))));}
                    }
                }
                ui.label(if draft.mode==Mode::Review {"変更可能範囲: なし。比較案の作成・入力変更・計算は許可しません。"}else{"変更可能範囲: 比較案の固定鉄骨大梁への既存有効断面割当。自重等は既存計算で再算定します。"});
                if ui.button("この対象・条件で未開始の依頼を保存").clicked() {
                    if let Err(reason)=self.confirm_ai_request() { self.report_error(reason); }
                }
            }
        });
        ui.collapsing("AIへの依頼一覧・成果", |ui| {
            let mut evidence=None;
            let mut target=None;
            let mut switch=None;
            let mut compare=None;
            let mut note=None;
            for s in 0..self.studies.items.len() {
                let study=&self.studies.items[s];
                for (t,task) in study.ai.tasks.iter().enumerate() {
                    ui.push_id((&study.ai.id,&task.id),|ui| {
                        ui.separator();
                        ui.strong(format!("{} / {}",task.purpose,task.status(&study.ai.jobs)));
                        ui.label(format!("検討ID: {} / 依頼ID: {}",study.ai.id,task.id));
                        ui.label(format!("案: {} / {} / {}",task.proposal_name,request_scope(task.fixed.len(),task.quantity.as_ref()),if task.mode==Mode::Review {"根拠の確認のみ"}else{"断面の比較案を作成"}));
                        ui.label(format!("階・通り: {} / 計算条件: {}",task.frame,task.conditions));
                        if task.from_main && !self.ai_task_source_current(task){ui.label("依頼元の本案は作成時から変更されています。未開始なら元入力と対象を再確認して依頼を作り直してください。");}
                        let elapsed=task.last_operation.elapsed().map(|duration|format!("{} 秒",duration.as_secs())).unwrap_or_else(|_|"時計変更により取得不能".into());
                        ui.label(format!("最終確認操作: {} / {} / 経過 {elapsed}",operation_label(&task.last_operation_name),observed_time(task.last_operation)));
                        if let Some((time,reason))=&task.interrupted {ui.label(format!("接続観測: {reason} / {}",observed_time(*time)));}
                        ui.label("接続・コピーは開始や完了を意味しません。計算停止には未対応です。解除後も受理済み計算は継続します。");
                        if ui.button("依頼文と識別情報をコピー").clicked() { if let Ok(text)=self.ai_request_text(s,&task.id) {ui.ctx().copy_text(text);} }
                        if ui.button("依頼時の固定入力を表示（読取専用）").clicked(){switch=Some(Location::RequestInput(s,t));}
                        ui.label("人による補足・判断理由（AIの提出説明とは別）");
                        let mut text=task.note.clone();
                        if ui.text_edit_multiline(&mut text).changed(){note=Some((s,task.id.clone(),text));}
                        for reference in &task.results { if ui.button(format!("依頼時の根拠 {} を開く",result_label(reference,&task.origin.model))).clicked() {evidence=Some(references::EvidenceReference::from(reference));} }
                        if let Some(q)=&task.quantity {if ui.button("依頼時の固定数量根拠").clicked() {evidence=Some(references::EvidenceReference::Quantity(Box::new(q.clone())));}}
                        for reference in &task.source_targets {if ui.button(format!("依頼時の{} を図表で確認",target_label(reference.target))).clicked() {target=Some((s,t,reference.clone()));}}
                        for job in study.ai.jobs.iter().filter(|j|j.request==task.id) {ui.label(format!("計算ジョブ: {} / {}",job.id,calculation_label(&job.state)));if let Some(error)=&job.error{ui.label(error);}}
                        for submission in &task.submissions {
                            ui.label("AI由来の補足説明（設計判断・検定合格の保証ではありません）");
                            ui.label(&submission.explanation);
                            if submission.unchecked.is_empty(){ui.label("未確認点: AIからの申告なし（安全確認の完了を意味しません）");}else{for unchecked in &submission.unchecked {ui.label(format!("未確認: {unchecked}"));}}
                            let current=self.ai_submission_current(s,submission);
                            ui.label(if current {"成果の元入力: 現行版と一致"}else{"成果の元入力: 古い版・要再計算（保存根拠は別途確認）"});
                            let count=|value:Option<usize>|value.map(|n|n.to_string()).unwrap_or_else(||"取得不能".into());
                            ui.label(format!("提出時の計算: {} / 検定済 {}・未検定 {}・NG {}",calculation_label(&submission.calculation_state),count(submission.verification.checked),count(submission.verification.unchecked),count(submission.verification.confirmed_ng)));
                            if let Some(observed)=&submission.verification.observed_counts {ui.label(format!("{} / {} / 検定済 {}・未検定 {}・NG {}",observed.scope,observed.count_unit,observed.checked,observed.unchecked,observed.confirmed_ng));}
                            for reference in &submission.references {
                                match self.ai_validate_reference(s,&submission.input,reference,true) {Ok(())=>{ui.label("参照先: 保持根拠を取得可能");},Err(reason)=>{ui.label(format!("参照先: 取得不能 / {}",reason.message));}}
                                if ui.button(format!("提出根拠 {}",result_label(reference,&task.origin.model))).clicked(){evidence=Some(references::EvidenceReference::from(reference));}
                            }
                            for q in &submission.quantity_references {if ui.button("提出された固定数量根拠").clicked(){evidence=Some(references::EvidenceReference::Quantity(Box::new(q.clone())));}}
                        }
                        if ui.button("変更・比較表・人による採用へ").clicked(){compare=Some(s);}
                        for (p,proposal) in study.proposals.iter().enumerate(){if ui.button(format!("{} を手作業で表示",proposal.name)).clicked(){switch=Some(Location::Proposal(s,p));}}
                    });
                }
            }
            if let Some(reference)=evidence {if let Err(reason)=self.open_result_evidence(reference){self.report_error(reason);}}
            if let Some((s,id,text))=note {if let Err(reason)=self.set_ai_request_note(s,&id,text){self.report_error(reason);}}
            if let Some((s,t,reference))=target {if let Err(reason)=self.open_ai_request_target(s,t,&reference){self.report_error(reason);}}
            if let Some(next)=switch {if let Err(reason)=self.switch_study(next){self.report_error(reason);}}
            if let Some(s)=compare {match comparison::ComparisonView::new(self,s){Ok(view)=>self.studies.comparison=Some(Box::new(view)),Err(reason)=>self.report_error(reason)}}
        });
    }
}
