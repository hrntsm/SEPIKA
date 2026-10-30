use super::{projection::*, *};
use references::{EvidenceReference, ReferenceTarget};

fn check_counts_label(
    scope: &str,
    summary: &Summary,
    row_count: usize,
    partial: bool,
    no_targets: bool,
) -> String {
    if no_targets {
        return format!("{scope}: 対象なし");
    }
    if row_count == 0 {
        return format!("{scope}: 集計不可・範囲未確認");
    }
    format!(
        "{scope}{} / 検定済 {} 行・NG {} 行・未検定 {} 行",
        if partial {
            " 部分集計（取得済み範囲のみ）"
        } else {
            " 算定済み範囲"
        },
        summary.checked,
        summary.ng,
        summary.skipped
    )
}

struct Entry {
    name: String,
    input: StudyInput,
    app: App,
    rows: Vec<CheckRow>,
    state: String,
    current: bool,
    generation: u64,
    run_time: Option<SystemTime>,
    retained_run: Option<usize>,
    case_evidence: Vec<ReferencedResult>,
    volume: Result<f64, String>,
    whole_quantity_error: Option<String>,
    quantity: references::QuantityEvidenceReference,
    cases: Vec<ResultInputKey>,
    failures: Vec<String>,
}
impl Entry {
    fn new(owner: &App, study: usize, proposal: usize, fixed: &[ElemId]) -> Self {
        let input = owner.proposal_input(study, proposal);
        let retained_run = owner.studies.items[study]
            .runs
            .iter()
            .rposition(|r| r.proposal == input.references.proposal);
        let run = retained_run.map(|r| &owner.studies.items[study].runs[r]);
        let mut failures = Vec::new();
        let mut rows = Vec::new();
        let (mut app, source, current, state) = if let Some(run) = run {
            let mut app = App {
                core: run.prepared.core(),
                ..Default::default()
            };
            app.core.scoped.results = run.results.clone();
            app.core.scoped.last_static = run.last_static;
            app.validate_restored_results();
            let current = App {
                core: input.core(),
                ..Default::default()
            }
            .input_reference()
                == App {
                    core: run.input.core(),
                    ..Default::default()
                }
                .input_reference();
            let state = if let Some(reason) = &run.error {
                format!("計算失敗を含む: {reason}")
            } else if !current {
                "古い結果（現在案は再編集済み）".into()
            } else {
                "保持した計算結果".into()
            };
            for evidence in &run.comparison_checks {
                match fixed_coverage(evidence, fixed).and_then(|()| check_rows(evidence)) {
                    Ok(values) => rows.extend(values),
                    Err(reason) => failures.push(format!(
                        "{}: {reason}",
                        evidence.bundle.design_source.as_ref().map_or_else(
                            || "検定ケース不明".into(),
                            |case| case_label(case, &evidence.input.model)
                        )
                    )),
                }
            }
            if run.comparison_checks.is_empty() {
                failures.push("ケース別検定の保存根拠なし・未計算".into());
            }
            (
                app,
                run.input.clone(),
                current && run.error.is_none(),
                state,
            )
        } else {
            (
                App {
                    core: input.core(),
                    ..Default::default()
                },
                input.clone(),
                false,
                "未計算".into(),
            )
        };
        let quantity = app.member_quantity_evidence_reference(fixed.to_vec());
        let steel = quantity.steel_takeoff();
        let whole_quantity_error = steel
            .as_ref()
            .ok()
            .and_then(|q| q.whole_quantity_error.clone());
        let volume = steel.and_then(|q| {
            let value = q.volume_m3();
            if value.is_finite() {
                Ok(value)
            } else {
                Err("数量が不正です".into())
            }
        });
        let cases: Vec<_> = app
            .core
            .model
            .load_cases
            .iter()
            .map(|case| {
                ResultInputKey::Static(
                    app.standard_lateral_case(case.id)
                        .unwrap_or(StaticCaseKey::User(case.id)),
                )
            })
            .chain(
                app.core
                    .model
                    .combinations
                    .iter()
                    .map(|combo| ResultInputKey::Combo(combo.name.clone())),
            )
            .collect();
        if let Some(run) = run {
            for case in &cases {
                if require_static_result(&app, case).is_err() {
                    failures.push(format!(
                        "静的結果の欠落・古い・失敗: {}",
                        case_label(case, &app.core.model)
                    ));
                }
                if !run
                    .comparison_checks
                    .iter()
                    .any(|e| e.bundle.design_source.as_ref() == Some(case))
                {
                    failures.push(format!(
                        "未評価ケース: {}",
                        case_label(case, &app.core.model)
                    ));
                }
            }
        }
        app.core.scoped.undo.restrict_sections(Vec::new());
        app.studies.active = Location::Proposal(0, 0);
        Self {
            name: owner.studies.items[study].proposals[proposal].name.clone(),
            input: source,
            app,
            rows,
            state,
            current,
            generation: input.references.generation,
            run_time: run.map(|run| run.created),
            retained_run,
            case_evidence: run.map(|r| r.comparison_checks.clone()).unwrap_or_default(),
            volume,
            whole_quantity_error,
            quantity,
            cases,
            failures,
        }
    }
    fn key(&self, kind: &ResultInputKey) -> Option<StaticKey> {
        match kind {
            ResultInputKey::Static(key) => self
                .app
                .core
                .scoped
                .results
                .as_ref()?
                .statics
                .iter()
                .any(|(k, _)| k == key)
                .then_some(StaticKey::Case(*key)),
            ResultInputKey::Combo(name) => self
                .app
                .core
                .scoped
                .results
                .as_ref()?
                .combos
                .iter()
                .position(|(n, _)| n == name)
                .map(StaticKey::Combo),
            _ => None,
        }
    }
    fn compatible(&self, baseline: &Self, fixed: &[ElemId]) -> bool {
        self.current
            && baseline.current
            && self.failures.is_empty()
            && baseline.failures.is_empty()
            && same_conditions(&baseline.input, &self.input, fixed)
            && self.cases == baseline.cases
    }
}

pub(in crate::app) struct ComparisonView {
    study: usize,
    entries: Vec<Entry>,
    fixed: Vec<ElemId>,
    sides: [usize; 2],
    viewports: [crate::viewer::ViewportState; 2],
    node: u32,
    component: usize,
    case: Option<ResultInputKey>,
    envelope: bool,
    common_deform: bool,
    deform_scale: f64,
    comparable: Vec<bool>,
    differences: Vec<Vec<String>>,
    displacements: Vec<Result<(f64, EvidenceReference), String>>,
    impacts: Vec<Vec<MemberImpact>>,
    confirmation: Option<adoption::Confirmation>,
    reason: String,
}
impl ComparisonView {
    pub(in crate::app) fn new(owner: &App, study: usize) -> Result<Self, String> {
        let item = owner.studies.items.get(study).ok_or("検討なし")?;
        let fixed = fixed_members(item);
        for target in &item.fixed {
            owner.resolve_study_fixed_target(study, target)?;
        }
        let entries: Vec<_> = (0..item.proposals.len())
            .map(|p| Entry::new(owner, study, p, &fixed))
            .collect();
        let case = entries[0].cases.first().cloned();
        let right = usize::from(entries.len() > 1);
        let node = entries[0].input.model.nodes.last().map_or(0, |n| n.id.0);
        let comparable = entries
            .iter()
            .map(|e| e.compatible(&entries[0], &fixed))
            .collect();
        let differences = entries
            .iter()
            .map(|e| condition_differences(&entries[0].input, &e.input, &fixed))
            .collect();
        let impacts = entries
            .iter()
            .map(|e| member_impacts(&entries[0].rows, &e.rows, &fixed))
            .collect();
        let mut view = Self {
            study,
            entries,
            fixed,
            sides: [0, right],
            viewports: Default::default(),
            node,
            component: 0,
            case,
            envelope: true,
            common_deform: true,
            deform_scale: 100.0,
            comparable,
            differences,
            displacements: Vec::new(),
            impacts,
            confirmation: None,
            reason: String::new(),
        };
        view.bind_case();
        view.refresh_displacements();
        Ok(view)
    }
    fn bind_case(&mut self) {
        if let Some(case) = &self.case {
            for entry in &mut self.entries {
                if let Some(evidence) = entry
                    .case_evidence
                    .iter()
                    .find(|e| e.bundle.design_source.as_ref() == Some(case))
                {
                    entry.app = result_app(&evidence.input, &evidence.bundle, evidence.last_static);
                    if let Err(reason) = fixed_coverage(evidence, &self.fixed)
                        .and_then(|()| validate_expected_checks(evidence))
                    {
                        entry.app.reject_incomplete_study_design(reason);
                    }
                } else {
                    entry.app.core.scoped.last_static = entry.key(case);
                    if let Some(bundle) = &mut entry.app.core.scoped.results {
                        bundle.clear_design_results();
                    }
                    entry.app.core.scoped.staleness.design_stale = true;
                }
            }
        }
    }
    fn refresh_displacements(&mut self) {
        self.displacements = self
            .entries
            .iter()
            .map(|entry| {
                let kind = self.case.as_ref().ok_or("指定ケースなし")?;
                let key = entry.key(kind).ok_or("指定結果欠落")?;
                displacement(&entry.app, key, NodeId(self.node), self.component)
            })
            .collect();
    }
    fn focus(&mut self, target: ReferenceTarget) {
        for entry in &mut self.entries {
            match target {
                ReferenceTarget::Member(id) => entry
                    .app
                    .ui
                    .scoped
                    .selection
                    .select_members(vec![id], Some(id)),
                ReferenceTarget::Node(id) => entry
                    .app
                    .ui
                    .scoped
                    .selection
                    .select_nodes(vec![id], Some(id)),
                _ => {}
            }
        }
    }
    fn show(&mut self, ui: &mut egui::Ui, owner: &mut App) {
        for (p, entry) in self.entries.iter_mut().enumerate() {
            let core = if owner.studies.active == Location::Proposal(self.study, p) {
                Some(&owner.core)
            } else {
                owner.studies.items[self.study]
                    .proposals
                    .get(p)
                    .and_then(|p| p.state.as_ref())
                    .map(|s| &s.core)
            };
            let generation = core
                .filter(|core| core.scoped.references.proposal == entry.input.references.proposal)
                .map(|core| {
                    core.scoped.references.generation + core.scoped.undo.revision()
                        - core.scoped.reference_preparation_revisions
                });
            let run_time = owner.studies.items[self.study]
                .runs
                .iter()
                .rev()
                .find(|r| r.proposal == entry.input.references.proposal)
                .map(|r| r.created);
            if generation != Some(entry.generation) || run_time != entry.run_time {
                entry.current = false;
                entry.state = "現在案の編集または新しい計算があります。表示中の保持入力・結果は固定です。再表示してください。".into();
                self.comparable[p] = false;
            }
        }
        if !self.entries[0].current {
            self.comparable.fill(false);
        }
        ui.strong("断面案の比較（読取専用）");
        ui.label("差はすべて『案−基準案』。図の左右の差ではありません。順位・改善率・変位の改善判定は行いません。");
        ui.label("保持した時点を表示します。現在案を編集・再計算した後は『再表示』してください。");
        ui.horizontal(|ui| {
            for side in 0..2 {
                egui::ComboBox::from_id_salt(("comparison_side", side))
                    .selected_text(format!(
                        "{}: {}",
                        ["左", "右"][side],
                        self.entries[self.sides[side]].name
                    ))
                    .show_ui(ui, |ui| {
                        for (p, entry) in self.entries.iter().enumerate() {
                            ui.selectable_value(&mut self.sides[side], p, &entry.name);
                        }
                    });
            }
            if ui.button("再表示").clicked() {
                if let Ok(view) = Self::new(owner, self.study) {
                    *self = view;
                }
            }
        });
        ui.separator();
        ui.strong("本案への採否（人による確認）");
        ui.label("採用は検定OKを意味しません。以下の変更・固定条件・全体への影響・未確認事項と注意を確認してください。");
        for (p, entry) in self.entries.iter().enumerate().skip(1) {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("{} の結果と断面変更を確認して採否へ", entry.name))
                    .clicked()
                {
                    let result = entry
                        .retained_run
                        .ok_or_else(|| "未計算です。案の計算へ戻ってください".to_string())
                        .and_then(|r| {
                            owner.confirm_study_run(
                                self.study,
                                p,
                                &owner.studies.items[self.study].runs[r],
                            )
                        });
                    match result {
                        Ok(mut value) => {
                            value.baseline_run = self.entries[0]
                                .retained_run
                                .map(|r| owner.studies.items[self.study].runs[r].clone());
                            value.case = self.case.clone();
                            value.node = self.node;
                            value.component = self.component;
                            value.envelope = self.envelope;
                            self.confirmation = Some(value);
                            self.reason.clear();
                        }
                        Err(reason) => owner.report_error(reason),
                    }
                }
                if ui.button(format!("{} を計算", entry.name)).clicked() {
                    if let Err(reason) = owner.start_study_calculation(self.study, p) {
                        owner.report_error(reason);
                    }
                }
            });
            if let Some(r) = entry.retained_run {
                let run = &owner.studies.items[self.study].runs[r];
                ui.collapsing(
                    format!("{} の既存警告・診断・未確認事項", entry.name),
                    |ui| {
                        for log in &run.log {
                            if log.level != LogLevel::Info {
                                ui.label(&log.message);
                            }
                        }
                        for diagnostic in &run.diagnostics {
                            ui.label(&diagnostic.message);
                        }
                        if let Some(notice) = &run.notice {
                            ui.label(notice);
                        }
                        if let Some(error) = &run.error {
                            ui.label(error);
                        }
                        for failure in &entry.failures {
                            ui.label(failure);
                        }
                    },
                );
            }
        }
        let mut decision = None;
        if let Some(confirmation) = &self.confirmation {
            ui.label(format!(
                "確認済み: {} / 計算時入力世代 {} / 変更一式のみを反映",
                confirmation.run.proposal_name, confirmation.input.references.generation
            ));
            ui.label("理由（任意）");
            ui.text_edit_multiline(&mut self.reason);
            ui.horizontal_wrapped(|ui| {
                for (accepted, label) in [
                    (true, "確認済みの変更一式を本案へ採用"),
                    (false, "不採用として判断を記録"),
                ] {
                    if ui.button(label).clicked() {
                        decision = Some(accepted);
                    }
                }
            });
        }
        if let Some(accepted) = decision {
            let confirmation = self.confirmation.take().expect("確認済み");
            if let Err(reason) =
                owner.decide_study(self.study, confirmation, accepted, self.reason.clone())
            {
                owner.report_error(reason);
            }
        }
        if owner.study_main_changed(self.study) {
            ui.colored_label(
                crate::theme::WARN_TEXT,
                "本案が基準と異なります。新しい基準で検討してください。",
            );
            if ui.button("本案を表示して新しい基準の検討へ").clicked() {
                match owner.switch_study(Location::Main) {
                    Ok(()) => {
                        owner.clear_geometry_selection();
                        owner.studies.window_open = true;
                        owner.report_info(
                            "本案で対象の鉄骨大梁を選び、新しい検討を作成してください",
                        );
                    }
                    Err(reason) => owner.report_error(reason),
                }
            }
        }
        let mut case_changed = false;
        let mut displacement_changed = false;
        let mut envelope_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("指定ケース / 組合せ:");
            egui::ComboBox::from_id_salt("comparison_case")
                .selected_text(self.case.as_ref().map_or_else(
                    || "未計算・指定なし".into(),
                    |case| case_label(case, &self.entries[0].app.core.model),
                ))
                .show_ui(ui, |ui| {
                    for kind in &self.entries[0].cases {
                        case_changed |= ui
                            .selectable_value(
                                &mut self.case,
                                Some(kind.clone()),
                                case_label(kind, &self.entries[0].app.core.model),
                            )
                            .changed();
                    }
                });
            ui.label("変位の節点");
            displacement_changed |= ui.add(egui::DragValue::new(&mut self.node)).changed();
            for (i, name) in ["全体X", "全体Y", "全体Z"].into_iter().enumerate() {
                displacement_changed |= ui.selectable_value(&mut self.component, i, name).changed();
            }
            envelope_changed = ui
                .checkbox(
                    &mut self.envelope,
                    "検定は同ケース集合内の最大（オフ: 指定ケースのみ）",
                )
                .changed();
        });
        if case_changed || displacement_changed || envelope_changed {
            self.confirmation = None;
        }
        if case_changed {
            self.bind_case();
        }
        if case_changed || displacement_changed {
            self.refresh_displacements();
        }
        let mut evidence = None;
        let mut target = None;
        egui::ScrollArea::vertical().id_salt("comparison_metrics").max_height(310.0).show(ui, |ui| {
            for (p, entry) in self.entries.iter().enumerate() {
                ui.separator(); ui.strong(&entry.name); ui.label(&entry.state);
                ui.label(if entry.current { "数量の集計元: 保持した計算の準備後入力" } else { "数量の集計元: 表示中の固定入力（未計算または古い履歴。解析・検定成功とは別）" });
                for difference in &self.differences[p] { ui.colored_label(crate::theme::WARN_TEXT, difference); }
                let compatible = self.comparable[p];
                ui.label(if compatible { "支点・ユーザー入力荷重・解析／検定設定・対象・ケース定義集合: 基準案と一致" } else { "条件・対象・ケース集合の不一致、未計算、失敗または古い結果: 差・悪化・新規NGを判定しません" });
                ui.label(format!("計算開始時入力世代 {} / 案 {}", entry.input.references.generation, entry.input.references.proposal));
                ui.horizontal(|ui| {
                    match &entry.volume {
                        Ok(value) => {
                            let delta = if compatible { self.entries[0].volume.as_ref().ok().map(|b| format!(" / 案−基準 {:+.6} m³", value-b)).unwrap_or_default() } else { String::new() };
                            if ui.button(format!("固定対象 {} 部材の鋼材体積 {value:.6} m³{delta} / 根拠", self.fixed.len())).clicked() { evidence = Some(EvidenceReference::Quantity(Box::new(entry.quantity.clone()))); }
                        }
                        Err(reason) => { ui.label(format!("対象鋼材体積: {reason}")); }
                    }
                    if let Some(reason)=&entry.whole_quantity_error {
                        ui.label(format!("全体数量は未算定: {reason}。上記は固定対象の鋼材のみです"));
                    }
                });
                match &self.displacements[p] {
                    Ok((value, reference)) => {
                        let delta = if compatible { self.displacements[0].as_ref().ok().map(|(base, _)| format!(" / 案−基準 {:+.6} mm", value-base)).unwrap_or_default() } else { String::new() };
                        if ui.button(format!("節点{} 全体{} {value:+.6} mm{delta} / 根拠", self.node, ["X","Y","Z"][self.component])).clicked() { evidence = Some(reference.clone()); }
                    }
                    Err(reason) => { ui.label(format!("指定変位: {reason}")); }
                }
                let selected: Vec<_> = entry.rows.iter().filter(|row| self.envelope || self.case.as_ref().is_some_and(|case| row.case == case_label(case, &entry.app.core.model))).collect();
                let baseline_rows: Vec<_> = self.entries[0].rows.iter().filter(|row| self.envelope || self.case.as_ref().is_some_and(|case| row.case == case_label(case, &self.entries[0].app.core.model))).collect();
                let fixed_selected = fixed_rows(selected.iter().copied(), &self.fixed);
                let fixed_baseline = fixed_rows(baseline_rows.iter().copied(), &self.fixed);
                let fixed_summary = summarize(fixed_selected.iter().copied());
                let fixed_base_summary = summarize(fixed_baseline.iter().copied());
                if !entry.failures.is_empty() { ui.colored_label(crate::theme::WARN_TEXT,"取得不能の範囲は行数も未確定です。以下の件数は取得できた行のみで、欠落範囲を未検定0行として扱いません。"); }
                ui.strong(format!("検討の固定対象 {} 部材の検定比（主架構全体とは別）", self.fixed.len()));
                ui.label("NGと未検定は同じ行に併存します。不完全な行は差・新規NGを判定しません。");
                ui.label(check_counts_label("固定対象", &fixed_summary, fixed_selected.len(), !entry.failures.is_empty(), self.fixed.is_empty()));
                if let Some(max) = &fixed_summary.maximum {
                    if compatible && same_evaluation_range(fixed_baseline.iter().copied(), fixed_selected.iter().copied()) {
                        if let Some(base) = &fixed_base_summary.maximum {
                            ui.label(format!("固定対象 最大検定比 案−基準 {:+.6} / {}", max.ratio.unwrap()-base.ratio.unwrap(), if max.case != base.case {"支配ケース変更（集合・定義は同じ）"} else {"支配ケースは同じ"}));
                        }
                    } else { ui.label("固定対象の条件・項目・評価範囲不一致または未評価のため差は比較しません"); }
                    if ui.button(format!("固定対象の算定済み範囲最大 {:.6} / {} / {} / xi {} / 支配項目 {} / 根拠", max.ratio.unwrap(), max.case, target_label(max.target), value_label(max.position), governing_label(max))).clicked() { target=max.target; evidence=max.evidence.as_ref().map(Into::into); }
                } else { ui.label("固定対象の最大検定比: 対象なし・未計算・未検定・欠落"); }
                ui.strong("今回実行した主架構検定全体");
                let summary = summarize(selected.iter().copied());
                ui.label(format!("{} / {}", check_counts_label("主架構", &summary, selected.len(), !entry.failures.is_empty(), false), if self.envelope {"同ケース集合内の最大"} else {"指定ケースのみ"}));
                if let Some(max) = summary.maximum {
                    if compatible {

                        if !same_evaluation_range(baseline_rows.iter().copied(), selected.iter().copied()) { ui.label("検定項目・対象・ケース集合または実算定／未検定位置の範囲が異なるため最大値の差・支配ケース変化は比較しません"); }
                        else if let Some(base) = summarize(baseline_rows.iter().copied()).maximum {
                            ui.label(format!("最大検定比 案−基準 {:+.6} / {}", max.ratio.unwrap()-base.ratio.unwrap(), if max.case != base.case { "支配ケース変更（集合・定義は同じ）" } else { "支配ケースは同じ" }));
                        }
                    }
                    if ui.button(format!("算定済み範囲の最大 {:.6} / {} / {} / 位置 {} / 支配項目 {} / 根拠", max.ratio.unwrap(), max.case, target_label(max.target), value_label(max.position), governing_label(&max))).clicked() {
                        evidence = max.evidence.as_ref().map(Into::into); target = max.target;
                    }
                } else { ui.label("最大検定比: 未計算・欠落・未検定（0や合格にはしません）"); }
                for reason in &entry.failures { ui.colored_label(crate::theme::WARN_TEXT, reason); }
                if compatible && p > 0 {
                    let (before_unmatched, after_unmatched) = unmatched_positions(baseline_rows.iter().copied(), selected.iter().copied());
                    if before_unmatched + after_unmatched > 0 { ui.label(format!("位置別はxi・項目集合完全一致のみ比較。基準{before_unmatched}・案{after_unmatched}位置は移動・集合差により未比較です。部材×ケースの全位置最大は別集計です。")); }
                    for impact in &self.impacts[p] {
                        if !self.envelope && !self.case.as_ref().is_some_and(|case| impact.after.case == case_label(case, &entry.app.core.model)) { continue; }
                        let before = impact.before.as_ref().map_or_else(|| "基準未評価".into(), |r| format!("基準 {:.6} (xi {} / {})", r.ratio.unwrap(), value_label(r.position), governing_label(r)));
                        let state = if impact.items_changed { "検定項目集合不一致・差／悪化／新規NGは比較不可" } else if impact.difference.is_none() { if impact.after.ratio.unwrap() > 1.0 { "案側NG・基準未評価あり（評価範囲比較不可）" } else { "未検定・欠落により評価範囲比較不可" } } else if impact.new_ng { "新規NG" } else if impact.after.ratio.unwrap() > 1.0 && (impact.positions_changed || impact.skipped_before > 0 || impact.before.is_none()) { "案側NG・基準未評価／未比較あり" } else { "悪化（算定済み範囲）" };
                        ui.label(format!("変更外 部材×ケース全位置最大 / {} / Δ比 {} / {} / 未検定 基準{}・案{}", row_label(&impact.after), value_label(impact.difference), state, impact.skipped_before, impact.skipped_after));
                        ui.horizontal_wrapped(|ui| {
                            if let Some(base) = &impact.before {
                                if ui.button(format!("{before} / 基準の根拠")).clicked() { target=base.target; evidence=base.evidence.as_ref().map(Into::into); }
                            } else { ui.label(&before); }
                            if ui.button(format!("案 {:.6} (xi {} / {}) / 案の根拠", impact.after.ratio.unwrap(), value_label(impact.after.position), governing_label(&impact.after))).clicked() { target=impact.after.target; evidence=impact.after.evidence.as_ref().map(Into::into); }
                        });
                    }
                    for (row, delta, ng) in worsening(&self.entries[0].rows, selected.iter().copied(), &self.fixed) {
                        if ui.button(format!("変更外 位置別（xi一致）{} / Δ比 {delta:+.6} / {} / 移動・根拠", row_label(row), if ng {"新規NG"} else {"悪化"})).clicked() { target = row.target; evidence = row.evidence.as_ref().map(Into::into); }
                    }
                }
                ui.collapsing(format!("{} の未検定・集計対象行", entry.name), |ui| {
                    for row in &selected {
                        if ui.button(format!("{} / {} / {}", row_label(row), row.ratio.map_or_else(|| "未検定".into(), |r| format!("{r:.6}")), row.reason)).clicked() { target = row.target; evidence = row.evidence.as_ref().map(Into::into); }
                    }
                });
                ui.collapsing(format!("{} の断面変更（表示中の計算時入力）", entry.name), |ui| {
                    let current = &entry.input;
                    let baseline = &owner.studies.items[self.study].baseline.model;
                    let mut changes = Vec::new();
                    for id in &self.fixed {
                        if let (Some(before), Some(after)) = (baseline.element(*id), current.model.element(*id)) {
                            if before.section == after.section { continue; }

                            let old = before.section.and_then(|sid| baseline.section(sid)).map_or("未割当", |s| s.name.as_str());
                            let new = after.section.and_then(|sid| current.model.section(sid)).map_or("未割当", |s| s.name.as_str());
                            let story = baseline.member_story(before).and_then(|sid| baseline.stories.iter().find(|s| s.id == sid)).map_or("階未割当", |s| s.name.as_str());
                            changes.push((*id, old, story, new));
                        }
                    }
                    use crate::table_util::Col;
                    ui.label("部材符号欄は基準案の断面名。部材の識別は部材番号を併記します。");
                    crate::table_util::standard_table(ui, &format!("study_changes_{p}"), &[Col::id_named("部材"), Col::name("符号（基準断面名）"), Col::label("階"), Col::name("変更前断面"), Col::name("変更後断面"), Col::num("対象数")], changes.len(), |row| {
                        let (id, old, story, new) = changes[row.index()];
                        row.col(|ui| { if ui.button(id.0.to_string()).clicked() { target = Some(ReferenceTarget::Member(id)); } });
                        row.col(|ui| { ui.label(old); }); row.col(|ui| { ui.label(story); }); row.col(|ui| { ui.label(old); }); row.col(|ui| { ui.label(new); }); row.col(|ui| { ui.label("1"); });
                    });
                    ui.label(format!("変更対象数: {} 部材 / 固定対象 {} 部材", changes.len(), self.fixed.len()));
                });
                ui.collapsing(format!("{} の派生自重・計算条件", entry.name), |ui| {
                    ui.label("入力荷重・支点・設定の一致は計算開始時の明示入力で照合します。断面に応じて再算定した自重・自動荷重・質量の差は条件不一致と区別します。");
                    ui.label(format!("開始時の解析／検定設定: {}", if self.differences[p].is_empty() {"一致"} else {"不一致"}));
                    for case in &entry.app.core.model.load_cases {
                        let sum: Vec<_> = case.nodal.iter().filter(|load| load.source.is_auto()).map(|l| l.values).collect();
                        let values = [0,1,2].map(|i| sum.iter().map(|v| v[i]).sum::<f64>());
                        let base = self.entries[0].app.core.model.load_cases.iter().find(|c| c.id == case.id);
                        let difference = base.map(|base| [0,1,2].map(|i| values[i]-base.nodal.iter().filter(|l| l.source.is_auto()).map(|l| l.values[i]).sum::<f64>()));
                        ui.label(format!("{} / 派生節点荷重 {} 件 / 合計XYZ {:?} N / 案−基準 {:?} N", case.name, sum.len(), values, difference.map_or_else(|| "比較元欠落".into(), |v| format!("{v:?}"))));
                        let auto_members: Vec<_> = case.member.iter().filter(|l| l.source.is_auto()).collect();
                        let member_changed = base.map(|base| bincode::serialize(&base.member.iter().filter(|l| l.source.is_auto()).collect::<Vec<_>>()).unwrap() != bincode::serialize(&auto_members).unwrap());
                        ui.label(format!("派生部材荷重 {} 件 / 基準からの変更 {}", auto_members.len(), member_changed.map_or("比較元欠落", |changed| if changed {"差あり"} else {"一致"})));
                    }
                    for story in &entry.app.core.model.stories {
                        let base = self.entries[0].app.core.model.stories.iter().find(|s| s.id == story.id).and_then(|s| s.seismic_weight);
                        ui.label(format!("{} 準備後の地震用階重量 {} N / 案−基準 {} N（手入力優先を含む）", story.name, value_label(story.seismic_weight), value_label(story.seismic_weight.zip(base).map(|(a,b)| a-b))));
                    }
                    if ui.button("保持した計算時入力・通常結果を表示").clicked() {
                        if let Some(r) = entry.retained_run {
                            if let Err(reason) = owner.switch_study(Location::History(self.study, r)) { owner.report_error(reason); }
                        }
                    }
                });
            }
        });
        ui.label("集計粒度: 部材×検定位置×ケース、接合部種別×節点×ケース、耐震壁項目×ケース。数値は既存検定の算定済み範囲のみ。床板・二次部材の設計、未実行ケース、動的・終局・保有耐力は集計外。未検定と保存根拠欠落を含むため設計全体の確認完了ではありません。");
        ui.horizontal_wrapped(|ui| {
            if ui.button("左の方向・構面・図倍率を両図へ揃える").clicked() {
                self.viewports[1].camera = self.viewports[0].camera.clone();
                self.viewports[1].frame_target = self.viewports[0].frame_target;
                self.viewports[1].fit_scale = self.viewports[0].fit_scale;
            }
            ui.checkbox(&mut self.common_deform, "変形の絶対表示倍率を共通にする");
            ui.add(egui::DragValue::new(&mut self.deform_scale).range(0.01..=100000.0).suffix("倍"));
            ui.label("応力コンター: 個別自動範囲。検定比: 共通0〜1、1超NG色。色だけで比較しないでください。");
        });
        for view in &mut self.viewports {
            view.fixed_deform_scale = self.common_deform.then_some(self.deform_scale);
        }
        let height = 360.0;
        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            ui.columns(2, |columns| {
                for (side, column) in columns.iter_mut().enumerate() {
                    let entry = &mut self.entries[self.sides[side]];
                    column.strong(format!(
                        "{} / 読取専用 / {}",
                        entry.name,
                        if self.common_deform {
                            format!("共通絶対倍率 {}倍", self.deform_scale)
                        } else {
                            format!(
                                "個別自動倍率 × {}",
                                self.viewports[side].deform_scale_factor
                            )
                        }
                    ));
                    column.label(format!(
                        "図の荷重状態: {}（上部共通指定・単一ケースの図）",
                        self.case.as_ref().map_or_else(
                            || "未計算・指定なし".into(),
                            |case| case_label(case, &entry.app.core.model)
                        )
                    ));
                    self.viewports[side].allow_creation = false;
                    self.viewports[side].static_case_locked = true;
                    if let Some(case) = &self.case {
                        self.viewports[side].static_key = entry.key(case);
                        self.viewports[side].static_unavailable = entry.key(case).is_none();
                    }
                    column.push_id(("study_compare_diagram", side), |ui| {
                        crate::viewer::viewer_panel_with_view(
                            &mut self.viewports[side],
                            ui,
                            &mut entry.app,
                        )
                    });
                }
            });
        });
        if let Some(target) = target {
            self.focus(target);
        }
        if let Some(reference) = evidence {
            if let Err(reason) = owner.open_result_evidence(&reference) {
                owner.report_error(reason);
            }
        }
    }
}
fn governing_label(row: &CheckRow) -> String {
    row.governing_items
        .iter()
        .map(|k| k.label())
        .collect::<Vec<_>>()
        .join("／")
}
fn row_label(row: &CheckRow) -> String {
    row.position.map_or_else(
        || row.key.clone(),
        |xi| format!("{} / {} / xi {xi:.6}", row.case, target_label(row.target)),
    )
}
fn value_label(value: Option<f64>) -> String {
    value.map_or_else(|| "未算定".into(), |v| format!("{v:.6}"))
}
fn target_label(target: Option<ReferenceTarget>) -> String {
    match target {
        Some(ReferenceTarget::Member(id)) => format!("部材{}", id.0),
        Some(ReferenceTarget::Node(id)) => format!("節点{}", id.0),
        _ => "対象参照なし".into(),
    }
}
impl App {
    pub(in crate::app) fn study_comparison_window(&mut self, ctx: &egui::Context) {
        let Some(mut view) = self.studies.comparison.take() else {
            return;
        };
        let mut open = true;
        egui::Window::new("断面変更・計算結果の比較")
            .default_size([1100.0, 780.0])
            .resizable(true)
            .vscroll(true)
            .open(&mut open)
            .show(ctx, |ui| view.show(ui, self));
        if open {
            self.studies.comparison = Some(view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::tests::assign;
    use super::super::tests::computed;
    use super::*;

    #[test]
    fn comparison_frames_and_evidence_keep_model_results_and_saved_inputs_unchanged() {
        let mut app = computed();
        let before = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let mut view = ComparisonView::new(&app, 0).unwrap();
        assert!(view.entries[1].compatible(&view.entries[0], &view.fixed));
        for entry in &view.entries {
            assert!(entry.app.study_read_only());
            assert!(!entry.rows.is_empty());
            for row in &entry.rows {
                if let Some(reference) = &row.evidence {
                    app.open_result_evidence(reference).unwrap();
                }
            }
        }
        let key = StaticKey::Case(StaticCaseKey::User(LoadCaseId(0)));
        let (negative, reference) = displacement(&view.entries[1].app, key, NodeId(3), 2).unwrap();
        assert!(negative < 0.0);
        app.open_result_evidence(&reference).unwrap();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            view.show(ui, &mut app);
        });
        assert_eq!(
            before,
            rmp_serde::to_vec_named(&app.saved_studies()).unwrap()
        );
        assert!(view
            .viewports
            .iter()
            .all(|v| v.fixed_deform_scale == Some(100.0)));
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 1));
        let stale = Entry::new(&app, 0, 1, &view.fixed);
        assert!(!stale.current);
        assert!(!stale.compatible(&view.entries[0], &view.fixed));
        assert_eq!(stale.input.model.elements[2].section, Some(SectionId(2)));
        assert_eq!(stale.app.core.model.elements[2].section, Some(SectionId(2)));
        let edited = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| view.show(ui, &mut app));
        assert!(!view.comparable[1]);
        assert_eq!(
            edited,
            rmp_serde::to_vec_named(&app.saved_studies()).unwrap()
        );
    }

    #[test]
    fn same_case_missing_from_both_sides_and_failed_or_old_results_are_explicitly_incomplete() {
        let mut app = computed();
        for run in &mut app.studies.items[0].runs {
            let missing = run.results.as_ref().unwrap().statics[0].0;
            run.results.as_mut().unwrap().statics.remove(0);
            run.comparison_checks
                .retain(|e| e.bundle.design_source != Some(ResultInputKey::Static(missing)));
        }
        let view = ComparisonView::new(&app, 0).unwrap();
        assert!(!view.entries[1].compatible(&view.entries[0], &view.fixed));
        assert!(view.entries[0].failures.iter().any(|s| s.contains("欠落")));
        assert!(view.entries[1]
            .failures
            .iter()
            .any(|s| s.contains("未評価")));
        app.studies.items[0].runs[1].error = Some("solver失敗".into());
        let entry = Entry::new(&app, 0, 1, &view.fixed);
        assert!(!entry.current);
        assert!(entry.state.contains("失敗"));
        for e in &mut app.studies.items[0].runs[2].comparison_checks {
            e.bundle.design_contract_version = Some(0);
        }
        let entry = Entry::new(&app, 0, 2, &view.fixed);
        assert!(entry.rows.is_empty());
        assert!(!entry.failures.is_empty());
    }

    #[test]
    #[ignore = "Issue980実機確認専用の正常入力artifactを明示生成する"]
    fn write_issue980_manual_fixture() {
        let mut app = computed();
        let view = ComparisonView::new(&app, 0).unwrap();
        for entry in &view.entries {
            let summary = summarize(&entry.rows);
            println!(
                "{} volume={:?} max={:?} counts=({},{},{}) failures={:?}",
                entry.name,
                entry.volume,
                summary
                    .maximum
                    .as_ref()
                    .map(|r| (&r.case, r.target, r.position, r.ratio)),
                summary.checked,
                summary.ng,
                summary.skipped,
                entry.failures
            );
            if let Some(case) = &view.case {
                println!(
                    "disp {:?}",
                    entry
                        .key(case)
                        .and_then(|k| displacement(&entry.app, k, NodeId(3), 2).ok())
                        .map(|r| r.0)
                );
            }
        }
        app.switch_study(Location::Main).unwrap();
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/issue980-artifacts");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("comparison-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        let restored = ComparisonView::new(&loaded, 0).unwrap();
        assert_eq!(restored.entries[1].volume, view.entries[1].volume);
        assert_eq!(restored.entries[1].rows, view.entries[1].rows);
        println!("fixture={}", path.display());
    }
    #[test]
    fn diagram_case_controls_keep_common_case_displacement_and_checks_consistent_across_frames() {
        fn text(shape: &egui::Shape, result: &mut String) {
            match shape {
                egui::Shape::Text(t) => result.push_str(t.galley.text()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, result);
                    }
                }
                _ => {}
            }
        }
        let mut app = computed();
        let before = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let mut view = ComparisonView::new(&app, 0).unwrap();
        view.case = Some(ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(1))));
        view.bind_case();
        view.refresh_displacements();
        for viewport in &mut view.viewports {
            viewport.view_mode = crate::viewer::ViewMode::Deformed;
        }
        let ctx = egui::Context::default();
        let mut rendered = String::new();
        for _ in 0..3 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(2200.0, 1800.0),
                    )),
                    ..Default::default()
                },
                |ui| view.show(ui, &mut app),
            );
            for shape in output.shapes {
                text(&shape.shape, &mut rendered);
            }
            for (side, viewport) in view.viewports.iter().enumerate() {
                assert!(viewport.static_case_locked);
                assert_eq!(
                    viewport.static_key,
                    Some(StaticKey::Case(StaticCaseKey::User(LoadCaseId(1))))
                );
                assert_eq!(
                    view.entries[view.sides[side]].app.core.scoped.last_static,
                    viewport.static_key
                );
            }
        }
        assert!(rendered.contains("上部の共通指定"), "{rendered}");
        for (p, entry) in view.entries.iter().enumerate() {
            let value = displacement(
                &entry.app,
                StaticKey::Case(StaticCaseKey::User(LoadCaseId(1))),
                NodeId(view.node),
                view.component,
            )
            .unwrap()
            .0;
            assert_eq!(view.displacements[p].as_ref().unwrap().0, value);
        }
        assert_eq!(
            before,
            rmp_serde::to_vec_named(&app.saved_studies()).unwrap()
        );
    }
    #[test]
    fn case_switch_and_normal_result_navigation_keep_the_constructed_run_until_explicit_refresh() {
        use super::super::super::tests::wait;
        let mut app = computed();
        let mut view = ComparisonView::new(&app, 0).unwrap();
        view.node = 3;
        view.component = 0;
        let old_run = view.entries[1].retained_run.unwrap();
        let original = rmp_serde::to_vec_named(&app.studies.items[0].runs[old_run]).unwrap();
        let input = rmp_serde::to_vec_named(&view.entries[1].input).unwrap();
        let quantity = view.entries[1].quantity.identity.clone();
        let volume = view.entries[1].volume.clone();
        let rows = view.entries[1].rows.clone();
        let old_displacement = app.studies.items[0].runs[old_run]
            .results
            .as_ref()
            .unwrap()
            .statics
            .iter()
            .find(|(k, _)| *k == StaticCaseKey::User(LoadCaseId(1)))
            .unwrap()
            .1
            .disp[3][0];
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 1));
        app.start_study_calculation(0, 1).unwrap();
        wait(&mut app);
        let new_run = app.studies.items[0].runs.len() - 1;
        assert_ne!(old_run, new_run);
        let new_displacement = app.studies.items[0].runs[new_run]
            .results
            .as_ref()
            .unwrap()
            .statics
            .iter()
            .find(|(k, _)| *k == StaticCaseKey::User(LoadCaseId(1)))
            .unwrap()
            .1
            .disp[3][0];
        assert!((old_displacement - new_displacement).abs() > 0.1);
        view.case = Some(ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(1))));
        view.bind_case();
        view.refresh_displacements();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| view.show(ui, &mut app));
        }
        assert_eq!(
            view.entries[1].app.core.model.elements[2].section,
            Some(SectionId(2))
        );
        assert_eq!(
            rmp_serde::to_vec_named(&view.entries[1].input).unwrap(),
            input
        );
        assert_eq!(view.entries[1].quantity.identity, quantity);
        assert_eq!(view.entries[1].volume, volume);
        assert_eq!(view.entries[1].rows, rows);
        assert_eq!(view.displacements[1].as_ref().unwrap().0, old_displacement);
        assert!(!view.comparable[1]);
        app.switch_study(Location::History(0, view.entries[1].retained_run.unwrap()))
            .unwrap();
        assert_eq!(app.core.model.elements[2].section, Some(SectionId(2)));
        assert_eq!(
            rmp_serde::to_vec_named(&app.studies.items[0].runs[old_run]).unwrap(),
            original
        );
        let mut refreshed = ComparisonView::new(&app, 0).unwrap();
        refreshed.case = view.case.clone();
        refreshed.node = 3;
        refreshed.component = 0;
        refreshed.bind_case();
        refreshed.refresh_displacements();
        assert_eq!(refreshed.entries[1].retained_run, Some(new_run));
        assert_eq!(
            refreshed.entries[1].input.model.elements[2].section,
            Some(SectionId(1))
        );
        assert_eq!(
            refreshed.entries[1].app.core.model.elements[2].section,
            Some(SectionId(1))
        );
        assert_ne!(refreshed.entries[1].quantity.identity, quantity);
        assert_ne!(refreshed.entries[1].volume, volume);
        assert_ne!(refreshed.entries[1].rows, rows);
        assert_eq!(
            refreshed.displacements[1].as_ref().unwrap().0,
            new_displacement
        );
        assert_eq!(
            rmp_serde::to_vec_named(&app.studies.items[0].runs[old_run]).unwrap(),
            original
        );
    }
    #[test]
    fn same_missing_fixed_or_outside_members_never_become_matching_complete_scopes_after_reload() {
        for missing in [ElemId(2), ElemId(0)] {
            let mut app = computed();
            let expected: Vec<_> = app.studies.items[0]
                .runs
                .iter()
                .map(|r| {
                    r.comparison_checks
                        .iter()
                        .map(|e| rmp_serde::to_vec_named(&e.expected_checks).unwrap())
                        .collect::<Vec<_>>()
                })
                .collect();
            for run in &mut app.studies.items[0].runs {
                for evidence in &mut run.comparison_checks {
                    evidence.bundle.member_checks.retain(|m| m.elem != missing);
                }
            }
            let raw = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
            let saved: SavedStudies = rmp_serde::from_slice(&raw).unwrap();
            let mut loaded = App::default();
            loaded.core.scoped.references.project = app.core.scoped.references.project.clone();
            loaded.restore_studies(saved).unwrap();
            let view = ComparisonView::new(&loaded, 0).unwrap();
            assert!(view.entries.iter().all(|e| !e.failures.is_empty()));
            assert!(view.comparable.iter().all(|c| !*c));
            assert!(view
                .entries
                .iter()
                .all(|e| e.app.current_design_results().is_none()));
            for (run, expected) in loaded.studies.items[0].runs.iter().zip(&expected) {
                for (e, expected) in run.comparison_checks.iter().zip(expected) {
                    assert_eq!(
                        &rmp_serde::to_vec_named(&e.expected_checks).unwrap(),
                        expected
                    );
                }
            }
            assert_eq!(
                raw,
                rmp_serde::to_vec_named(&loaded.saved_studies()).unwrap()
            );
        }
    }

    #[test]
    fn partially_missing_check_items_cannot_reappear_as_low_ratios_in_comparison_or_common_evidence(
    ) {
        let mut app = computed();
        let evidence = &app.studies.items[0].runs[0].comparison_checks[0];
        let reference = check_rows(evidence)
            .unwrap()
            .into_iter()
            .find_map(|r| r.evidence)
            .unwrap();
        let evidence = &mut app.studies.items[0].runs[0].comparison_checks[0];
        let expected = rmp_serde::to_vec_named(&evidence.expected_checks).unwrap();
        let check = evidence
            .bundle
            .member_checks
            .iter_mut()
            .flat_map(|m| &mut m.positions)
            .find_map(|p| match &mut p.outcome {
                sepika_design_jp::CheckOutcome::Checked(c) if c.components.len() > 1 => Some(c),
                _ => None,
            })
            .unwrap();
        check.components.pop();
        let raw = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let view = ComparisonView::new(&app, 0).unwrap();
        assert!(view.entries[0].failures.iter().any(|s| s.contains("項目")));
        assert!(view.entries[0].app.current_design_results().is_none());
        assert!(app.open_result_evidence(&reference).is_err());
        assert_eq!(
            expected,
            rmp_serde::to_vec_named(
                &app.studies.items[0].runs[0].comparison_checks[0].expected_checks
            )
            .unwrap()
        );
        assert_eq!(raw, rmp_serde::to_vec_named(&app.saved_studies()).unwrap());
    }
    #[test]
    fn normal_retained_result_and_reference_also_reject_partial_items_without_changing_saved_scope()
    {
        let mut app = computed();
        let run = &app.studies.items[0].runs[0];
        let mut source = result_app(
            &run.prepared,
            run.results.as_ref().unwrap(),
            run.last_static,
        );
        source.select_displayed_result(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
        source.require_design_input().unwrap();
        let prepared = StudyInput::capture(&source.core);
        let bundle = source.core.scoped.results.as_ref().unwrap().clone();
        let reference = source
            .recorded_target_result_reference(
                ResultInputKey::Design,
                ReferenceTarget::Member(ElemId(2)),
                references::DisplayConditions {
                    position: Some(0.5),
                    ..Default::default()
                },
            )
            .unwrap();
        let run = &mut app.studies.items[0].runs[0];
        run.prepared = prepared;
        run.expected_result_checks = Some(ExpectedChecks::capture(&bundle));
        run.results = Some(bundle);
        run.last_static = source.core.scoped.last_static;
        let expected = rmp_serde::to_vec_named(&run.expected_result_checks).unwrap();
        assert!(run.original_scope_error().is_none());
        let normal = result_app(
            &run.prepared,
            run.results.as_ref().unwrap(),
            run.last_static,
        );
        normal.require_design_input().unwrap();
        let check = run
            .results
            .as_mut()
            .unwrap()
            .member_checks
            .iter_mut()
            .flat_map(|m| &mut m.positions)
            .find_map(|p| match &mut p.outcome {
                sepika_design_jp::CheckOutcome::Checked(c) if c.components.len() > 1 => Some(c),
                _ => None,
            })
            .unwrap();
        check.components.pop();
        let raw = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        assert!(app.open_result_evidence(&reference).is_err());
        app.switch_study(Location::History(0, 0)).unwrap();
        assert!(app.current_design_results().is_none());
        assert_eq!(
            expected,
            rmp_serde::to_vec_named(&app.studies.items[0].runs[0].expected_result_checks).unwrap()
        );
        assert_eq!(raw, rmp_serde::to_vec_named(&app.saved_studies()).unwrap());
    }
    #[test]
    fn count_labels_distinguish_unavailable_partial_normal_zero_ng_and_no_targets() {
        fn collect(shape: &egui::Shape, result: &mut String) {
            match shape {
                egui::Shape::Text(t) => {
                    result.push_str(t.galley.text());
                    result.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, result);
                    }
                }
                _ => {}
            }
        }
        for state in [
            "all_missing",
            "partial_missing",
            "normal",
            "no_targets",
            "uncalculated",
            "old",
            "no_evidence",
            "failed_empty",
        ] {
            let mut app = computed();
            for run in &mut app.studies.items[0].runs {
                match state {
                    "all_missing" => {
                        for evidence in &mut run.comparison_checks {
                            evidence.expected_checks = None;
                        }
                    }
                    "partial_missing" => run.comparison_checks[0].expected_checks = None,
                    "old" => {
                        for evidence in &mut run.comparison_checks {
                            evidence.bundle.design_contract_version = Some(0);
                        }
                    }
                    "no_evidence" => run.comparison_checks.clear(),
                    "failed_empty" => {
                        run.comparison_checks.clear();
                        run.error = Some("計算失敗".into());
                    }
                    _ => {}
                }
            }
            if state == "uncalculated" {
                app.studies.items[0].runs.clear();
            }
            let mut view = ComparisonView::new(&app, 0).unwrap();
            if state == "no_targets" {
                view.fixed.clear();
            }
            let before = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
            let ctx = egui::Context::default();
            let mut rendered = String::new();
            for _ in 0..2 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(2600.0, 6000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| view.show(ui, &mut app),
                );
                for shape in output.shapes {
                    collect(&shape.shape, &mut rendered);
                }
            }
            match state {
                "normal" => {
                    assert!(
                        rendered.contains("固定対象 算定済み範囲 / 検定済"),
                        "{state}: {rendered}"
                    );
                    assert!(rendered.contains("NG 0 行"));
                }
                "partial_missing" => {
                    for scope in ["固定対象", "主架構"] {
                        assert!(
                            rendered.contains(&format!(
                                "{scope} 部分集計（取得済み範囲のみ） / 検定済"
                            )),
                            "{state}: {rendered}"
                        );
                    }
                }
                "no_targets" => assert!(
                    rendered.contains("固定対象: 対象なし"),
                    "{state}: {rendered}"
                ),
                _ => {
                    for scope in ["固定対象", "主架構"] {
                        assert!(
                            rendered.contains(&format!("{scope}: 集計不可・範囲未確認")),
                            "{state}: {rendered}"
                        );
                    }
                    assert!(!rendered
                        .contains("固定対象 算定済み範囲 / 検定済 0 行・NG 0 行・未検定 0 行"));
                }
            }
            assert_eq!(
                before,
                rmp_serde::to_vec_named(&app.saved_studies()).unwrap()
            );
        }
    }
}
