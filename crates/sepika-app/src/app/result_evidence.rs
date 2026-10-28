use super::*;
use references::{
    DisplayConditions, InputReference, RecordedCalculationInput, ReferenceTarget, ResultReference,
};

const QUANTITY_EVIDENCE_CONTRACT_VERSION: u32 = 1;

/// 保存結果または固定した数量入力を指す。数量の対象範囲はモデル全体。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EvidenceReference {
    Calculation(Box<ResultReference>),
    Quantity(Box<QuantityEvidenceReference>),
}

/// 集計時に固定した入力と対象範囲。現在モデルで欠落を補完しない。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QuantityEvidenceReference {
    source: InputReference,
    model: sepika_core::model::Model,
    contract: u32,
    scope: QuantityEvidenceScope,
    identity: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum QuantityEvidenceScope {
    WholeModel,
}

impl From<&ResultReference> for EvidenceReference {
    fn from(reference: &ResultReference) -> Self {
        Self::Calculation(Box::new(reference.clone()))
    }
}

impl From<&EvidenceReference> for EvidenceReference {
    fn from(reference: &EvidenceReference) -> Self {
        reference.clone()
    }
}

impl QuantityEvidenceReference {
    fn identity(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(
                bincode::serialize(&(&self.source, &self.model, self.contract, &self.scope))
                    .expect("数量根拠の直列化")
            )
        )
    }
}

pub(crate) struct ResultEvidence {
    title: String,
    source: Option<InputReference>,
    reference: Option<EvidenceReference>,
    input: Result<RecordedCalculationInput, String>,
    lines: Vec<String>,
    return_to: Option<workspace::EvidenceReturn>,
}

impl App {
    pub(crate) fn static_evidence_key(&self, key: StaticKey) -> Option<ResultInputKey> {
        match key {
            StaticKey::Case(key) => Some(ResultInputKey::Static(key)),
            StaticKey::Combo(index) => self
                .core
                .scoped
                .results
                .as_ref()?
                .combos
                .get(index)
                .map(|(name, _)| ResultInputKey::Combo(name.clone())),
        }
    }

    pub(crate) fn result_origin_ui(&mut self, ui: &mut egui::Ui, kind: ResultInputKey) {
        let recorded = self.recorded_calculation_input(&kind);
        let source = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|bundle| {
                bundle
                    .reference_sources
                    .iter()
                    .find(|(key, _)| *key == kind)
            })
            .map(|(_, source)| source.clone());
        if let Some(source) = source {
            ui.label(format!(
                "計算元の本案 / 案 {} / 入力世代 {}",
                source.proposal, source.generation
            ));
            ui.small(format!("入力識別: {}", source.input));
            ui.label("計算時の案名称は記録されていません。案の識別で区別します。");
        } else {
            ui.label("計算元の案・入力版は取得不能です。");
        }
        let state = if kind == ResultInputKey::Design {
            self.require_design_input()
        } else {
            self.require_result_input(kind.clone())
        };
        match state {
            Ok(()) => {
                ui.label("状態: 現行の生成入力と一致（個々の合否・未検定は結果欄を確認）");
            }
            Err(reason) => {
                ui.colored_label(
                    crate::theme::WARN_TEXT,
                    format!("状態: 要再計算・現行入力の最新結果として利用不可 / {reason}"),
                );
            }
        }
        match recorded {
            Ok((source, input)) => {
                if ui.button("計算時の入力を見る（読み取り専用）").clicked() {
                    self.ui.scoped.reference_snapshot = Some((
                        format!(
                            "計算時入力 / 案 {} / 世代 {}",
                            source.proposal, source.generation
                        ),
                        input,
                    ));
                }
            }
            Err(reason) => {
                ui.label(reason);
            }
        }
    }

    /// 共通参照が指す保存入力を根拠として開く。閲覧で現行モデルを変更しない。
    pub fn open_result_evidence(
        &mut self,
        reference: impl Into<EvidenceReference>,
    ) -> Result<(), String> {
        let reference = match reference.into() {
            EvidenceReference::Calculation(reference) => reference,
            EvidenceReference::Quantity(quantity) => {
                return self.open_quantity_reference(*quantity)
            }
        };
        let input = self.recorded_result_input(&reference)?;
        if reference.kind == ResultInputKey::Design {
            if let (ReferenceTarget::Member(elem), Some(position)) =
                (reference.target.target, reference.display.position)
            {
                self.open_member_check_evidence(elem, position);
                return Ok(());
            }
        }
        let lines = self.static_reference_lines(&reference, &input.model)?;
        self.ui.scoped.result_evidence = Some(ResultEvidence {
            title: "参照した結果の根拠".into(),
            source: Some(reference.source.clone()),
            reference: Some(EvidenceReference::Calculation(reference.clone())),
            input: Ok(input),
            lines,
            return_to: None,
        });
        Ok(())
    }

    fn static_reference_lines(
        &self,
        reference: &ResultReference,
        model: &sepika_core::model::Model,
    ) -> Result<Vec<String>, String> {
        let ReferenceTarget::Node(id) = reference.target.target else {
            return Ok(vec![format!("対象: {:?}", reference.target.target)]);
        };
        let component = match reference.display.direction.as_deref() {
            Some("X") => 0,
            Some("Y") => 1,
            Some("Z") => 2,
            _ => return Err("変位の全体座標方向は未対応・取得不能です。".into()),
        };
        if reference.display.component.as_deref() != Some("節点変位")
            || reference.display.unit.as_deref() != Some("mm")
        {
            return Err("変位の成分・単位は未対応です。".into());
        }
        let index = model
            .nodes
            .iter()
            .position(|node| node.id == id)
            .ok_or("計算時節点は取得不能です。")?;
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        let result = match &reference.kind {
            ResultInputKey::Static(key) => bundle
                .statics
                .iter()
                .find(|(saved, _)| saved == key)
                .map(|(_, result)| result),
            ResultInputKey::Combo(name) => bundle
                .combos
                .iter()
                .find(|(saved, _)| saved == name)
                .map(|(_, result)| result),
            _ => return Err("この参照の静的変位表示は未対応です。".into()),
        }
        .ok_or("参照した静的結果は取得不能です。")?;
        let value = result
            .disp
            .get(index)
            .map(|values| values[component])
            .filter(|value| value.is_finite())
            .ok_or("指定節点の変位は欠落・取得不能です。")?;
        Ok(vec![
            format!(
                "計算時節点 #{} / 座標 {:?} mm / 全体座標 {} / {value:.6} mm",
                id.0,
                model.nodes[index].coord,
                reference.display.direction.as_deref().unwrap()
            ),
            format!("荷重状態: {}", load_label(&reference.kind, Some(model))),
        ])
    }

    pub(crate) fn open_member_check_evidence(&mut self, elem: ElemId, position: f64) {
        let key = ResultInputKey::Design;
        let mut checked = false;
        let mut lines = vec![format!(
            "部材 #{} / 材軸位置 ξ={position:.3}（始端0・終端1） / 検定比 [無次元]",
            elem.0
        )];
        if let Some(bundle) = &self.core.scoped.results {
            if let Some(check) = bundle
                .member_checks
                .iter()
                .find(|m| m.elem == elem)
                .and_then(|m| m.positions.iter().find(|p| p.xi == position))
            {
                match &check.outcome {
                    sepika_design_jp::CheckOutcome::Checked(check) => {
                        checked = true;
                        lines.push(format!(
                            "検定比 {:.4} / {} / 式名・根拠: {}",
                            check.ratio(),
                            if check.ok() { "OK" } else { "NG" },
                            check.basis
                        ));
                        lines.push(if check.detail.is_empty() {
                            "既存詳細は記録がなく取得不能です。".into()
                        } else {
                            check.detail.clone()
                        });
                        for component in &check.components {
                            lines.push(format!(
                                "{} / 比 {:.4} / {}",
                                component.kind.label(),
                                component.ratio,
                                component.detail
                            ));
                        }
                    }
                    sepika_design_jp::CheckOutcome::Skipped { reason } => {
                        lines.push(format!("未検定・検定不能: {reason}"))
                    }
                }
            } else {
                lines.push("検定詳細は取得不能です。".into());
            }
        }
        if let Ok((_, input)) = self.recorded_calculation_input(&key) {
            if let references::RecordedSettings::Design { source, .. } = &input.settings {
                match source {
                    Some((source_key, source_input)) => {
                        lines.push(format!(
                            "比較ケース集合: 1件 / {}",
                            load_label(source_key, source_input.as_deref().map(|s| &s.model))
                        ));
                        if checked {
                            lines.push(format!("支配ケース: {}（この1件の応力による検定。複数ケースの包絡比較は行っていません）", load_label(source_key, source_input.as_deref().map(|s| &s.model))));
                        } else {
                            lines.push(format!(
                                "採用応力源: {} / 支配ケース: 未検定のため未確定",
                                load_label(source_key, source_input.as_deref().map(|s| &s.model))
                            ));
                        }
                    }
                    None => lines
                        .push("主架構の応力源・比較ケース集合・支配ケースは取得不能です。".into()),
                }
            }
            append_member_input(&mut lines, &input.model, elem);
        }
        self.open_evidence(
            key,
            ReferenceTarget::Member(elem),
            DisplayConditions {
                position: Some(position),
                unit: Some("無次元".into()),
                ..Default::default()
            },
            "部材検定の根拠",
            lines,
        );
    }

    fn open_evidence(
        &mut self,
        key: ResultInputKey,
        target: ReferenceTarget,
        display: DisplayConditions,
        title: &str,
        mut lines: Vec<String>,
    ) {
        let reference = match self.recorded_target_result_reference(key.clone(), target, display) {
            Ok(reference) => Some(EvidenceReference::Calculation(Box::new(reference))),
            Err(reason) => {
                lines.push(reason);
                None
            }
        };
        let recorded = self.recorded_calculation_input(&key);
        let source = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|bundle| {
                bundle
                    .reference_sources
                    .iter()
                    .find(|(saved, _)| *saved == key)
            })
            .map(|(_, source)| source.clone());
        let input = recorded.map(|(_, input)| input);
        self.ui.scoped.result_evidence = Some(ResultEvidence {
            title: title.into(),
            source,
            reference,
            input,
            lines,
            return_to: None,
        });
    }

    pub(crate) fn static_evidence_ui(&mut self, ui: &mut egui::Ui, key: StaticKey) {
        let Some(kind) = self.static_evidence_key(key) else {
            ui.label("静的結果: 未計算・取得不能");
            return;
        };
        egui::CollapsingHeader::new("表示結果の計算元・変位の根拠").id_salt("static_result_evidence").show(ui, |ui| {
            self.result_origin_ui(ui, kind.clone());
            let recorded = self.calculation_snapshot(&kind);
            let model = recorded.as_ref().ok().map(|(_, model)| model);
            let same_input = recorded.as_ref().is_ok_and(|(source, _)| *source == self.input_reference());
            ui.label(format!("荷重状態: {}", load_label(&kind, model)));
            ui.label("変位は節点の全体座標 X・Y・Z [mm]。部材の局所軸方向ではありません。");
            let source_identity = recorded.as_ref().ok().map(|(source, _)| (&source.proposal, source.generation, &source.input));
            let old_member_id = ui.id().with(("recorded_member", format!("{kind:?}/{source_identity:?}")));
            let old_node_id = ui.id().with(("recorded_node", format!("{kind:?}/{source_identity:?}")));
            let mut member = if same_input { self.ui.scoped.selection.active_member() } else { ui.data(|data| data.get_temp::<ElemId>(old_member_id)) };
            let mut selected = if same_input { self.ui.scoped.selection.active_node() } else { ui.data(|data| data.get_temp::<NodeId>(old_node_id)) };
            if !same_input {
                ui.label("現在選択は入力版が異なるため使いません。計算時入力の対象を明示して選択してください。");
                if let Some(model) = model {
                    egui::ComboBox::from_id_salt(old_node_id).selected_text(selected.map_or_else(|| "計算時節点を選択".into(), |id| format!("計算時節点 #{}", id.0))).show_ui(ui, |ui| {
                        for node in &model.nodes { ui.selectable_value(&mut selected, Some(node.id), format!("#{} / {:?} mm", node.id.0, node.coord)); }
                    });
                    egui::ComboBox::from_id_salt(old_member_id).selected_text(member.map_or_else(|| "計算時部材を選択".into(), |id| format!("計算時部材 #{}", id.0))).show_ui(ui, |ui| {
                        for element in &model.elements { ui.selectable_value(&mut member, Some(element.id), format!("#{} / 節点 {:?}", element.id.0, element.nodes)); }
                    });
                    ui.data_mut(|data| {
                        if let Some(id) = selected { data.insert_temp(old_node_id, id); }
                        if let Some(id) = member { data.insert_temp(old_member_id, id); }
                    });
                }
            }
            let design_matches = self.core.scoped.results.as_ref().and_then(|bundle| bundle.reference_sources.iter().find(|(key, _)| *key == ResultInputKey::Design)).is_some_and(|(_, source)| recorded.as_ref().is_ok_and(|(static_source, _)| source == static_source));
            if !design_matches { ui.label("検定の入力版はこの静的結果と異なるか取得不能です。検定表の履歴から計算時の部材・位置を選択してください。"); }
            if let Some(elem) = member.filter(|_| design_matches) {
                let positions: Vec<_> = self.core.scoped.results.as_ref().and_then(|bundle| bundle.member_checks.iter().find(|m| m.elem == elem)).map(|member| member.positions.iter().map(|p| p.xi).collect()).unwrap_or_default();
                if positions.is_empty() { ui.label("選択番号の部材に主架構検定結果はありません（未検定・欠落）。"); }
                for position in positions {
                    if ui.button(format!("計算時部材 #{} / ξ={position:.3} の検定根拠", elem.0)).clicked() { self.open_member_check_evidence(elem, position); }
                }
            }
            let Some(id) = selected else { ui.label("図で節点を選択すると指定変位の値と根拠を確認できます。"); return; };
            let Some(model) = model else { ui.label("計算時節点が取得不能のため変位と現行節点を対応付けません。"); return; };
            let Some(index) = model.nodes.iter().position(|node| node.id == id) else { ui.label("選択番号の計算時節点は取得不能です。"); return; };
            ui.label(format!("計算時節点 #{} / 座標 {:?} mm", id.0, model.nodes[index].coord));
            let values = self.core.scoped.results.as_ref().and_then(|bundle| match key {
                StaticKey::Case(key) => bundle.statics.iter().find(|(k, _)| *k == key).map(|(_, r)| r),
                StaticKey::Combo(index) => bundle.combos.get(index).map(|(_, r)| r),
            }).and_then(|result| result.disp.get(index)).copied();
            if let Some(values) = values {
                for (component, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                    if !value.is_finite() { ui.label(format!("{component}: 取得不能")); continue; }
                    if ui.button(format!("{component}: {value:.6} mm / 根拠")).clicked() {
                        match self.recorded_target_result_reference(kind.clone(), ReferenceTarget::Node(id), DisplayConditions { direction: Some(component.into()), component: Some("節点変位".into()), unit: Some("mm".into()), ..Default::default() }) {
                            Ok(reference) => { if let Err(reason) = self.open_result_evidence(&reference) { self.report_notice(reason); } }
                            Err(reason) => self.report_notice(reason),
                        }
                    }
                }
            } else { ui.label("指定節点の変位は欠落・取得不能です。"); }
        });
    }

    /// 現在の本案とモデル全体の集計対象を同時に固定する。
    pub fn quantity_evidence_reference(&self) -> EvidenceReference {
        let mut quantity = QuantityEvidenceReference {
            source: self.input_reference(),
            model: self.core.model.clone(),
            contract: QUANTITY_EVIDENCE_CONTRACT_VERSION,
            scope: QuantityEvidenceScope::WholeModel,
            identity: String::new(),
        };
        quantity.identity = quantity.identity();
        EvidenceReference::Quantity(Box::new(quantity))
    }

    fn open_quantity_reference(
        &mut self,
        quantity: QuantityEvidenceReference,
    ) -> Result<(), String> {
        if quantity.source.project != self.core.scoped.references.project {
            return Err("別プロジェクトの数量根拠です。".into());
        }
        if quantity.identity != quantity.identity() {
            return Err("数量の案・入力版と固定入力の識別が一致しません。".into());
        }
        if quantity.contract != QUANTITY_EVIDENCE_CONTRACT_VERSION {
            return Err("数量集計の計算契約が異なるため取得不能です。".into());
        }
        let takeoff = sepika_design_jp::quantity::try_compute_quantity_takeoff(
            &quantity.model,
            &quantity.model.quantity_cfg,
        )?;
        let totals = takeoff.totals();
        let unit_weight = sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3;
        let mut lines = vec![format!("集計対象: モデル全体 / 明細 {} 件 / コンクリート {:.6} m³ / 鋼材体積 {:.6} m³ / 鉄骨 {:.6} t", takeoff.items.len(), totals.concrete_m3, totals.steel_t / unit_weight, totals.steel_t), "鋼材体積は鉄骨重量を数量用単位重量7.85 t/m³で換算した値です。鉄筋を含みません。".into(), format!("鉄筋の状態: {}", totals.rebar_status().label())];
        for item in &takeoff.items {
            lines.push(format!("要素 {:?} / 床板 {:?} / 二次部材 {:?} / 壁版 {:?} / {} / {} / 断面 {:?} / コンクリート {:.6} m³ / 鋼材体積 {:.6} m³ / 鉄筋 {}", item.elem, item.slab, item.secondary, item.wall_plate, item.story, item.label, item.section, item.concrete_m3, item.steel_weight_t() / unit_weight, item.rebar_status().label()));
            for issue in &item.rebar_issues {
                lines.push(issue.description());
            }
        }
        lines.extend(takeoff.notes);
        self.ui.scoped.result_evidence = Some(ResultEvidence {
            title: "数量の集計元".into(),
            source: Some(quantity.source.clone()),
            reference: Some(EvidenceReference::Quantity(Box::new(quantity.clone()))),
            input: Ok(RecordedCalculationInput {
                model: quantity.model,
                settings: references::RecordedSettings::Quantity,
            }),
            lines,
            return_to: None,
        });
        Ok(())
    }

    fn evidence_state(&self, evidence: &ResultEvidence) -> String {
        match &evidence.reference {
            Some(EvidenceReference::Calculation(reference)) => {
                if let Err(reason) = self.result_input_snapshot(reference) { return format!("状態: 参照した実行は現在保持する結果と異なるか取得不能です。表示値は開いた時点の記録です / {reason}"); }
                let state = if reference.kind == ResultInputKey::Design { self.require_design_input() } else { self.require_result_input(reference.kind.clone()) };
                state.err().map_or_else(|| "状態: 現行の生成入力と一致（合否・未検定は個々の結果を確認）".into(), |reason| format!("状態: 履歴・要再計算 / {reason}"))
            }
            Some(EvidenceReference::Quantity(quantity)) => {
                if quantity.source == self.input_reference() { "状態: 現在の本案の数量入力と一致（解析・検定の成功とは別）".into() }
                else { "状態: 過去の固定数量入力。現在の本案から再集計した最新数量ではありません。".into() }
            }
            None => "状態: 生成元の参照は取得不能です。表示値は保持されていた記録であり、最新結果として利用できません。".into(),
        }
    }

    pub(crate) fn result_evidence_window(&mut self, ctx: &egui::Context) {
        let Some(mut evidence) = self.ui.scoped.result_evidence.take() else {
            return;
        };
        let mut open = true;
        let state = self.evidence_state(&evidence);
        egui::Window::new(&evidence.title).id(egui::Id::new("result_evidence")).default_width(680.0).open(&mut open).show(ctx, |ui| {
            ui.strong("結果生成時の根拠（読み取り専用）");
            if let Some(source) = &evidence.source {
                ui.label(format!("案 {} / 入力世代 {} / 入力識別 {}", source.proposal, source.generation, source.input));
            } else { ui.label("計算元の案・入力版は取得不能です。"); }
            match &evidence.reference {
                Some(EvidenceReference::Calculation(reference)) => { ui.small(format!("実行 {} / 計算契約 {}", reference.run, reference.contract)); }
                Some(EvidenceReference::Quantity(quantity)) => { ui.small(format!("数量集計契約 {} / 対象: モデル全体 / 識別 {}", quantity.contract, quantity.identity)); }
                None => {}
            }
            ui.label(&state);
            egui::ScrollArea::vertical().id_salt("evidence_lines").max_height(330.0).show(ui, |ui| {
                for line in &evidence.lines { ui.label(line); }
            });
            match &evidence.input {
                Ok(input) => {
                    if ui.button("計算時の入力を見る（読み取り専用）").clicked() {
                        let title = evidence.source.as_ref().map_or_else(|| evidence.title.clone(), |source| format!("{} / 案 {} / 入力世代 {}", evidence.title, source.proposal, source.generation));
                        self.ui.scoped.reference_snapshot = Some((title, input.clone()));
                    }
                }
                Err(reason) => { ui.label(reason); ui.label("現在の入力で補完しません。"); }
            }
            let current = self.input_reference();
            ui.separator();
            ui.label(format!("現在編集する本案: 案 {} / 入力世代 {}", current.proposal, current.generation));
            ui.label(if evidence.source.as_ref() == Some(&current) { "根拠と現在編集の入力版は一致します。" } else { "根拠と現在編集の案・入力版は異なるか取得不能です。対象の対応は推定しません。" });
            ui.horizontal(|ui| {
                for (label, tab) in [("現在の断面入力を編集", ModelTab::Sections), ("現在の材料入力を編集", ModelTab::Materials)] {
                    if ui.button(label).clicked() {
                        let return_to = self.enter_evidence_editor(tab);
                        if evidence.return_to.is_none() { evidence.return_to = Some(return_to); }
                    }
                }
                if evidence.return_to.is_some() && ui.button("元の結果へ戻る").clicked() {
                        self.return_from_evidence_editor(evidence.return_to.take().unwrap());
                }
            });
            ui.label("閲覧後はこの窓を閉じると元の図表に戻ります。AIの説明は補足であり、検定や計算値の保証を置き換えません。");
        });
        if open {
            self.ui.scoped.result_evidence = Some(evidence);
        } else if let Some(return_to) = evidence.return_to {
            self.return_from_evidence_editor(return_to);
        }
    }
}

fn append_member_input(lines: &mut Vec<String>, model: &sepika_core::model::Model, elem: ElemId) {
    if let Some(member) = model.element(elem) {
        lines.push(format!("計算時部材 #{} / 節点 {:?}", elem.0, member.nodes));
        if let Some(section) = member.section.and_then(|id| model.section(id)) {
            lines.push(format!(
                "計算時断面 #{} {} / A={} mm² / Iy={} mm⁴ / Iz={} mm⁴",
                section.id.0, section.name, section.area, section.iy, section.iz
            ));
        } else {
            lines.push("計算時断面は取得不能です。".into());
        }
        lines.extend(references::recorded_member_material_lines(model, member));
    } else {
        lines.push("計算時部材は取得不能です。".into());
    }
}

fn load_label(kind: &ResultInputKey, model: Option<&sepika_core::model::Model>) -> String {
    match kind {
        ResultInputKey::Static(StaticCaseKey::User(id)) => model
            .and_then(|model| model.load_cases.iter().find(|c| c.id == *id))
            .map(|case| format!("ケース #{} {} / {:?}", id.0, case.name, case.kind))
            .unwrap_or_else(|| format!("ケース #{}（名称・条件は取得不能）", id.0)),
        ResultInputKey::Static(StaticCaseKey::Seismic(dir)) => format!("地震静的 {dir:?}"),
        ResultInputKey::Combo(name) => model
            .and_then(|model| model.combinations.iter().find(|c| c.name == *name))
            .map(|combo| format!("組合せ {} / 係数 {:?}", name, combo.terms))
            .unwrap_or_else(|| format!("組合せ {name}（係数は取得不能）")),
        _ => "荷重状態は取得不能です。".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calculated() -> App {
        let mut app = App::default();
        assert!(app.load_model(crate::sample::portal_frame()));
        app.run_linear_static(LoadCaseId(0));
        app.run_design_check();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(!app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .member_checks
            .is_empty());
        app
    }

    #[test]
    fn issue978_古い検定の根拠は計算時断面と条件を保持し閲覧では編集しない() {
        let mut app = calculated();
        let before = app.input_reference();
        let revision = app.core.scoped.undo.revision();
        let check = app.core.scoped.results.as_ref().unwrap().member_checks[0].clone();
        let section = app.core.model.element(check.elem).unwrap().section.unwrap();
        let original = app.core.model.section(section).unwrap().name.clone();
        app.open_member_check_evidence(check.elem, check.positions[0].xi);
        let evidence = app.ui.scoped.result_evidence.as_ref().unwrap();
        assert!(evidence
            .lines
            .iter()
            .any(|line| line.contains("比較ケース集合: 1件")));
        assert!(evidence
            .lines
            .iter()
            .any(|line| line.contains("支配ケース:")));
        assert!(evidence.lines.iter().any(|line| line.contains(&original)));
        assert_eq!(app.input_reference(), before);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        app.core
            .model
            .sections
            .iter_mut()
            .find(|s| s.id == section)
            .unwrap()
            .area *= 2.0;
        app.open_member_check_evidence(check.elem, check.positions[0].xi);
        let evidence = app.ui.scoped.result_evidence.as_ref().unwrap();
        assert!(app.evidence_state(evidence).contains("履歴・要再計算"));
        assert_ne!(
            evidence
                .input
                .as_ref()
                .unwrap()
                .model
                .section(section)
                .unwrap()
                .area,
            app.core.model.section(section).unwrap().area
        );
    }

    #[test]
    fn issue978_欠落入力と新しいrunを現行入力で補わない() {
        let mut app = calculated();
        let check = app.core.scoped.results.as_ref().unwrap().member_checks[0].clone();
        let reference = app
            .recorded_target_result_reference(
                ResultInputKey::Design,
                ReferenceTarget::Member(check.elem),
                DisplayConditions::default(),
            )
            .unwrap();
        app.run_design_check();
        assert!(app.open_result_evidence(&reference).is_err());
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .input_records
            .retain(|(key, _)| *key != ResultInputKey::Design);
        app.open_member_check_evidence(check.elem, check.positions[0].xi);
        let evidence = app.ui.scoped.result_evidence.as_ref().unwrap();
        assert!(evidence.input.as_ref().unwrap_err().contains("取得不能"));
        assert!(evidence.reference.is_none());
        assert!(evidence.lines.iter().any(|line| line.contains("検定比")));
        assert!(!evidence
            .lines
            .iter()
            .any(|line| line.starts_with("計算時断面")));
    }

    #[test]
    fn issue978_数量の根拠を開いた後の編集でも集計元は変わらない() {
        let mut app = calculated();
        let before = app.input_reference();
        let reference = app.quantity_evidence_reference();
        let reference: EvidenceReference =
            rmp_serde::from_slice(&rmp_serde::to_vec_named(&reference).unwrap()).unwrap();
        app.open_result_evidence(&reference).unwrap();
        app.core.model.quantity_cfg.anchorage_dia_factor += 1.0;
        let evidence = app.ui.scoped.result_evidence.as_ref().unwrap();
        assert_eq!(evidence.source.as_ref(), Some(&before));
        assert_ne!(
            evidence
                .input
                .as_ref()
                .unwrap()
                .model
                .quantity_cfg
                .anchorage_dia_factor,
            app.core.model.quantity_cfg.anchorage_dia_factor
        );
        let lines = evidence.lines.clone();
        app.open_result_evidence(&reference).unwrap();
        assert_eq!(app.ui.scoped.result_evidence.as_ref().unwrap().lines, lines);
    }

    #[test]
    fn issue978_根拠窓と静的変位の描画だけではモデルや結果を変更しない() {
        let mut app = calculated();
        let check = app.core.scoped.results.as_ref().unwrap().member_checks[0].clone();
        app.open_member_check_evidence(check.elem, check.positions[0].xi);
        app.select_node(NodeId(2));
        let input = app.input_reference();
        let results = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
        let revision = app.core.scoped.undo.revision();
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 2000.0),
                )),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show_inside(root, |ui| {
                    app.static_evidence_ui(ui, StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))))
                });
                app.result_evidence_window(root.ctx());
            },
        );
        let text = rendered_text(output);
        assert!(text.contains("計算時節点 #2 / 座標"), "{text}");
        assert!(text.contains("mm / 根拠"), "{text}");
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap(),
            results
        );
    }

    fn rendered_text(output: egui::FullOutput) -> String {
        fn collect(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(text.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn issue978_旧入力の変位根拠は現在選択を流用せず計算時対象を明示選択する() {
        let mut app = calculated();
        app.select_node(NodeId(2));
        app.core.model.nodes[2].coord[0] += 1.0;
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 2000.0),
                )),
                ..Default::default()
            },
            |ui| {
                app.static_evidence_ui(ui, StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
            },
        );
        let text = rendered_text(output);
        assert!(
            text.contains("現在選択は入力版が異なるため使いません"),
            "{text}"
        );
        assert!(text.contains("計算時節点を選択"), "{text}");
        assert!(!text.contains("計算時節点 #2 / 座標"), "{text}");
        assert!(!text.contains("mm / 根拠"), "{text}");
    }

    #[test]
    fn issue978_静的共通参照から値座標荷重状態を再構成し開いた後の失効と別runを表示する() {
        let mut app = calculated();
        let reference = app
            .recorded_target_result_reference(
                ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0))),
                ReferenceTarget::Node(NodeId(2)),
                DisplayConditions {
                    direction: Some("Z".into()),
                    component: Some("節点変位".into()),
                    unit: Some("mm".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        app.open_result_evidence(&reference).unwrap();
        let evidence = app.ui.scoped.result_evidence.as_ref().unwrap();
        assert!(evidence
            .lines
            .iter()
            .any(|line| line.contains("全体座標 Z") && line.contains("mm")));
        assert!(evidence
            .lines
            .iter()
            .any(|line| line.starts_with("荷重状態: ケース #0")));
        assert!(app
            .evidence_state(evidence)
            .contains("現行の生成入力と一致"));
        let lines = evidence.lines.clone();
        app.core.model.nodes[2].coord[0] += 1.0;
        assert!(app
            .evidence_state(app.ui.scoped.result_evidence.as_ref().unwrap())
            .contains("履歴・要再計算"));
        assert_eq!(app.ui.scoped.result_evidence.as_ref().unwrap().lines, lines);
        app.run_linear_static(LoadCaseId(0));
        assert!(app
            .evidence_state(app.ui.scoped.result_evidence.as_ref().unwrap())
            .contains("参照した実行"));
    }

    #[test]
    fn issue978_数量参照は入力と案版の不整合と他プロジェクトと契約不一致を拒否する() {
        let mut app = calculated();
        let EvidenceReference::Quantity(original) = app.quantity_evidence_reference() else {
            unreachable!()
        };
        let mut changed = original.clone();
        changed.model.nodes[0].coord[0] += 1.0;
        assert!(app
            .open_result_evidence(EvidenceReference::Quantity(changed))
            .unwrap_err()
            .contains("識別が一致"));
        let mut changed = original.clone();
        changed.source.generation += 1;
        assert!(app
            .open_result_evidence(EvidenceReference::Quantity(changed))
            .is_err());
        let mut changed = original.clone();
        changed.source.project = "other".into();
        assert!(app
            .open_result_evidence(EvidenceReference::Quantity(changed))
            .unwrap_err()
            .contains("別プロジェクト"));
        let mut changed = original;
        changed.contract += 1;
        changed.identity = changed.identity();
        assert!(app
            .open_result_evidence(EvidenceReference::Quantity(changed))
            .unwrap_err()
            .contains("計算契約"));
    }

    #[test]
    fn issue978_数量の再入場後も床板と二次部材の支持配置と採用鉄筋比を読み取り専用で表示する() {
        use sepika_core::ids::{SecondaryMemberId, StoryId};
        use sepika_core::model::{
            MaterialCategory, SecondaryMember, SecondaryMemberEnds, SlabPlate,
        };
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        let mut material = app.core.model.materials[0].clone();
        material.id = MaterialId(1);
        material.category = MaterialCategory::Concrete;
        material.fc = Some(24.0);
        material.fy = None;
        material.name = "固定Fc24".into();
        app.core.model.materials.push(material);
        let mut section = sepika_core::section_shape::SectionShape::RcSlab { thickness: 150.0 }
            .to_section(SectionId(2), "固定床板".into());
        section.material = Some(MaterialId(1));
        app.core.model.sections.push(section);
        for (id, coord) in [(4, [6000.0, 3000.0, 3500.0]), (5, [0.0, 3000.0, 3500.0])] {
            let mut node = app.core.model.nodes[3].clone();
            node.id = NodeId(id);
            node.coord = coord;
            app.core.model.nodes.push(node);
        }
        let slab = app.core.model.add_enclosed_slab_from_nodes(
            &[NodeId(2), NodeId(3), NodeId(4), NodeId(5)],
            SlabPlate {
                section: Some(SectionId(2)),
                ..Default::default()
            },
        );
        app.core.model.slabs[slab.0 as usize].reference_story = Some(StoryId(1));
        app.core.model.slabs[slab.0 as usize].vertical_eccentricity_mm = Some(0.0);
        for element in &mut app.core.model.elements {
            if element.section.is_none() {
                element.section = Some(SectionId(1));
            }
        }
        app.core.model.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(557),
            name: "固定小梁".into(),
            section: Some(SectionId(1)),
            ends: SecondaryMemberEnds::Detached([[0.0, 1500.0, 3500.0], [6000.0, 1500.0, 3500.0]]),
            ..Default::default()
        });
        app.core.model.quantity_cfg.beam_main_ratio = 0.007;
        app.core.model.quantity_cfg.beam_stirrup_ratio = 0.002;
        app.core.model.quantity_cfg.slab_rebar_ratio = 0.012;
        let reference = app.quantity_evidence_reference();
        app.open_result_evidence(&reference).unwrap();
        let original = app
            .ui
            .scoped
            .result_evidence
            .as_ref()
            .unwrap()
            .lines
            .clone();
        app.core.model.nodes[4].coord[1] = 9999.0;
        app.core.model.quantity_cfg.slab_rebar_ratio = 0.099;
        app.open_result_evidence(&reference).unwrap();
        assert_eq!(
            app.ui.scoped.result_evidence.as_ref().unwrap().lines,
            original
        );
        app.ui.scoped.reference_snapshot = Some((
            "数量の固定入力".into(),
            app.ui
                .scoped
                .result_evidence
                .as_ref()
                .unwrap()
                .input
                .as_ref()
                .unwrap()
                .clone(),
        ));
        let before = app.input_reference();
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let mut text = String::new();
        for _ in 0..2 {
            text = rendered_text(ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1800.0, 1600.0),
                    )),
                    ..Default::default()
                },
                |ui| app.reference_snapshot_window(ui.ctx()),
            ));
        }
        for expected in [
            "小梁主筋比 0.007",
            "小梁スターラップ比 0.002",
            "床・片持ち床の鉄筋比 0.012",
            "床板 #0 / 断面",
            "支持境界 [mm]",
            "支持部材・材軸区間",
            "二次部材 #557 固定小梁",
            "実配置: 上面Z=3500 mm / 下面Z=3350 mm / 実厚=150 mm",
            "二次部材 #557 / 主材料: #0 SN400B",
            "3000.0",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert!(!text.contains("0.099"), "{text}");
        assert!(!text.contains("9999.0"), "{text}");
        assert_eq!(app.input_reference(), before);
    }

    #[test]
    fn issue978_未検定の応力生成元を支配ケースとして確定しない() {
        let mut app = calculated();
        let member = &mut app.core.scoped.results.as_mut().unwrap().member_checks[0];
        let elem = member.elem;
        let position = member.positions[0].xi;
        member.positions[0].outcome = sepika_design_jp::CheckOutcome::Skipped {
            reason: "未対応の条件".into(),
        };
        app.open_member_check_evidence(elem, position);
        let lines = &app.ui.scoped.result_evidence.as_ref().unwrap().lines;
        assert!(lines
            .iter()
            .any(|line| line.contains("比較ケース集合: 1件")));
        assert!(lines
            .iter()
            .any(|line| line.contains("採用応力源:") && line.contains("未検定のため未確定")));
        assert!(!lines.iter().any(|line| line.starts_with("支配ケース:")));
    }

    fn role_material_model() -> sepika_core::model::Model {
        use sepika_core::model::MaterialCategory;
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
        let mut model = crate::sample::portal_frame();
        model.sections[0] = SectionShape::SrcColumnRect {
            b: 400.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![2],
                y: vec![2],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
            steel_height: 400.0,
            steel_width: 200.0,
            steel_web_thick: 10.0,
            steel_flange_thick: 16.0,
        }
        .to_section(SectionId(0), "役割別SRC柱".into());
        for (id, name, category, fc, fy) in [
            (
                1,
                "割当コンクリート",
                MaterialCategory::Concrete,
                Some(30.0),
                None,
            ),
            (2, "割当主筋", MaterialCategory::Rebar, None, Some(390.0)),
            (
                3,
                "割当せん断補強筋",
                MaterialCategory::Rebar,
                None,
                Some(295.0),
            ),
            (
                4,
                "割当内蔵鉄骨",
                MaterialCategory::Steel,
                None,
                Some(325.0),
            ),
        ] {
            let mut material = model.materials[0].clone();
            material.id = MaterialId(id);
            material.name = name.into();
            material.category = category;
            material.fc = fc;
            material.fy = fy;
            model.materials.push(material);
        }
        let section = &mut model.sections[0];
        section.material = Some(MaterialId(1));
        section.rebar_material = Some(MaterialId(2));
        section.shear_rebar_material = Some(MaterialId(3));
        section.steel_material = Some(MaterialId(4));
        model
    }

    #[test]
    fn issue978_固定入力の役割別材料と未割当を根拠と読み取り専用入力で区別する() {
        let model = role_material_model();
        let mut lines = Vec::new();
        append_member_input(&mut lines, &model, ElemId(0));
        for expected in [
            "主材料: #1 割当コンクリート",
            "主筋: #2 割当主筋",
            "せん断補強筋: #3 割当せん断補強筋",
            "内蔵鉄骨・鋼管: #4 割当内蔵鉄骨",
        ] {
            assert!(
                lines.iter().any(|line| line.contains(expected)),
                "{lines:?}"
            );
        }
        let mut app = App::default();
        app.load_model(model.clone());
        app.ui.scoped.reference_snapshot = Some((
            "固定SRC材料".into(),
            RecordedCalculationInput {
                model: model.clone(),
                settings: references::RecordedSettings::Quantity,
            },
        ));
        for material in &mut app.core.model.materials {
            material.name = "現在の変更済み材料".into();
        }
        let before = app.input_reference();
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let mut text = String::new();
        for _ in 0..2 {
            text = rendered_text(ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(2000.0, 1800.0),
                    )),
                    ..Default::default()
                },
                |ui| app.reference_snapshot_window(ui.ctx()),
            ));
        }
        let fixed_input = app.ui.scoped.reference_snapshot.as_ref().unwrap().1.clone();
        text.push_str(&rendered_text(ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2400.0, 12000.0),
                )),
                ..Default::default()
            },
            |ui| App::recorded_input_contents_ui(ui, &fixed_input),
        )));
        for expected in [
            "主材料: #1 割当コンクリート",
            "主筋: #2 割当主筋",
            "せん断補強筋: #3 割当せん断補強筋",
            "内蔵鉄骨・鋼管: #4 割当内蔵鉄骨",
            "Fc=Some(30.0)",
            "Fy=Some(390.0)",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert!(!text.contains("現在の変更済み材料"), "{text}");
        assert_eq!(app.input_reference(), before);
        let mut missing = model;
        missing.sections[0].rebar_material = None;
        missing.sections[0].shear_rebar_material = None;
        missing.sections[0].steel_material = None;
        let mut lines = Vec::new();
        append_member_input(&mut lines, &missing, ElemId(0));
        for role in ["主筋", "せん断補強筋", "内蔵鉄骨・鋼管"] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains(&format!("{role}: 未割当・解決不能"))),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn issue978_固定入力の共通と径別指定を含む取込材料の実解決を根拠に表示する() {
        use sepika_core::model::{
            DiameterStrength, StbCommonStrength, StbMemberStrength, StbRebarStrength,
            StbSectionStrength, StbSteelStrength, StrengthTarget,
        };
        let mut model = role_material_model();
        model.stb_strengths.common = Some(StbCommonStrength {
            project_name: String::new(),
            app_name: String::new(),
            app_version: String::new(),
            strength_concrete: Some("Fc36".into()),
            reinforcement: vec![DiameterStrength {
                diameter: "D25".into(),
                strength: "SD345".into(),
            }],
        });
        model.stb_strengths.members.push(StbMemberStrength {
            target: StrengthTarget::Element(ElemId(0)),
            node: NodeId(0),
            node_order: vec![],
            concrete: None,
        });
        model.stb_strengths.sections.push(StbSectionStrength {
            section: SectionId(0),
            concrete: None,
            native_material: None,
            native_rebar: false,
            rebar: None,
            reinforcement: vec![
                StbRebarStrength {
                    element: "StbSecColumn_RC".into(),
                    part: "main".into(),
                    position: None,
                    native_material: None,
                    diameter: Some("D25".into()),
                    strength: None,
                },
                StbRebarStrength {
                    element: "StbSecColumn_RC".into(),
                    part: "band".into(),
                    position: None,
                    native_material: None,
                    diameter: Some("D10".into()),
                    strength: Some("SD295A".into()),
                },
            ],
            steel: vec![StbSteelStrength {
                native_material: None,
                element: "StbSecColumn_S".into(),
                part: "main".into(),
                position: None,
                strength: "SN490B".into(),
            }],
        });
        model.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Column);
        model.prepare_stb_strength_materials();
        let mut lines = Vec::new();
        append_member_input(&mut lines, &model, ElemId(0));
        for (role, grade) in [
            ("主材料", "Fc36"),
            ("主筋", "SD345"),
            ("せん断補強筋", "SD295A"),
            ("内蔵鉄骨・鋼管", "SN490B"),
        ] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains(&format!("{role}: #")) && line.contains(grade)),
                "{role}/{grade}: {lines:?}"
            );
        }
        assert!(!lines.iter().any(|line| line.contains("割当主筋")));
    }

    #[test]
    fn issue978_数量採用入力の内法長ハンチ断面寸法階開口二次材を固定入力から描画する() {
        use sepika_core::ids::{SecondaryMemberId, StoryId};
        use sepika_core::model::{
            ElementKind, FrameSectionUse, Haunch, MemberDetailAttr, PropertyBasis, SecondaryMember,
            SecondaryMemberEnds, WallAttr, WallOpening,
        };
        let mut model = role_material_model();
        model.nodes[0].story = Some(StoryId(0));
        model.stories[0].name = "固定1F".into();
        model.stories[0].sl_offset_mm = Some(12.0);
        model.elements[0].rigid_zone.face_i = None;
        model.elements[0].rigid_zone.face_j = Some(23.0);
        model.elements[0].vertical_eccentricity = Some(17.0);
        model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.member_detail_attrs.push(MemberDetailAttr {
            elem: ElemId(0),
            haunch_i: Some(Haunch {
                length: 111.0,
                depth_increase: 22.0,
                width_increase: 33.0,
            }),
            haunch_j: None,
            joints: vec![],
        });
        model.sections[0].frame_use = Some(FrameSectionUse::Column);
        model.sections[0].width = 444.0;
        model.sections[0].depth = 666.0;
        model.sections[0].thickness = Some(77.0);
        model.sections[0].property_basis.area = PropertyBasis::Supplied;
        model.elements[2].kind = ElementKind::Shell;
        model.elements[2].nodes = [0, 1, 3, 2].into_iter().map(NodeId).collect();
        model.wall_attrs.push(WallAttr {
            elem: ElemId(2),
            opening_area: 1200.0,
            opening_weight: 0.0,
            slit: Default::default(),
            openings: vec![WallOpening {
                width: 30.0,
                height: 40.0,
                offset: Some([50.0, 60.0]),
            }],
            finish_intensity: 0.0,
        });
        model.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(557),
            name: "固定SRC小梁".into(),
            section: Some(SectionId(0)),
            ends: SecondaryMemberEnds::Detached([[0.0; 3], [2000.0, 0.0, 0.0]]),
            ..Default::default()
        });
        let input = RecordedCalculationInput {
            model,
            settings: references::RecordedSettings::Quantity,
        };
        let before = bincode::serialize(&input.model).unwrap();
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let text = rendered_text(ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2400.0, 12000.0),
                )),
                ..Default::default()
            },
            |ui| App::recorded_input_contents_ui(ui, &input),
        ));
        for expected in [
            "所属階 Some(StoryId(0))",
            "階 #0 固定1F",
            "ΔSL Some(12.0)",
            "フェース距離 i=None mm・j=Some(23.0)",
            "鉛直偏心 Some(17.0)",
            "局所軸参照方向 [0.0, 1.0, 0.0]",
            "length: 111.0",
            "断面用途 Some(Column) / 幅 444 mm / せい 666 mm / 実板厚 Some(77.0)",
            "断面積の採用元 Supplied",
            "種別 Concrete",
            "壁・シェルの開口面積 1200 mm²",
            "width: 30.0",
            "二次部材 #557 固定SRC小梁 / 種別 Beam",
            "二次部材 #557 / 主筋: #2 割当主筋",
            "二次部材 #557 / せん断補強筋: #3 割当せん断補強筋",
            "二次部材 #557 / 内蔵鉄骨・鋼管: #4 割当内蔵鉄骨",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert_eq!(bincode::serialize(&input.model).unwrap(), before);
    }

    #[test]
    fn issue978_断面名変更後も履歴表の断面名は保存入力の名前を表示する() {
        let mut app = calculated();
        let original = app.core.model.sections[0].name.clone();
        app.core.model.sections[0].name = "現在だけ変更した断面名".into();
        assert!(app.require_design_input().is_err());
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let text = rendered_text(ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2400.0, 12000.0),
                )),
                ..Default::default()
            },
            |ui| crate::design_view::design_table(ui, &mut app),
        ));
        assert!(text.contains(&format!("計算時断面 {original}")), "{text}");
        assert!(!text.contains("現在だけ変更した断面名"), "{text}");
    }

    #[test]
    fn issue978_図単独の入力工程でも表を開き元の結果配置を復帰する() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.run_linear_static(sepika_core::ids::LoadCaseId(0));
        app.run_design_check();
        app.ui.view.active_tab = crate::app::Tab::Model;
        app.enter_workspace();
        app.ui.view.workspace.placement.contents = [workspace::Content::Drawing; 2];
        app.ui.view.bottom_dock_open = false;
        app.capture_workspace();
        app.ui.view.active_tab = crate::app::Tab::Design;
        app.enter_workspace();
        let original = bincode::serialize(&app.ui.view.workspace.placement).unwrap();
        app.select_member(ElemId(1));
        let check = app.core.scoped.results.as_ref().unwrap().member_checks[0].clone();
        app.open_member_check_evidence(check.elem, check.positions[0].xi);
        let selection = app.ui.scoped.selection.members().to_vec();
        let return_to = app.enter_evidence_editor(crate::app::ModelTab::Sections);
        assert!(app.table_in_workspace(workspace::Content::Model));
        assert_eq!(app.ui.view.model_tab, crate::app::ModelTab::Sections);
        assert_eq!(app.ui.scoped.selection.members(), selection);
        app.return_from_evidence_editor(return_to);
        assert_eq!(app.ui.view.active_tab, crate::app::Tab::Design);
        assert_eq!(
            bincode::serialize(&app.ui.view.workspace.placement).unwrap(),
            original
        );
    }

    #[test]
    #[ignore = "Windows実機確認用のプロジェクトを明示生成する"]
    fn issue978_gui_fixtures() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/issue978-fixtures");
        std::fs::create_dir_all(&directory).unwrap();
        let mut app = calculated();
        app.save_project_to(directory.join("representative.ovika"));
        assert!(app.core.scoped.last_error.is_none());
        let path = directory.join("missing-input.ovika");
        let mut saved = SavedResults {
            bundle: app.core.scoped.results.clone().unwrap(),
            last_static: app.core.scoped.last_static,
            pushover_view_dir: Some(app.core.scoped.pushover_view_dir),
            view_vibration_case: None,
            view_lumped_vibration_case: None,
            last_run: app.core.scoped.staleness.last_run,
        };
        saved.bundle.input_records.clear();
        let bytes = rmp_serde::to_vec_named(&saved).unwrap();
        let cfg = rmp_serde::to_vec_named(&SavedAnalysisSettings {
            references: Some(app.saved_references()),
            cfg: app.core.analysis_cfg,
            wave_name: None,
            wave_sha256: None,
            lumped_wave_name: None,
            lumped_wave_sha256: None,
        })
        .unwrap();
        sepika_io::ovika::save_ovika(
            &path,
            &app.core.model,
            sepika_io::ovika::OvikaExtras {
                results: Some(&bytes),
                analysis_settings: Some(&cfg),
                preparation: None,
            },
        )
        .unwrap();
        println!("GUI fixtures: {}", directory.display());
    }
}
