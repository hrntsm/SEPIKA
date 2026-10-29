use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use super::result_validity::ResultInputKey;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 保存できる案の識別。接続・実行許可は含まない。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectReferences {
    pub project: String,
    pub proposal: String,
    pub generation: u64,
    pub fixed: Vec<TargetReference>,
    pub fixed_results: Vec<ResultReference>,
    pub saved_input: Option<String>,
    #[serde(default)]
    pub accepted_time_history: Option<String>,
    #[serde(default)]
    pub accepted_lumped_time_history: Option<String>,
}

impl Default for ProjectReferences {
    fn default() -> Self {
        Self {
            project: uuid::Uuid::new_v4().to_string(),
            proposal: uuid::Uuid::new_v4().to_string(),
            generation: 0,
            fixed: Vec::new(),
            fixed_results: Vec::new(),
            saved_input: None,
            accepted_time_history: None,
            accepted_lumped_time_history: None,
        }
    }
}

impl ProjectReferences {
    pub(crate) fn advance(&mut self) {
        self.generation = self.generation.checked_add(1).expect("編集世代の上限");
    }
}

/// 名前を用いず、案と入力版を完全一致で照合する。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputReference {
    pub project: String,
    pub proposal: String,
    pub generation: u64,
    pub input: String,
}

/// 読込セッションにだけ有効な照会先。保存参照・実行許可とは別に照合する。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionReference {
    pub session: String,
    pub input: InputReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceTarget {
    Node(NodeId),
    Member(ElemId),
    Section(SectionId),
    Material(MaterialId),
    Story(sepika_core::ids::StoryId),
    Axis { group: usize, axis: usize },
    LoadCase(LoadCaseId),
    Combination(usize),
    Vibration(VibrationCaseId),
    LumpedVibration(LumpedVibrationCaseId),
}

/// 対象存続は入力版内だけで有効。編集・削除・再配置を越えて推定しない。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetReference {
    pub source: InputReference,
    pub target: ReferenceTarget,
    pub identity: String,
    pub lifetime: String,
}

/// 表示条件は計算入力と独立して保存できる。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayConditions {
    pub diagram: Option<ReferenceDiagram>,
    pub force_components: Option<[bool; 6]>,
    pub frame: Option<ReferenceTarget>,
    pub direction: Option<String>,
    pub component: Option<String>,
    pub unit: Option<String>,
    pub position: Option<f64>,
    pub mode: Option<u32>,
    pub step: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceDiagram {
    Shape,
    Deformed,
    Force,
    Mode,
    Hinge,
    TimeHistory,
    LumpedMode,
    LumpedTimeHistory,
}

/// 実行ID・開始入力・計算契約が一致する結果だけを解決する。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultReference {
    pub source: InputReference,
    pub target: TargetReference,
    pub kind: ResultInputKey,
    pub run: String,
    pub calculation_input: String,
    pub contract: u32,
    pub display: DisplayConditions,
}

#[derive(Clone, Debug)]
pub struct RecordedCalculationInput {
    pub model: sepika_core::model::Model,
    pub settings: RecordedSettings,
}

#[derive(Clone, Debug)]
pub enum RecordedSettings {
    Quantity,
    Analysis {
        kind: ResultInputKey,
        cfg: Box<AnalysisSettings>,
        period_s: Option<f64>,
    },
    Design {
        short_term: bool,
        rc_damage_control: bool,
        bond_method: sepika_design_jp::BondMethod,
        qd_method: sepika_design_jp::QdMethod,
        source: Option<(ResultInputKey, Option<Box<RecordedCalculationInput>>)>,
    },
    Vibration {
        analysis_identity: String,
        identity: String,
        cfg: Option<Box<AnalysisSettings>>,
    },
}

#[cfg(feature = "gui")]
pub(crate) fn recorded_member_material_lines(
    model: &sepika_core::model::Model,
    member: &sepika_core::model::ElementData,
) -> Vec<String> {
    [
        ("主材料", model.element_material(member)),
        ("主筋", model.element_rebar_material(member)),
        ("せん断補強筋", model.element_shear_rebar_material(member)),
        ("内蔵鉄骨・鋼管", model.element_steel_material(member)),
    ].into_iter().map(|(role, material)| {
        material.map_or_else(
            || format!("計算時の実解決材料 / {role}: 未割当・解決不能（形状により対象外の役割を含む）"),
            |material| format!("計算時の実解決材料 / {role}: #{} {} / Fc={:?} N/mm² / Fy={:?} N/mm² / E={} N/mm²", material.id.0, material.name, material.fc, material.fy, material.young),
        )
    }).collect()
}

impl App {
    /// 保存された計算時入力内の対象から参照を作る。現行入力への対応は推定しない。
    pub fn recorded_target_result_reference(
        &self,
        kind: ResultInputKey,
        target: ReferenceTarget,
        display: DisplayConditions,
    ) -> Result<ResultReference, String> {
        let (source, model) = self.calculation_snapshot(&kind)?;
        let exists = match target {
            ReferenceTarget::Node(id) => model.node(id).is_some(),
            ReferenceTarget::Member(id) => model.element(id).is_some(),
            ReferenceTarget::Section(id) => model.section(id).is_some(),
            ReferenceTarget::Material(id) => model.materials.iter().any(|m| m.id == id),
            _ => return Err("この対象の計算時参照は未対応です。".into()),
        };
        if !exists {
            return Err("計算時入力内の対象は取得不能です。".into());
        }
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        let bytes = &bundle
            .input_records
            .iter()
            .find(|(key, _)| *key == kind)
            .ok_or("計算時入力は取得不能です。")?
            .1;
        let run = bundle
            .reference_runs
            .iter()
            .find(|(key, _)| *key == kind)
            .ok_or("実行識別は取得不能です。")?
            .1
            .clone();
        let contract = if kind == ResultInputKey::Design {
            bundle.design_contract_version
        } else {
            bundle.analysis_contract_version
        }
        .ok_or("計算契約は取得不能です。")?;
        let identity = String::new();
        let lifetime =
            digest(&bincode::serialize(&(&source, target, &identity)).expect("対象参照の直列化"));
        Ok(ResultReference {
            target: TargetReference {
                source: source.clone(),
                target,
                identity,
                lifetime,
            },
            source,
            kind,
            run,
            calculation_input: digest(bytes),
            contract,
            display,
        })
    }

    /// ユーザーが確定した解析設定を適用し、変更がある場合だけ編集世代を進める。
    pub fn commit_analysis_settings(&mut self, next: AnalysisSettings) -> bool {
        if bincode::serialize(&self.core.analysis_cfg).expect("解析設定の直列化")
            == bincode::serialize(&next).expect("解析設定の直列化")
        {
            return false;
        }
        if !self.study_is_main() {
            self.report_error("検討の計算条件は作成時に固定されています");
            return false;
        }
        self.core.analysis_cfg = next;
        self.core.scoped.references.advance();
        self.core.scoped.staleness.mark_non_calc_edited();
        true
    }

    pub(crate) fn accept_time_history_reference(
        &mut self,
        source: &TimeHistorySource,
        lumped: bool,
    ) {
        let state = &mut self.core.scoped.references;
        let accepted = if lumped {
            &mut state.accepted_lumped_time_history
        } else {
            &mut state.accepted_time_history
        };
        if accepted.as_ref() != Some(&source.raw_identity) {
            *accepted = Some(source.raw_identity.clone());
            state.advance();
            self.core.scoped.staleness.mark_non_calc_edited();
        }
    }

    pub fn connection_reference(&self) -> ConnectionReference {
        ConnectionReference {
            session: self.core.scoped.reference_session.clone(),
            input: self.input_reference(),
        }
    }

    pub fn validate_connection_reference(
        &self,
        reference: &ConnectionReference,
    ) -> Result<(), String> {
        if reference.session != self.core.scoped.reference_session {
            return Err("接続セッションが異なります。保存参照から実行許可を再開しません。".into());
        }
        if reference.input != self.input_reference() {
            return Err("接続先の案・入力版が異なります。".into());
        }
        Ok(())
    }

    pub(crate) fn result_source_reference(&self) -> InputReference {
        self.core
            .scoped
            .applying_result_source
            .clone()
            .unwrap_or_else(|| self.input_reference())
    }

    fn reference_input_identity(&self) -> String {
        let mut input = self.core.model.clone();
        let member_ids: Vec<_> = input
            .elements
            .iter()
            .filter(|element| element.kind != sepika_core::model::ElementKind::PanelZone)
            .map(|element| element.id)
            .collect();
        input.mass_method = self.core.analysis_cfg.mass_method;
        let source_id_assignment = input.assign_stb_node_ids();
        let panel_mode = input.panel_zone;
        input.panel_zone = sepika_core::model::PanelZoneMode::None;
        sepika_element::springs::panel_gen::apply_auto_panel_zones(&mut input)
            .expect("パネル除去だけでは算定を行わない");
        input.panel_zone = panel_mode;
        input.load_cases.retain(|case| {
            case.origin != sepika_core::model::LoadCaseOrigin::Generated
                || case.nodal.iter().any(|load| !load.source.is_auto())
                || case.member.iter().any(|load| !load.source.is_auto())
        });
        for case in &mut input.load_cases {
            case.kind = case.explicit_input_kind();
            case.origin = sepika_core::model::LoadCaseOrigin::Explicit;
        }
        let model = &input;
        let definitions: Vec<_> = model
            .load_cases
            .iter()
            .map(|case| (case.id, &case.name, case.kind))
            .collect();
        let names: Vec<_> = model
            .sections
            .iter()
            .map(|s| &s.name)
            .chain(model.materials.iter().map(|m| &m.name))
            .chain(model.stories.iter().map(|s| &s.name))
            .chain(model.source_stories.iter().map(|s| &s.name))
            .chain(model.floor_regions.iter().flat_map(|r| {
                std::iter::once(&r.name).chain(r.secondary_beams.iter().map(|member| &member.name))
            }))
            .chain(model.wall_regions.iter().flat_map(|r| {
                std::iter::once(&r.name).chain(r.posts.iter().map(|member| &member.name))
            }))
            .chain(
                model
                    .unassigned_beams
                    .iter()
                    .chain(&model.unassigned_posts)
                    .map(|member| &member.name),
            )
            .collect();
        digest(
            &bincode::serialize(&(
                sepika_job::weight_preparation::weight_input_key(model, model.mass_method),
                &model.axes,
                &model.quantity_cfg,
                &model.combinations,
                &model.stb_node_ids,
                source_id_assignment,
                member_ids,
                self.core.analysis_cfg,
                &self.core.scoped.references.accepted_time_history,
                &self.core.scoped.references.accepted_lumped_time_history,
                definitions,
                names,
            ))
            .expect("入力参照の直列化"),
        )
    }

    pub(crate) fn account_preparation_revision(&mut self, before: &InputReference, revision: u64) {
        if self.reference_input_identity() == before.input {
            let added = self
                .core
                .scoped
                .undo
                .revision()
                .checked_sub(revision)
                .expect("準備計算のUndo revision");
            self.core.scoped.reference_preparation_revisions = self
                .core
                .scoped
                .reference_preparation_revisions
                .checked_add(added)
                .expect("準備計算revisionの上限");
        }
    }

    pub(crate) fn saved_references(&self) -> ProjectReferences {
        let mut saved = self.core.scoped.references.clone();
        let input = self.input_reference();
        saved.generation = input.generation;
        saved.saved_input = Some(input.input);
        saved
    }

    pub(crate) fn restore_references(&mut self, saved: Option<ProjectReferences>) {
        if let Some(saved) = saved {
            self.core.scoped.references = saved;
            let actual = self.input_reference().input;
            if self.core.scoped.references.saved_input.as_ref() != Some(&actual) {
                self.core.scoped.references.advance();
            }
        }
    }

    pub fn input_reference(&self) -> InputReference {
        let state = &self.core.scoped.references;
        InputReference {
            project: state.project.clone(),
            proposal: state.proposal.clone(),
            generation: state
                .generation
                .checked_add(self.core.scoped.undo.revision())
                .and_then(|generation| {
                    generation.checked_sub(self.core.scoped.reference_preparation_revisions)
                })
                .expect("編集世代の上限"),
            input: self.reference_input_identity(),
        }
    }

    pub fn target_reference(&self, target: ReferenceTarget) -> Result<TargetReference, String> {
        self.target_reference_at(target, self.input_reference())
    }

    fn target_reference_at(
        &self,
        target: ReferenceTarget,
        source: InputReference,
    ) -> Result<TargetReference, String> {
        if !self.reference_target_exists(target) {
            return Err("対象が存在しません。".into());
        }
        let identity = match target {
            ReferenceTarget::LoadCase(id) => digest(
                &bincode::serialize(
                    &self
                        .core
                        .model
                        .load_cases
                        .iter()
                        .find(|case| case.id == id)
                        .map(|case| (case.id, &case.name, case.explicit_input_kind())),
                )
                .expect("荷重ケースの直列化"),
            ),
            ReferenceTarget::Vibration(id) => digest(
                &bincode::serialize(
                    &self
                        .core
                        .model
                        .vibration_cases
                        .iter()
                        .find(|case| case.id == id),
                )
                .expect("振動ケースの直列化"),
            ),
            ReferenceTarget::LumpedVibration(id) => digest(
                &bincode::serialize(
                    &self
                        .core
                        .model
                        .lumped_vibration_cases
                        .iter()
                        .find(|case| case.id == id),
                )
                .expect("質点系振動ケースの直列化"),
            ),
            _ => String::new(),
        };
        let lifetime =
            digest(&bincode::serialize(&(&source, target, &identity)).expect("対象参照の直列化"));
        Ok(TargetReference {
            source,
            target,
            identity,
            lifetime,
        })
    }

    fn reference_target_exists(&self, target: ReferenceTarget) -> bool {
        match target {
            ReferenceTarget::Node(id) => self.core.model.nodes.iter().any(|n| n.id == id),
            ReferenceTarget::Member(id) => self.core.model.element(id).is_some(),
            ReferenceTarget::Section(id) => self.core.model.sections.iter().any(|s| s.id == id),
            ReferenceTarget::Material(id) => self.core.model.materials.iter().any(|m| m.id == id),
            ReferenceTarget::Story(id) => self.core.model.stories.iter().any(|s| s.id == id),
            ReferenceTarget::Axis { group, axis } => self
                .core
                .model
                .axes
                .get(group)
                .and_then(|g| g.axes.get(axis))
                .is_some(),
            ReferenceTarget::LoadCase(id) => {
                self.core.model.load_cases.iter().any(|case| case.id == id)
            }
            ReferenceTarget::Combination(index) => {
                self.core.model.combinations.get(index).is_some()
            }
            ReferenceTarget::Vibration(id) => self
                .core
                .model
                .vibration_cases
                .iter()
                .any(|case| case.id == id),
            ReferenceTarget::LumpedVibration(id) => self
                .core
                .model
                .lumped_vibration_cases
                .iter()
                .any(|case| case.id == id),
        }
    }

    pub fn resolve_target_reference(
        &self,
        reference: &TargetReference,
    ) -> Result<ReferenceTarget, String> {
        self.resolve_target_reference_at(reference, &self.input_reference())
    }

    fn resolve_target_reference_at(
        &self,
        reference: &TargetReference,
        current: &InputReference,
    ) -> Result<ReferenceTarget, String> {
        if current.project != reference.source.project {
            return Err("別プロジェクトの参照です。".into());
        }
        if current.proposal != reference.source.proposal {
            return Err("別案の参照です。明示された対象対応が必要です。".into());
        }
        if *current != reference.source {
            return Err("入力版が異なります。固定対象を現在の入力へ読み替えません。".into());
        }
        let expected = self.target_reference_at(reference.target, current.clone())?;
        if expected.lifetime != reference.lifetime {
            return Err("対象の存続識別が一致しません。".into());
        }
        Ok(reference.target)
    }

    pub fn result_reference(
        &self,
        target: TargetReference,
        kind: ResultInputKey,
        display: DisplayConditions,
    ) -> Result<ResultReference, String> {
        self.resolve_target_reference(&target)?;
        self.require_result_input(kind.clone())?;
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        let input = bundle
            .input_records
            .iter()
            .find(|(key, _)| *key == kind)
            .ok_or("計算時入力は取得不能です。")?;
        let run = bundle
            .reference_runs
            .iter()
            .find(|(key, _)| *key == kind)
            .ok_or("実行識別がありません。結果を再実行してください。")?;
        let contract = if kind == ResultInputKey::Design {
            bundle.design_contract_version
        } else {
            bundle.analysis_contract_version
        }
        .ok_or("計算契約が不明です。")?;
        let source = bundle
            .reference_sources
            .iter()
            .find(|(key, _)| *key == kind)
            .ok_or("計算時の案識別がありません。")?
            .1
            .clone();
        if target.source != source {
            return Err(
                "対象参照は計算時の入力版と異なります。計算時入力を閲覧してください。".into(),
            );
        }
        Ok(ResultReference {
            source,
            target,
            kind,
            run: run.1.clone(),
            calculation_input: digest(&input.1),
            contract,
            display,
        })
    }

    /// 保持された計算用モデルを返す。完全な編集モデルの欠落を現在入力から補完しない。
    pub fn result_input_snapshot(
        &self,
        reference: &ResultReference,
    ) -> Result<sepika_core::model::Model, String> {
        if let Some(app) = self.study_result_view(reference) {
            return app.result_input_snapshot(reference);
        }
        if reference.target.source != reference.source {
            return Err("結果と対象の入力版が一致しません。".into());
        }
        let lifetime = digest(
            &bincode::serialize(&(
                &reference.target.source,
                reference.target.target,
                &reference.target.identity,
            ))
            .expect("対象参照の直列化"),
        );
        if lifetime != reference.target.lifetime {
            return Err("対象の存続識別が一致しません。".into());
        }
        let state = &self.core.scoped.references;
        if state.project != reference.source.project || state.proposal != reference.source.proposal
        {
            return Err("別プロジェクトまたは別案の結果です。".into());
        }
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        if !bundle
            .reference_runs
            .iter()
            .any(|(key, run)| *key == reference.kind && *run == reference.run)
        {
            return Err("参照した実行結果は取得不能です。".into());
        }
        if !bundle
            .reference_sources
            .iter()
            .any(|(key, source)| *key == reference.kind && *source == reference.source)
        {
            return Err("計算時の案・入力版が一致しません。".into());
        }
        let contract = if reference.kind == ResultInputKey::Design {
            bundle.design_contract_version
        } else {
            bundle.analysis_contract_version
        };
        if contract != Some(reference.contract) {
            return Err("計算契約が一致しません。".into());
        }
        let (_, bytes) = bundle
            .input_records
            .iter()
            .find(|(key, _)| *key == reference.kind)
            .ok_or("計算時入力は取得不能です。")?;
        if digest(bytes) != reference.calculation_input {
            return Err("計算時入力の識別が一致しません。".into());
        }
        bincode::deserialize_from(std::io::Cursor::new(bytes))
            .map_err(|_| "計算時入力は取得不能です。".into())
    }

    pub fn resolve_result_reference(
        &self,
        reference: &ResultReference,
    ) -> Result<ReferenceTarget, String> {
        if let Some(result) = self.resolve_study_result(reference) {
            return result;
        }
        self.result_input_snapshot(reference)?;
        self.require_result_input(reference.kind.clone())?;
        self.resolve_target_reference(&reference.target)
    }

    pub fn calculation_snapshot(
        &self,
        kind: &ResultInputKey,
    ) -> Result<(InputReference, sepika_core::model::Model), String> {
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        let source = bundle
            .reference_sources
            .iter()
            .find(|(key, _)| key == kind)
            .ok_or("計算時の案・入力版は取得不能です。")?
            .1
            .clone();
        if source.project != self.core.scoped.references.project
            || source.proposal != self.core.scoped.references.proposal
        {
            return Err("別プロジェクトまたは別案の結果です。".into());
        }
        let (_, bytes) = bundle
            .input_records
            .iter()
            .find(|(key, _)| key == kind)
            .ok_or("計算時入力は取得不能です。")?;
        let model = bincode::deserialize_from(std::io::Cursor::new(bytes))
            .map_err(|_| "計算時入力は取得不能です。")?;
        Ok((source, model))
    }

    pub fn recorded_calculation_input(
        &self,
        kind: &ResultInputKey,
    ) -> Result<(InputReference, RecordedCalculationInput), String> {
        let (source, _) = self.calculation_snapshot(kind)?;
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        let (_, bytes) = bundle
            .input_records
            .iter()
            .find(|(key, _)| key == kind)
            .ok_or("計算時入力は取得不能です。")?;
        let cfg = bundle
            .reference_vibration_settings
            .iter()
            .find(|(key, _)| key == kind)
            .map(|(_, cfg)| *cfg);
        Ok((
            source,
            super::result_validity::decode_recorded_input(kind, bytes, cfg)?,
        ))
    }

    pub fn recorded_result_input(
        &self,
        reference: &ResultReference,
    ) -> Result<RecordedCalculationInput, String> {
        if let Some(app) = self.study_result_view(reference) {
            return app.recorded_result_input(reference);
        }
        self.result_input_snapshot(reference)?;
        self.recorded_calculation_input(&reference.kind)
            .map(|(_, input)| input)
    }
}

#[cfg(feature = "gui")]
#[derive(Clone, Copy)]
pub enum ReferenceView {
    Diagram,
    Table,
    Detail,
}

#[cfg(feature = "gui")]
impl App {
    pub fn current_display_conditions(&self) -> Result<DisplayConditions, String> {
        use crate::viewer::ViewMode;
        let content = self.active_workspace_content();
        if content != super::workspace::Content::Drawing
            && !(content == super::workspace::Content::Results
                && self.ui.view.results_view == ResultsView::Spatial)
        {
            return Err(
                "表・グラフからの表示条件固定には未対応です。主領域の図を選択してください。".into(),
            );
        }
        let view = self.active_viewport();
        let diagram = match view.view_mode {
            ViewMode::Shape => ReferenceDiagram::Shape,
            ViewMode::Deformed => ReferenceDiagram::Deformed,
            ViewMode::Force => ReferenceDiagram::Force,
            ViewMode::Mode => ReferenceDiagram::Mode,
            ViewMode::Hinge => ReferenceDiagram::Hinge,
            ViewMode::TimeHistory => ReferenceDiagram::TimeHistory,
            ViewMode::LumpedMode => ReferenceDiagram::LumpedMode,
            ViewMode::LumpedTimeHistory => ReferenceDiagram::LumpedTimeHistory,
            _ => return Err("この表示種別の結果参照固定には未対応です。形状・変形・応力・モード・ヒンジ・時刻歴の図で固定してください。".into()),
        };
        let frame = view.frame_target.map(|target| match target {
            sepika_core::frame::FrameTarget::Story(id) => ReferenceTarget::Story(id),
            sepika_core::frame::FrameTarget::Axis { group, axis } => {
                ReferenceTarget::Axis { group, axis }
            }
        });
        let force_components = (diagram == ReferenceDiagram::Force)
            .then(|| crate::viewer::ForceComponent::ALL.map(|c| view.force_components.is_on(c)));
        let unit = match diagram {
            ReferenceDiagram::Force => "kN・kN·m",
            ReferenceDiagram::Deformed
            | ReferenceDiagram::TimeHistory
            | ReferenceDiagram::LumpedTimeHistory => "mm",
            _ => "無次元／形状",
        };
        Ok(DisplayConditions {
            diagram: Some(diagram),
            force_components,
            frame,
            component: (diagram == ReferenceDiagram::Force).then(|| {
                view.force_components
                    .selected()
                    .map(|c| format!("{c:?}"))
                    .collect::<Vec<_>>()
                    .join("/")
            }),
            unit: Some(unit.into()),
            direction: matches!(diagram, ReferenceDiagram::Hinge).then(|| {
                format!(
                    "{:?}",
                    view.pushover_dir
                        .unwrap_or(self.core.scoped.pushover_view_dir)
                )
            }),
            mode: matches!(
                diagram,
                ReferenceDiagram::Mode | ReferenceDiagram::LumpedMode
            )
            .then_some(view.view_mode_idx as u32),
            step: match diagram {
                ReferenceDiagram::TimeHistory | ReferenceDiagram::LumpedTimeHistory => {
                    Some(view.th_frame as u32)
                }
                ReferenceDiagram::Hinge => view.hinge_step.map(|step| step as u32),
                _ => None,
            },
            position: None,
        })
    }

    pub fn show_result_reference(
        &mut self,
        reference: &ResultReference,
        view: ReferenceView,
        reveal: bool,
    ) -> Result<(), String> {
        if let Some(location) = self.study_result_location(reference) {
            if self.studies.active != location {
                self.switch_study(location)?;
            }
        }
        self.activate_study_result_reference(reference);
        self.resolve_result_reference(reference)?;
        self.validate_reference_display(reference)?;
        if let Some(frame) = reference.display.frame {
            if !matches!(
                frame,
                ReferenceTarget::Story(_) | ReferenceTarget::Axis { .. }
            ) || !self.reference_target_exists(frame)
            {
                return Err("表示条件の階・通りが取得不能です。".into());
            }
        }
        let frame = reference.display.frame.map(|frame| match frame {
            ReferenceTarget::Story(id) => sepika_core::frame::FrameTarget::Story(id),
            ReferenceTarget::Axis { group, axis } => {
                sepika_core::frame::FrameTarget::Axis { group, axis }
            }
            _ => unreachable!(),
        });
        if !reveal && self.reference_hidden_in(reference.target.target, frame) {
            return Err(
                "対象は保存した階・通りで非表示です。「対象を表示」で表示できます。".into(),
            );
        }
        let static_key = match &reference.kind {
            ResultInputKey::Static(key) => Some(StaticKey::Case(*key)),
            ResultInputKey::Combo(name) => Some(StaticKey::Combo(
                self.core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.combos.iter().position(|(n, _)| n == name))
                    .ok_or("組合せ結果は取得不能です。")?,
            )),
            _ => None,
        };
        let playback = match (&reference.kind, reference.display.step) {
            (ResultInputKey::TimeHistory(id), Some(step)) => self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|bundle| bundle.time_history_for(*id))
                .and_then(|result| result.recording.as_ref())
                .and_then(|recording| recording.frame_time.get(step as usize))
                .map(|time| (step as usize, *time)),
            (ResultInputKey::LumpedTimeHistory(id), Some(step)) => self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|bundle| bundle.lumped_result_for(*id))
                .and_then(|result| result.response.as_ref())
                .and_then(|response| response.time.get(step as usize))
                .map(|time| (step as usize, *time)),
            _ => None,
        };
        if playback.is_some_and(|(_, time)| !time.is_finite()) {
            return Err("参照するステップの時刻は取得不能です。".into());
        }
        self.show_reference_at(&reference.target, view, true, Tab::Results)?;
        self.ui.view.frame_target = if reveal {
            None
        } else {
            reference.display.frame.map(|frame| match frame {
                ReferenceTarget::Story(id) => sepika_core::frame::FrameTarget::Story(id),
                ReferenceTarget::Axis { group, axis } => {
                    sepika_core::frame::FrameTarget::Axis { group, axis }
                }
                _ => unreachable!(),
            })
        };
        if let Some(diagram) = reference.display.diagram {
            use crate::viewer::ViewMode;
            self.ui.view.view_mode = match diagram {
                ReferenceDiagram::Shape => ViewMode::Shape,
                ReferenceDiagram::Deformed => ViewMode::Deformed,
                ReferenceDiagram::Force => ViewMode::Force,
                ReferenceDiagram::Mode => ViewMode::Mode,
                ReferenceDiagram::Hinge => ViewMode::Hinge,
                ReferenceDiagram::TimeHistory => ViewMode::TimeHistory,
                ReferenceDiagram::LumpedMode => ViewMode::LumpedMode,
                ReferenceDiagram::LumpedTimeHistory => ViewMode::LumpedTimeHistory,
            };
        }
        if let Some(flags) = reference.display.force_components {
            for (component, enabled) in crate::viewer::ForceComponent::ALL.into_iter().zip(flags) {
                *self.ui.view.force_components.flag_mut(component) = enabled;
            }
        }
        if let Some(mode) = reference.display.mode {
            self.ui.view.view_mode_idx = mode as usize;
        }
        if let Some(step) = reference.display.step {
            if reference.display.diagram == Some(ReferenceDiagram::Hinge) {
                self.ui.view.hinge_step = Some(step as usize);
            } else {
                self.ui.view.th_frame = step as usize;
            }
        }
        if reference.display.diagram == Some(ReferenceDiagram::Hinge) {
            self.ui.view.hinge_step = reference.display.step.map(|step| step as usize);
        }
        match reference.kind {
            ResultInputKey::Pushover(dir) => self.ui.view.viewport.pushover_dir = Some(dir),
            ResultInputKey::TimeHistory(id) => self.bind_spatial_case_to_active_view(id),
            ResultInputKey::LumpedTimeHistory(id) => self.bind_lumped_case_to_active_view(id),
            _ => {}
        }
        if let Some((frame, time)) = playback {
            self.ui.view.th_frame = frame;
            self.ui.view.th_play_time = time;
            self.ui.view.th_playing = false;
        }
        if let Some(key) = static_key {
            self.ui.view.viewport.static_key = Some(key);
            self.ui.view.viewport.static_unavailable = false;
        }
        Ok(())
    }

    fn validate_reference_display(&self, reference: &ResultReference) -> Result<(), String> {
        let Some(diagram) = reference.display.diagram else {
            return Err("表示種別が未指定です。".into());
        };
        let compatible = match diagram {
            ReferenceDiagram::Shape => true,
            ReferenceDiagram::Deformed | ReferenceDiagram::Force => matches!(
                reference.kind,
                ResultInputKey::Static(_) | ResultInputKey::Combo(_)
            ),
            ReferenceDiagram::Mode => reference.kind == ResultInputKey::Modal,
            ReferenceDiagram::Hinge => matches!(reference.kind, ResultInputKey::Pushover(_)),
            ReferenceDiagram::TimeHistory => {
                matches!(reference.kind, ResultInputKey::TimeHistory(_))
            }
            ReferenceDiagram::LumpedMode | ReferenceDiagram::LumpedTimeHistory => {
                matches!(reference.kind, ResultInputKey::LumpedTimeHistory(_))
            }
        };
        if !compatible {
            return Err("結果種別と表示条件が一致しません。".into());
        }
        let expected_unit = match diagram {
            ReferenceDiagram::Force => "kN・kN·m",
            ReferenceDiagram::Deformed
            | ReferenceDiagram::TimeHistory
            | ReferenceDiagram::LumpedTimeHistory => "mm",
            _ => "無次元／形状",
        };
        if reference.display.unit.as_deref() != Some(expected_unit) {
            return Err("この表示単位には未対応です。単位を現在値で代用しません。".into());
        }
        if diagram == ReferenceDiagram::Force {
            let flags = reference
                .display
                .force_components
                .ok_or("応力成分が未指定です。")?;
            let components = crate::viewer::ForceComponent::ALL
                .into_iter()
                .zip(flags)
                .filter(|(_, enabled)| *enabled)
                .map(|(component, _)| format!("{component:?}"))
                .collect::<Vec<_>>()
                .join("/");
            if reference.display.component.as_ref() != Some(&components) {
                return Err("応力成分の指定が一致しません。".into());
            }
        }
        if matches!(
            diagram,
            ReferenceDiagram::Mode | ReferenceDiagram::LumpedMode
        ) && reference.display.mode.is_none()
        {
            return Err("モードが未指定です。".into());
        }
        if matches!(
            diagram,
            ReferenceDiagram::TimeHistory | ReferenceDiagram::LumpedTimeHistory
        ) && reference.display.step.is_none()
        {
            return Err("ステップが未指定です。".into());
        }
        if let ResultInputKey::Pushover(direction) = reference.kind {
            if diagram == ReferenceDiagram::Hinge
                && reference.display.direction.as_deref() != Some(&format!("{direction:?}"))
            {
                return Err("加力方向の指定が一致しません。".into());
            }
        }
        if reference.display.position.is_some() {
            return Err("この図の評価位置指定には未対応です。位置を現在値で代用しません。".into());
        }
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("結果がありません。")?;
        if let Some(mode) = reference.display.mode {
            let count = match reference.kind {
                ResultInputKey::Modal => bundle.modal.as_ref().map_or(0, |m| m.shapes.len()),
                ResultInputKey::LumpedTimeHistory(id) => bundle
                    .lumped_result_for(id)
                    .map_or(0, |r| r.modal.shapes.len()),
                _ => 0,
            };
            if mode as usize >= count {
                return Err("参照するモードは取得不能です。".into());
            }
        }
        if let Some(step) = reference.display.step {
            let count = match reference.kind {
                ResultInputKey::TimeHistory(id) => bundle
                    .time_history_for(id)
                    .and_then(|r| r.recording.as_ref())
                    .map_or(0, |r| r.frame_time.len()),
                ResultInputKey::LumpedTimeHistory(id) => bundle
                    .lumped_result_for(id)
                    .and_then(|r| r.response.as_ref())
                    .map_or(0, |r| r.time.len()),
                ResultInputKey::Pushover(dir) => {
                    let run = match dir {
                        SeismicDir::X => bundle.pushover_x.as_ref(),
                        SeismicDir::Y => bundle.pushover_y.as_ref(),
                    };
                    let member = match reference.target.target {
                        ReferenceTarget::Member(id) => Some(id),
                        _ => None,
                    };
                    run.and_then(|r| r.member_history.iter().find(|h| Some(h.elem) == member))
                        .map_or(0, |h| h.records.len())
                }
                _ => 0,
            };
            if step as usize >= count {
                return Err("参照するステップは取得不能です。".into());
            }
        }
        Ok(())
    }

    pub fn freeze_selected_references(&mut self) -> Result<(), String> {
        let mut selected: Vec<_> = self
            .ui
            .scoped
            .selection
            .nodes()
            .iter()
            .copied()
            .map(ReferenceTarget::Node)
            .chain(
                self.ui
                    .scoped
                    .selection
                    .members()
                    .iter()
                    .copied()
                    .map(ReferenceTarget::Member),
            )
            .collect();
        if selected.is_empty() {
            if let Some(id) = self.ui.scoped.nav.focus_section {
                selected.push(ReferenceTarget::Section(id));
            }
            if let Some(id) = self.ui.scoped.nav.focus_material {
                selected.push(ReferenceTarget::Material(id));
            }
            if let Some(id) = self.ui.scoped.nav.focus_load_case {
                selected.push(ReferenceTarget::LoadCase(id));
            }
            if let Some(frame) = self.active_viewport().frame_target {
                selected.push(match frame {
                    sepika_core::frame::FrameTarget::Story(id) => ReferenceTarget::Story(id),
                    sepika_core::frame::FrameTarget::Axis { group, axis } => {
                        ReferenceTarget::Axis { group, axis }
                    }
                });
            }
        }
        let input = self.input_reference();
        let fixed = selected
            .into_iter()
            .map(|target| self.target_reference_at(target, input.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        self.core.scoped.references.fixed = fixed;
        self.core.scoped.staleness.mark_non_calc_edited();
        Ok(())
    }

    pub fn show_reference(
        &mut self,
        reference: &TargetReference,
        view: ReferenceView,
        reveal: bool,
    ) -> Result<(), String> {
        if let Some(location) = self.study_target_location(reference) {
            if self.studies.active != location {
                self.switch_study(location)?;
            }
        }

        let destination = if matches!(
            reference.target,
            ReferenceTarget::Vibration(_) | ReferenceTarget::LumpedVibration(_)
        ) {
            Tab::Results
        } else {
            Tab::Model
        };
        self.show_reference_at(reference, view, reveal, destination)
    }

    fn show_reference_at(
        &mut self,
        reference: &TargetReference,
        view: ReferenceView,
        reveal: bool,
        destination: Tab,
    ) -> Result<(), String> {
        let target = self.resolve_target_reference(reference)?;
        self.require_reference_view(target, view)?;
        self.show_resolved_reference(reference, target, view, reveal, destination)
    }

    pub(super) fn require_reference_view(
        &self,
        target: ReferenceTarget,
        view: ReferenceView,
    ) -> Result<(), String> {
        match (target, view) {
            (
                ReferenceTarget::Material(_)
                | ReferenceTarget::LoadCase(_)
                | ReferenceTarget::Combination(_),
                ReferenceView::Diagram,
            ) => {
                return Err(
                    "この対象の図表示には未対応です。表または詳細を使用してください。".into(),
                )
            }
            (
                ReferenceTarget::Story(_)
                | ReferenceTarget::Vibration(_)
                | ReferenceTarget::LumpedVibration(_),
                ReferenceView::Table,
            ) => return Err("この対象の表表示には未対応です。詳細を使用してください。".into()),
            (ReferenceTarget::Vibration(id), ReferenceView::Diagram) => {
                self.require_result_input(ResultInputKey::TimeHistory(id))?
            }
            (ReferenceTarget::LumpedVibration(id), ReferenceView::Diagram) => {
                self.require_result_input(ResultInputKey::LumpedTimeHistory(id))?
            }
            (ReferenceTarget::Section(id), ReferenceView::Diagram)
                if !self
                    .core
                    .model
                    .elements
                    .iter()
                    .any(|member| member.section == Some(id)) =>
            {
                return Err("この断面を使う表示対象部材がありません。".into())
            }
            _ => {}
        }
        Ok(())
    }

    fn show_resolved_reference(
        &mut self,
        reference: &TargetReference,
        target: ReferenceTarget,
        view: ReferenceView,
        reveal: bool,
        destination: Tab,
    ) -> Result<(), String> {
        if self.reference_hidden(target) && !reveal {
            return Err("対象は現在の階・通りで非表示です。「対象を表示」で表示できます。".into());
        }
        if matches!(view, ReferenceView::Diagram) {
            self.ui.view.active_tab = destination;
            self.show_workspace_reference_diagram();
        }
        if reveal {
            self.ui.view.frame_target = None;
        }
        self.ui.scoped.opened_reference = Some(reference.clone());
        self.ui.scoped.nav.focus_section = None;
        self.ui.scoped.nav.focus_material = None;
        self.ui.scoped.nav.focus_load_case = None;
        self.ui.scoped.nav.focus_vibration_case = None;
        self.ui.scoped.nav.focus_lumped_vibration_case = None;
        if !matches!(
            target,
            ReferenceTarget::Node(_) | ReferenceTarget::Member(_)
        ) {
            self.clear_geometry_selection();
        }
        match target {
            ReferenceTarget::Node(id) => self.select_node(id),
            ReferenceTarget::Member(id) => self.select_member(id),
            ReferenceTarget::Section(id) => {
                self.ui.scoped.nav.focus_section = Some(id);
                if matches!(view, ReferenceView::Diagram) {
                    let members = self
                        .core
                        .model
                        .elements
                        .iter()
                        .filter(|member| member.section == Some(id))
                        .map(|member| member.id)
                        .collect();
                    self.select_members(members, None);
                }
            }
            ReferenceTarget::Material(id) => self.ui.scoped.nav.focus_material = Some(id),
            ReferenceTarget::Story(id) => {
                self.ui.view.frame_target = Some(sepika_core::frame::FrameTarget::Story(id))
            }
            ReferenceTarget::Axis { group, axis } => {
                self.ui.view.frame_target =
                    Some(sepika_core::frame::FrameTarget::Axis { group, axis })
            }
            ReferenceTarget::LoadCase(id) => self.ui.scoped.nav.focus_load_case = Some(id),
            ReferenceTarget::Combination(index) => {
                let name = &self.core.model.combinations[index].name;
                let result = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.combos.iter().position(|(n, _)| n == name));
                self.ui.scoped.nav.focus_result = result.map(StaticKey::Combo);
            }
            ReferenceTarget::Vibration(id) => {
                self.ui.scoped.nav.focus_vibration_case = Some(id);
                if matches!(view, ReferenceView::Diagram) {
                    self.bind_spatial_case_to_active_view(id);
                    self.ui.view.viewport.view_mode = crate::viewer::ViewMode::TimeHistory;
                }
            }
            ReferenceTarget::LumpedVibration(id) => {
                self.ui.scoped.nav.focus_lumped_vibration_case = Some(id);
                if matches!(view, ReferenceView::Diagram) {
                    self.bind_lumped_case_to_active_view(id);
                    self.ui.view.viewport.view_mode = crate::viewer::ViewMode::LumpedTimeHistory;
                }
            }
        }
        match view {
            ReferenceView::Diagram => {
                if !matches!(
                    target,
                    ReferenceTarget::Vibration(_) | ReferenceTarget::LumpedVibration(_)
                ) {
                    self.ui.view.active_tab = destination;
                    self.ui.view.view_mode = crate::viewer::ViewMode::Shape;
                }
                self.show_workspace_reference_diagram();
            }
            ReferenceView::Table => {
                self.enter_workspace();
                self.ui.view.bottom_dock_open = true;
                self.ui.view.bottom_tab = BottomTab::Model;
                self.ui.view.model_tab = match target {
                    ReferenceTarget::Node(_) => ModelTab::Nodes,
                    ReferenceTarget::Member(_) => ModelTab::Members,
                    ReferenceTarget::Section(_) => ModelTab::Sections,
                    ReferenceTarget::Material(_) => ModelTab::Materials,
                    ReferenceTarget::Axis { .. } => ModelTab::Axes,
                    _ => {
                        self.ui.view.bottom_tab = BottomTab::Loads;
                        ModelTab::Nodes
                    }
                };
                self.promote_table(if self.ui.view.bottom_tab == BottomTab::Loads {
                    super::workspace::Content::Loads
                } else {
                    super::workspace::Content::Model
                });
            }
            ReferenceView::Detail => {
                self.ui.view.right_dock_open = true;
                self.ui.view.right_panel = RightPanel::Inspector;
                self.ui.view.workspace.compact_dock = Some(false);
            }
        }
        Ok(())
    }

    pub(super) fn reference_hidden(&self, target: ReferenceTarget) -> bool {
        self.reference_hidden_in(target, self.ui.view.frame_target)
    }

    fn reference_hidden_in(
        &self,
        target: ReferenceTarget,
        frame: Option<sepika_core::frame::FrameTarget>,
    ) -> bool {
        let frame =
            frame.and_then(|target| sepika_core::frame::build_frame(&self.core.model, target));
        match (frame, target) {
            (Some(frame), ReferenceTarget::Node(id)) => {
                !frame.node_on.get(id.index()).copied().unwrap_or(false)
            }
            (Some(frame), ReferenceTarget::Member(id)) => {
                !frame.elem_on.get(id.index()).copied().unwrap_or(false)
            }
            _ => false,
        }
    }

    pub(crate) fn reference_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(reference) = &self.ui.scoped.opened_reference {
            ui.strong("参照から開いた対象");
            match self.resolve_target_reference(reference) {
                Err(reason) => {
                    ui.label(reason);
                }
                Ok(target) => {
                    ui.label(self.reference_target_description(target));
                }
            }
        }
        ui.collapsing("共通参照・固定対象", |ui| {
            let input = self.input_reference();
            ui.label(format!(
                "{} / 編集世代 {}",
                if self.study_is_main() {
                    "本案"
                } else {
                    "表示中の検討入力"
                },
                input.generation
            ));
            ui.label(format!("プロジェクト: {}", input.project));
            ui.label(format!("案: {}", input.proposal));
            ui.label(format!(
                "読込セッション（保存しない）: {}",
                self.core.scoped.reference_session
            ));
            ui.label(format!(
                "画面選択: {}件 / 固定対象: {}件",
                self.ui.scoped.selection.nodes().len() + self.ui.scoped.selection.members().len(),
                self.core.scoped.references.fixed.len()
            ));
            if ui.button("画面選択を固定対象にする").clicked() {
                if let Err(reason) = self.freeze_selected_references() {
                    self.report_notice(reason);
                }
            }
            for reference in self.core.scoped.references.fixed.clone() {
                ui.label(format!(
                    "固定: {:?} / 世代 {}",
                    reference.target, reference.source.generation
                ));
                match self.resolve_target_reference_at(&reference, &input) {
                    Err(reason) => {
                        ui.label(reason);
                    }
                    Ok(target) => {
                        if self.reference_hidden(target) {
                            ui.label("非表示（階・通りの絞込み）");
                        }
                        ui.horizontal(|ui| {
                            for (label, view) in [
                                ("図へ", ReferenceView::Diagram),
                                ("表へ", ReferenceView::Table),
                                ("詳細へ", ReferenceView::Detail),
                            ] {
                                if ui.button(label).clicked() {
                                    if let Err(reason) =
                                        self.show_reference(&reference, view, false)
                                    {
                                        self.report_notice(reason);
                                    }
                                }
                            }
                            if ui.button("対象を表示").clicked() {
                                if let Err(reason) =
                                    self.show_reference(&reference, ReferenceView::Diagram, true)
                                {
                                    self.report_notice(reason);
                                }
                            }
                        });
                    }
                }
            }
            ui.separator();
            ui.strong("結果の計算時入力（読み取り専用）");
            let keys: Vec<_> = self
                .core
                .scoped
                .results
                .as_ref()
                .map(|bundle| {
                    bundle
                        .input_records
                        .iter()
                        .map(|(key, _)| key.clone())
                        .collect()
                })
                .unwrap_or_default();
            for key in keys {
                if ui
                    .button(format!("{:?} の固定対象・結果参照を保存", key))
                    .clicked()
                {
                    let fixed = self.core.scoped.references.fixed.clone();
                    for target in fixed {
                        let display = match self.current_display_conditions() {
                            Ok(display) => display,
                            Err(reason) => {
                                self.report_notice(reason);
                                break;
                            }
                        };
                        match self.result_reference(target, key.clone(), display) {
                            Ok(reference) => match self.validate_reference_display(&reference) {
                                Ok(()) => {
                                    self.core.scoped.references.fixed_results.push(reference);
                                    self.core.scoped.staleness.mark_non_calc_edited();
                                    self.retain_study_view_references();
                                }
                                Err(reason) => self.report_notice(reason),
                            },
                            Err(reason) => self.report_notice(reason),
                        }
                    }
                }
                if ui.button(format!("{:?} の計算時入力を開く", key)).clicked() {
                    match self.recorded_calculation_input(&key) {
                        Ok((source, model)) => {
                            self.ui.scoped.reference_snapshot = Some((
                                format!(
                                    "計算時入力 / 案 {} / 世代 {} / {:?}",
                                    source.proposal, source.generation, key
                                ),
                                model,
                            ))
                        }
                        Err(reason) => self.report_notice(reason),
                    }
                }
            }
            for reference in self.core.scoped.references.fixed_results.clone() {
                ui.label(format!(
                    "固定結果: {:?} / run {} / 世代 {}",
                    reference.kind, reference.run, reference.source.generation
                ));
                if ui.button("固定した結果の図へ戻る").clicked() {
                    if let Err(reason) =
                        self.show_result_reference(&reference, ReferenceView::Diagram, false)
                    {
                        self.report_notice(reason);
                    }
                }
                if ui.button("固定した結果の対象を表示").clicked() {
                    if let Err(reason) =
                        self.show_result_reference(&reference, ReferenceView::Diagram, true)
                    {
                        self.report_notice(reason);
                    }
                }
                if ui.button("固定結果の計算時入力を開く").clicked() {
                    match self.recorded_result_input(&reference) {
                        Ok(model) => {
                            self.ui.scoped.reference_snapshot = Some((
                                format!("固定結果の計算時入力 / run {}", reference.run),
                                model,
                            ))
                        }
                        Err(reason) => self.report_notice(reason),
                    }
                }
            }
            ui.label("現在の入力を編集するには、この閲覧を閉じて現在案の図・入力表を使用します。");
        });
    }

    pub(crate) fn reference_target_description(&self, target: ReferenceTarget) -> String {
        let model = &self.core.model;
        match target {
            ReferenceTarget::Node(id) => model
                .node(id)
                .map(|node| format!("節点 #{} / 座標 {:?} mm", id.0, node.coord)),
            ReferenceTarget::Member(id) => model.element(id).map(|member| {
                format!(
                    "部材 #{} / 節点 {:?} / 断面 {:?}",
                    id.0, member.nodes, member.section
                )
            }),
            ReferenceTarget::Section(id) => model.section(id).map(|section| {
                format!(
                    "断面 #{} {} / A={} mm² / 影響部材 {}件",
                    id.0,
                    section.name,
                    section.area,
                    model
                        .elements
                        .iter()
                        .filter(|member| member.section == Some(id))
                        .count()
                )
            }),
            ReferenceTarget::Material(id) => model
                .materials
                .iter()
                .find(|material| material.id == id)
                .map(|material| {
                    format!(
                        "材料 #{} {} / {} / E={} N/mm² / ν={} / 密度={} t/mm³ / G={:?} N/mm² / Fc={:?} N/mm² / Fy={:?} N/mm²",
                        id.0, material.name, material.category.label(), material.young, material.poisson, material.density, material.shear, material.fc, material.fy
                    )
                }),
            ReferenceTarget::Story(id) => model
                .stories
                .iter()
                .find(|story| story.id == id)
                .map(|story| format!("階 #{} {}", id.0, story.name)),
            ReferenceTarget::Axis { group, axis } => model
                .axes
                .get(group)
                .and_then(|g| g.axes.get(axis))
                .map(|axis| format!("通り {}", axis.name)),
            ReferenceTarget::LoadCase(id) => model
                .load_cases
                .iter()
                .find(|case| case.id == id)
                .map(|case| format!("荷重ケース #{} {}", id.0, case.name)),
            ReferenceTarget::Combination(index) => model.combinations.get(index).map(|combo| {
                format!(
                    "荷重組合せ #{} {} / 係数 {:?}",
                    index, combo.name, combo.terms
                )
            }),
            ReferenceTarget::Vibration(id) => model
                .vibration_cases
                .iter()
                .find(|case| case.id == id)
                .map(|case| format!("立体振動ケース #{} {}", id.0, case.name)),
            ReferenceTarget::LumpedVibration(id) => model
                .lumped_vibration_cases
                .iter()
                .find(|case| case.id == id)
                .map(|case| format!("質点系振動ケース #{} {}", id.0, case.name)),
        }
        .unwrap_or_else(|| "対象が取得不能です。".into())
    }

    pub(crate) fn reference_snapshot_window(&mut self, ctx: &egui::Context) {
        let Some((title, input)) = self.ui.scoped.reference_snapshot.as_ref() else {
            return;
        };
        let current = self.input_reference();
        let model = &input.model;
        let mut open = true;
        let mut edit_current = false;
        let mut move_to_current = false;
        egui::Window::new(title).open(&mut open).show(ctx, |ui| {
            ui.strong("読み取り専用・計算時の計算用入力");
            if matches!(input.settings, RecordedSettings::Quantity) {
                ui.label(format!("数量設定: 壁筋仮定径 {} mm / 定着係数 {} 径倍", model.quantity_cfg.assumed_wall_bar_dia, model.quantity_cfg.anchorage_dia_factor));
                ui.label(format!("小梁主筋比 {} / 小梁スターラップ比 {} / 床・片持ち床の鉄筋比 {}（コンクリート体積比・無次元）", model.quantity_cfg.beam_main_ratio, model.quantity_cfg.beam_stirrup_ratio, model.quantity_cfg.slab_rebar_ratio));
            } else {
                ui.label("通り・数量・保存波形設定等を含む完全な編集モデルは取得不能です。");
            }
            ui.label("現在の編集入力とは独立した、結果生成時の入力です。");
            Self::recorded_settings_ui(ui, &input.settings);
            egui::ScrollArea::vertical()
                .max_height(450.0)
                .show(ui, |ui| Self::recorded_input_contents_ui(ui, input));
            ui.label(format!("現在表示の入力: 案 {} / 入力世代 {}（編集への移動先は本案です）", current.proposal, current.generation));
            if ui.button("元の結果へ戻る（閲覧を閉じる）").clicked() {
                edit_current = true;
            }
            if ui.button("現在の本案の入力表へ移動（計算時入力の閲覧は保持）").clicked() {
                move_to_current = true;
            }
        });
        if move_to_current {
            match self.enter_evidence_editor(self.ui.view.model_tab) {
                Ok(return_to) => {
                    if self.ui.scoped.reference_snapshot_return.is_none() {
                        self.ui.scoped.reference_snapshot_return = Some(return_to);
                    }
                }
                Err(reason) => self.report_error(reason),
            }
        }
        if !open || edit_current {
            self.ui.scoped.reference_snapshot = None;
            if let Some(return_to) = self.ui.scoped.reference_snapshot_return.take() {
                self.return_from_evidence_editor(return_to);
            }
        }
    }

    pub(crate) fn recorded_input_contents_ui(ui: &mut egui::Ui, input: &RecordedCalculationInput) {
        let model = &input.model;
        if matches!(input.settings, RecordedSettings::Quantity) {
            ui.collapsing("数量対象の配置・支持範囲（読み取り専用）", |ui| {
                for slab in &model.slabs {
                    ui.label(format!("床板 #{} / 断面 {:?} / 基準階 {:?} / 鉛直偏心 {:?} mm / 配置 {:?}", slab.id.0, slab.plate.section, slab.reference_story, slab.vertical_eccentricity_mm, slab.shape));
                    ui.label(format!("床板 #{} の支持境界 [mm]: {}", slab.id.0, slab.boundary_coords(model).map_or_else(|| "取得不能".into(), |coords| format!("{coords:?}"))));
                    if let Some(region) = model.slab_assignment_region(slab.id) { ui.label(format!("床板 #{} の支持部材・材軸区間（無次元）: {:?}", slab.id.0, region.boundary)); }
                    match model.resolved_slab_placement(slab.id) {
                        Ok(placement) => { ui.label(format!("床板 #{} の実配置: 上面Z={} mm / 下面Z={} mm / 実厚={} mm / 上面境界 {:?} mm", slab.id.0, placement.top_z_mm, placement.bottom_z_mm, placement.thickness_mm, placement.top_boundary)); }
                        Err(reason) => { ui.label(format!("床板 #{} の実配置は取得不能: {reason}", slab.id.0)); }
                    }
                }
                for plate in &model.wall_plates {
                    ui.label(format!("壁版 #{} / 断面 {:?} / 配置 {:?} / 開口面積 {} mm² / 個別開口 {:?}", plate.id.0, plate.section, plate.shape, plate.opening_area, plate.openings));
                    ui.label(format!("壁版 #{} の境界 [mm]: {}", plate.id.0, plate.boundary_coords(model).map_or_else(|| "取得不能".into(), |coords| format!("{coords:?}"))));
                    if let Some(region) = model.wall_plate_assignment_region(plate.id) { ui.label(format!("壁版 #{} の支持部材・材軸区間（無次元）: {:?}", plate.id.0, region.boundary)); }
                }
                for region in &model.floor_regions { ui.label(format!("床領域 #{} {} / 境界節点 {:?} / 床板 {:?}", region.id.0, region.name, region.boundary, region.slab_ids)); }
                for region in &model.wall_regions { ui.label(format!("壁領域 #{} {} / 境界節点 {:?} / 壁版 {:?}", region.id.0, region.name, region.boundary, region.wall_plate_ids)); }
                for member in model.floor_regions.iter().flat_map(|region| &region.secondary_beams).chain(model.wall_regions.iter().flat_map(|region| &region.posts)).chain(&model.unassigned_beams).chain(&model.unassigned_posts) {
                    ui.label(format!("二次部材 #{} {} / 種別 {:?} / 断面 {:?} / 支持位置 {:?} / 鉛直偏心 {:?} mm", member.id.0, member.name, member.kind, member.section, member.ends, member.vertical_eccentricity));
                    ui.label(format!("二次部材 #{} の両端 [mm]: {}", member.id.0, model.secondary_member_end_points(member).map_or_else(|| "取得不能".into(), |coords| format!("{coords:?}"))));
                    let steel = member.section.and_then(|id| model.section(id)).and_then(|section| {
                        if model.stb_strengths.sections.iter().any(|input| input.section == section.id && !input.steel.is_empty()) { model.stb_steel_material(section.id) }
                        else { section.steel_material.and_then(|id| model.materials.get(id.index())) }
                    });
                    for (role, material) in [("主材料", model.secondary_material(member)), ("主筋", model.secondary_rebar_material(member)), ("せん断補強筋", model.secondary_shear_rebar_material(member)), ("内蔵鉄骨・鋼管", steel)] {
                        ui.label(material.map_or_else(|| format!("二次部材 #{} / {role}: 未割当・解決不能（形状により対象外の役割を含む）", member.id.0), |material| format!("二次部材 #{} / {role}: #{} {} / Fc={:?} N/mm² / Fy={:?} N/mm²", member.id.0, material.id.0, material.name, material.fc, material.fy)));
                    }
                }
            });
        }
        ui.collapsing("節点 [mm]", |ui| {
            for node in &model.nodes {
                ui.label(format!(
                    "{:?}: {:?} / 所属階 {:?}",
                    node.id, node.coord, node.story
                ));
                ui.label(format!(
                    "拘束 {:?} / 支点ばね {:?}",
                    node.restraint, node.support_spring
                ));
            }
        });
        ui.collapsing("階の配置 [mm]", |ui| {
            for story in &model.stories {
                ui.label(format!(
                    "階 #{} {} / FL {:?} mm / ΔSL {:?} mm",
                    story.id.0, story.name, story.elevation, story.sl_offset_mm
                ));
            }
        });
        ui.collapsing("部材・断面", |ui| {
            for member in &model.elements {
                ui.label(format!(
                    "{:?}: 節点 {:?} / 断面 {:?}",
                    member.id, member.nodes, member.section
                ));
                for line in recorded_member_material_lines(model, member) { ui.label(line); }
                ui.label(format!("種別 {:?} / フェース距離 i={:?} mm・j={:?} mm（Noneは未算定） / 鉛直偏心 {:?} mm / 局所軸参照方向 {:?}", member.kind, member.rigid_zone.face_i, member.rigid_zone.face_j, member.vertical_eccentricity, member.local_axis.ref_vector));
                if let Some(detail) = model.member_detail(member.id) { ui.label(format!("ハンチ i={:?} / j={:?}（長さ・せい増分・幅増分 mm）", detail.haunch_i, detail.haunch_j)); }
                else { ui.label("ハンチ: 指定なし"); }
                if let Some(wall) = model.wall_attrs.iter().find(|attr| attr.elem == member.id) { ui.label(format!("壁・シェルの開口面積 {} mm² / 個別開口 {:?}（寸法・位置 mm）", wall.opening_area, wall.openings)); }
            }
            for section in &model.sections {
                ui.label(format!(
                    "#{} {} / A={} mm² / Iy={} mm⁴ / Iz={} mm⁴",
                    section.id.0, section.name, section.area, section.iy, section.iz
                ));
                ui.label(format!("断面の明示割当: 主材料 {:?} / 主筋 {:?} / せん断補強筋 {:?} / 内蔵鉄骨・鋼管 {:?}", section.material, section.rebar_material, section.shear_rebar_material, section.steel_material));
                ui.label(format!("断面形状・配筋 {:?}", section.shape));
                ui.label(format!("断面用途 {:?} / 幅 {} mm / せい {} mm / 実板厚 {:?} mm / 断面積の採用元 {:?}", section.frame_use, section.width, section.depth, section.thickness, section.property_basis.area));
            }
        });
        ui.collapsing("荷重ケース・組合せ", |ui| {
            for case in &model.load_cases {
                ui.label(format!("{:?}: {} / {:?}", case.id, case.name, case.kind));
                for load in &case.nodal {
                    ui.label(format!(
                        "節点 #{} / {} / [Fx,Fy,Fz] N・[Mx,My,Mz] N·mm: {:?}",
                        load.node.0, load.name, load.values
                    ));
                }
                for load in &case.member {
                    ui.label(format!(
                        "部材 #{} / 全体座標方向 {:?} / 荷重 {:?}（位置mm・集中N・分布N/mm）",
                        load.elem.0, load.dir, load.kind
                    ));
                }
            }
            for combo in &model.combinations {
                ui.label(format!("{}: {:?}", combo.name, combo.terms));
            }
        });
        ui.collapsing("材料", |ui| {
            for material in &model.materials {
                ui.label(format!(
                    "#{} {} / 種別 {:?} / E={} N/mm² / ν={} / 密度={} t/mm³ / Fc={:?} N/mm² / Fy={:?} N/mm²",
                    material.id.0, material.name, material.category, material.young, material.poisson, material.density, material.fc, material.fy
                ));
            }
        });
    }

    fn recorded_settings_ui(ui: &mut egui::Ui, settings: &RecordedSettings) {
        ui.strong("保存された計算条件（読み取り専用）");
        match settings {
            RecordedSettings::Quantity => {
                ui.label("数量は表示時の入力から集計しています。解析・検定の成功を意味しません。");
            }
            RecordedSettings::Analysis {
                kind,
                cfg,
                period_s,
            } => {
                Self::recorded_analysis_settings_ui(ui, cfg);
                if matches!(kind, ResultInputKey::Pushover(_)) {
                    ui.label(format!("増分解析方向: {:?} / 最大ステップ数: {} / 目標変位 [mm]: {} / 制御方式: {:?} / 長期荷重: {} / 靭性評価方式: {:?}", cfg.push_dir, cfg.push_steps, cfg.push_max_disp, cfg.push_control, cfg.push_apply_long_term, cfg.ductility_method));
                    ui.label(format!(
                        "目標変位条件: {} / 層間変形角条件: {} / 変形角分母: {}",
                        cfg.push_use_max_disp, cfg.push_use_drift_angle, cfg.push_drift_denom
                    ));
                }
                ui.label(format!("地震荷重生成に採用した周期 [s]: {period_s:?}"));
                ui.label("照合に含まれない条件・要求モード数・完全な編集時設定は取得不能です。");
            }
            RecordedSettings::Design {
                short_term,
                rc_damage_control,
                bond_method,
                qd_method,
                source,
            } => {
                ui.label(format!(
                    "継続時間: {} / RC損傷制御: {} / 付着方式: {:?} / 設計用せん断力方式: {:?}",
                    if *short_term { "短期" } else { "長期" },
                    rc_damage_control,
                    bond_method,
                    qd_method
                ));
                if let Some((key, input)) = source {
                    ui.label(format!("応力の生成元: {key:?}"));
                    if let Some(input) = input {
                        ui.collapsing("応力生成時の条件", |ui| {
                            Self::recorded_settings_ui(ui, &input.settings)
                        });
                    } else {
                        ui.label("応力生成時の条件は取得不能です。");
                    }
                } else {
                    ui.label("応力源を使用しない直接検定です。");
                }
            }
            RecordedSettings::Vibration {
                analysis_identity,
                identity,
                cfg,
            } => {
                ui.label(format!("実波形・実効条件の識別: {analysis_identity}"));
                ui.label(format!("計算モデルと実波形の組の識別: {identity}"));
                if let Some(cfg) = cfg {
                    Self::recorded_analysis_settings_ui(ui, cfg);
                    ui.label(format!("時刻歴方向: {:?} / 非線形: {} / 時間刻み [s]: {} / 質点系方向: {:?} / 質点系非線形: {}", cfg.th_dir, cfg.th_nonlinear, cfg.th_dt, cfg.lumped_dir, cfg.lumped_nonlinear));
                    ui.label(format!("減衰モデル: {:?} / 減衰定数: {} / 第2減衰定数: {} / 長期荷重: {} / 最大反復: {} / 許容誤差: {} / 記録間隔: {}", cfg.th_damping_model, cfg.th_damping, cfg.th_h2, cfg.th_apply_long_term, cfg.th_max_iter, cfg.th_tol, cfg.th_record_every));
                    ui.label(format!(
                        "波形生成設定: 継続時間 {} s / 周期 {} s / 振幅 {} mm/s²",
                        cfg.th_duration, cfg.th_period, cfg.th_amp
                    ));
                    ui.label(format!(
                        "質点系: {:?} / {:?} / 剛性 {:?} / 割線剛性比 {} / モード数 {}",
                        cfg.lumped_dim,
                        cfg.lumped_mass_type,
                        cfg.lumped_stiffness,
                        cfg.lumped_secant_ratio,
                        cfg.lumped_n_modes
                    ));
                    ui.label(format!(
                        "質点系減衰 {} / 刻み {} s / 継続時間 {} s / 周期 {} s / 振幅 {} mm/s²",
                        cfg.lumped_th_damping,
                        cfg.lumped_th_dt,
                        cfg.lumped_th_duration,
                        cfg.lumped_th_period,
                        cfg.lumped_th_amp
                    ));
                } else {
                    ui.label("計算時の時刻歴設定は記録されていないため取得不能です。");
                }
                ui.label("この記録は波形の識別を保持します。実波形列はこの記録に保存されていないため取得不能です。");
            }
        }
    }

    fn recorded_analysis_settings_ui(ui: &mut egui::Ui, cfg: &AnalysisSettings) {
        ui.label(format!(
            "標準せん断力係数 C0: {} / 地域係数 Z: {} / 地盤種別: {:?} / Ai算定: {:?}",
            cfg.c0, cfg.z, cfg.soil, cfg.ai_mode
        ));
        ui.label(format!(
            "質量方式: {:?} / 多雪区域: {} / 積雪係数 δ1: {} / δ3: {} / 計算スレッド数: {}",
            cfg.mass_method, cfg.heavy_snow_zone, cfg.snow_delta1, cfg.snow_delta3, cfg.threads
        ));
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    fn sample() -> App {
        let mut app = App::default();
        assert!(app.load_model(crate::sample::portal_frame()));
        app
    }

    fn calculate(app: &mut App) -> ResultReference {
        let id = app.core.model.load_cases[0].id;
        app.run_linear_static(id);
        let target = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        app.result_reference(
            target,
            ResultInputKey::Static(StaticCaseKey::User(id)),
            DisplayConditions::default(),
        )
        .unwrap()
    }

    #[test]
    fn 同整数idでも別プロジェクトと別案を解決しない() {
        let app = sample();
        let reference = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let mut other = sample();
        assert!(other
            .resolve_target_reference(&reference)
            .unwrap_err()
            .contains("別プロジェクト"));
        other.core.scoped.references.project = app.core.scoped.references.project.clone();
        assert!(other
            .resolve_target_reference(&reference)
            .unwrap_err()
            .contains("別案"));
        other.core.scoped.references = app.core.scoped.references.clone();
        other.core.model.nodes[0].coord[0] += 1.0;
        assert!(other
            .resolve_target_reference(&reference)
            .unwrap_err()
            .contains("入力版"));
    }

    #[test]
    fn 固定対象は選択変更と削除再配置とundoで読み替えない() {
        let mut app = sample();
        app.select_member(ElemId(0));
        app.freeze_selected_references().unwrap();
        let fixed = app.core.scoped.references.fixed.clone();
        app.select_member(ElemId(1));
        assert_eq!(app.core.scoped.references.fixed, fixed);
        let section = app
            .target_reference(ReferenceTarget::Section(SectionId(0)))
            .unwrap();
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [1.0, 0.0, 0.0]
        })));
        let edited = app.input_reference().generation;
        assert!(app.resolve_target_reference(&fixed[0]).is_err());
        app.undo_action();
        assert!(app.input_reference().generation > edited);
        assert!(app.resolve_target_reference(&section).is_err());
        app.redo_action();
        assert_eq!(app.core.scoped.references.fixed, fixed);
    }

    #[test]
    fn 表の直接undo経路でも編集世代を再使用しない() {
        let mut app = sample();
        let reference = app
            .target_reference(ReferenceTarget::Node(NodeId(0)))
            .unwrap();
        assert!(app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetNodeCoord {
                node: NodeId(0),
                coord: [1.0, 0.0, 0.0]
            })
        ));
        app.core.scoped.undo.undo(&mut app.core.model);
        assert!(app.resolve_target_reference(&reference).is_err());
        assert!(app.input_reference().generation >= 2);
    }

    #[test]
    fn 保存参照は同入力の再読込で解決し欠落は理由を返す() {
        let mut app = sample();
        app.select_member(ElemId(0));
        app.freeze_selected_references().unwrap();
        let reference = app.core.scoped.references.fixed[0].clone();
        let connection = app.connection_reference();
        let path =
            std::env::temp_dir().join(format!("sepika-reference-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            loaded.resolve_target_reference(&reference),
            Ok(reference.target)
        );
        assert_eq!(loaded.core.scoped.references.fixed, vec![reference.clone()]);
        assert!(loaded.core.scoped.job.is_none());
        assert!(loaded
            .validate_connection_reference(&connection)
            .unwrap_err()
            .contains("接続セッション"));
        assert!(loaded
            .validate_connection_reference(&loaded.connection_reference())
            .is_ok());
        let mut missing = reference;
        missing.target = ReferenceTarget::Member(ElemId(999));
        assert!(loaded
            .resolve_target_reference(&missing)
            .unwrap_err()
            .contains("存在"));
    }

    #[test]
    fn 設定と実効組合せとrunが異なる結果を取り違えない() {
        let mut app = sample();
        let reference = calculate(&mut app);
        assert!(app.resolve_result_reference(&reference).is_ok());
        app.core.analysis_cfg.c0 *= 1.1;
        assert!(app.resolve_result_reference(&reference).is_err());
        assert!(app.result_input_snapshot(&reference).is_ok());
        app.core.analysis_cfg.c0 /= 1.1;
        let second = calculate(&mut app);
        assert_ne!(reference.run, second.run);
        assert!(app.result_input_snapshot(&reference).is_err());
    }

    #[test]
    fn 古い結果の入力閲覧は現在入力とundoと有効性を変えない() {
        let mut app = sample();
        let reference = calculate(&mut app);
        let original = app.core.model.nodes[0].coord;
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [1.0, 0.0, 0.0]
        })));
        let input = app.input_reference();
        let revision = app.core.scoped.undo.revision();
        let stale = app.core.scoped.staleness.results_stale;
        let snapshot = app.result_input_snapshot(&reference).unwrap();
        assert_eq!(snapshot.nodes[0].coord, original);
        assert_ne!(snapshot.nodes[0].coord, app.core.model.nodes[0].coord);
        assert!(app.resolve_result_reference(&reference).is_err());
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(app.core.scoped.staleness.results_stale, stale);
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .input_records
            .clear();
        assert!(app
            .result_input_snapshot(&reference)
            .unwrap_err()
            .contains("取得不能"));
    }

    #[test]
    fn 共通表示と表示条件変更はモデルと結果を変更しない() {
        let mut app = sample();
        let reference = calculate(&mut app);
        let input = app.input_reference();
        let revision = app.core.scoped.undo.revision();
        for view in [
            ReferenceView::Diagram,
            ReferenceView::Table,
            ReferenceView::Detail,
        ] {
            app.show_reference(&reference.target, view, false).unwrap();
        }
        let mut with_units = reference.clone();
        with_units.display.unit = Some("kN".into());
        with_units.display.component = Some("N".into());
        with_units.display.mode = Some(1);
        assert!(app.resolve_result_reference(&with_units).is_ok());
        assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(0)));
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(!app.core.scoped.staleness.results_stale);
    }

    #[test]
    fn 非計算編集後に古い結果の案版を現在版へ付け替えない() {
        let mut app = sample();
        let reference = calculate(&mut app);
        let axes = sepika_core::axis_gen::generate_axes(&app.core.model);
        assert!(app.apply_model_edit(Box::new(sepika_edit::ReplaceAxes { axes })));
        let target = app.target_reference(reference.target.target).unwrap();
        assert!(app
            .result_reference(target, reference.kind.clone(), DisplayConditions::default())
            .unwrap_err()
            .contains("計算時"));
        let (source, _) = app.calculation_snapshot(&reference.kind).unwrap();
        assert_eq!(source, reference.source);
    }

    #[test]
    fn 削除再利用と下位削除の再配置でも旧対象を拒否する() {
        let mut app = sample();
        app.core.model.elements.clear();
        let original = app.core.model.sections[1].clone();
        let reference = app
            .target_reference(ReferenceTarget::Section(original.id))
            .unwrap();
        assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteSection { id: SectionId(0) })));
        assert!(app.resolve_target_reference(&reference).is_err());
        app.undo_action();
        assert!(app.resolve_target_reference(&reference).is_err());
        let before_delete = app
            .target_reference(ReferenceTarget::Section(original.id))
            .unwrap();
        assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteSection { id: original.id })));
        app.core.model.sections.push(original);
        let replacement = app.target_reference(before_delete.target).unwrap();
        assert_ne!(before_delete.lifetime, replacement.lifetime);
        assert!(app.resolve_target_reference(&before_delete).is_err());
    }

    #[test]
    fn 非表示参照は明示表示まで選択とフィルタを変えない() {
        let mut app = sample();
        let reference = app
            .target_reference(ReferenceTarget::Member(ElemId(2)))
            .unwrap();
        app.ui.view.frame_target = Some(sepika_core::frame::FrameTarget::Story(
            sepika_core::ids::StoryId(0),
        ));
        let input = app.input_reference();
        assert!(app
            .show_reference(&reference, ReferenceView::Diagram, false)
            .unwrap_err()
            .contains("非表示"));
        assert_eq!(app.ui.scoped.selection, GeometrySelection::None);
        assert!(app.ui.view.frame_target.is_some());
        app.show_reference(&reference, ReferenceView::Diagram, true)
            .unwrap();
        assert!(app.ui.view.frame_target.is_none());
        assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(2)));
        assert_eq!(app.input_reference(), input);
    }

    #[test]
    fn 結果と固定参照の保存再読込でも生成版とrunを保持する() {
        let mut app = sample();
        let reference = calculate(&mut app);
        app.core
            .scoped
            .references
            .fixed_results
            .push(reference.clone());
        let path = std::env::temp_dir().join(format!(
            "sepika-result-reference-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            loaded.core.scoped.references.fixed_results,
            vec![reference.clone()]
        );
        assert!(loaded.resolve_result_reference(&reference).is_ok());
        assert!(loaded.result_input_snapshot(&reference).is_ok());
        assert!(loaded.core.scoped.job.is_none());
    }

    #[test]
    fn 非同期完了時の非計算編集で開始版を付け替えない() {
        let mut app = sample();
        let id = app.core.model.load_cases[0].id;
        app.start_eigen_job(2);
        let source = app
            .core
            .scoped
            .job
            .as_ref()
            .unwrap()
            .reference_source
            .clone();
        let axes = sepika_core::axis_gen::generate_axes(&app.core.model);
        assert!(app.apply_model_edit(Box::new(sepika_edit::ReplaceAxes { axes })));
        for _ in 0..200 {
            if app.poll_job() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (recorded, _) = app.calculation_snapshot(&ResultInputKey::Modal).unwrap();
        assert_eq!(recorded, source);
        assert_ne!(recorded.generation, app.input_reference().generation);
        assert_eq!(app.core.model.load_cases[0].id, id);
    }

    #[test]
    fn 同名組合せの係数変更は別の計算入力となる() {
        let mut app = sample();
        app.core
            .model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "確認".into(),
                terms: vec![(LoadCaseId(0), 1.0)],
            });
        app.run_combination(0);
        let target = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let reference = app
            .result_reference(
                target,
                ResultInputKey::Combo("確認".into()),
                DisplayConditions::default(),
            )
            .unwrap();
        app.core.model.combinations[0].terms[0].1 = 2.0;
        assert!(app.resolve_result_reference(&reference).is_err());
        app.run_combination(0);
        let target = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let changed = app
            .result_reference(
                target,
                ResultInputKey::Combo("確認".into()),
                DisplayConditions::default(),
            )
            .unwrap();
        assert_ne!(reference.calculation_input, changed.calculation_input);
        assert_ne!(reference.run, changed.run);
    }

    #[test]
    fn 画面参照パネルと過去入力窓の描画だけでは編集しない() {
        let mut app = sample();
        let reference = calculate(&mut app);
        app.select_member(ElemId(0));
        app.freeze_selected_references().unwrap();
        app.ui.scoped.reference_snapshot = Some((
            "計算用入力の確認".into(),
            app.recorded_result_input(&reference).unwrap(),
        ));
        let before = app.input_reference();
        let revision = app.core.scoped.undo.revision();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.reference_panel(ui);
            app.reference_snapshot_window(ui.ctx());
        });
        assert_eq!(app.input_reference(), before);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(app.resolve_result_reference(&reference).is_ok());
    }

    #[test]
    fn 固定結果の応力成分と階通りへ戻っても計算と履歴は変わらない() {
        let mut app = sample();
        let result = calculate(&mut app);
        app.ui.view.view_mode = crate::viewer::ViewMode::Force;
        app.ui.view.force_components = crate::viewer::ForceComponents::PRESET_N;
        let reference = app
            .result_reference(
                result.target.clone(),
                result.kind.clone(),
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        let input = app.input_reference();
        let before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
        let revision = app.core.scoped.undo.revision();
        app.ui.view.view_mode = crate::viewer::ViewMode::Shape;
        app.ui.view.force_components = crate::viewer::ForceComponents::PRESET_M;
        app.ui.view.frame_target = Some(sepika_core::frame::FrameTarget::Story(
            sepika_core::ids::StoryId(0),
        ));
        app.show_result_reference(&reference, ReferenceView::Diagram, false)
            .unwrap();
        assert_eq!(app.ui.view.view_mode, crate::viewer::ViewMode::Force);
        assert_eq!(
            app.ui.view.force_components,
            crate::viewer::ForceComponents::PRESET_N
        );
        assert!(app.ui.view.frame_target.is_none());
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            before
        );
        let mut missing_step = reference.clone();
        missing_step.display.step = Some(999);
        assert!(app
            .show_result_reference(&missing_step, ReferenceView::Diagram, false)
            .unwrap_err()
            .contains("ステップ"));
        let mut wrong_unit = reference;
        wrong_unit.display.unit = Some("N".into());
        assert!(app
            .show_result_reference(&wrong_unit, ReferenceView::Diagram, false)
            .unwrap_err()
            .contains("単位"));
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.scoped.undo.revision(), revision);
    }

    #[test]
    fn 同じ保存案から同数の独立編集をしても異なる入力を一致させない() {
        let app = sample();
        let path = std::env::temp_dir().join(format!(
            "sepika-branch-reference-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        let mut app = app;
        app.save_project_to(path.clone());
        let mut left = App::default();
        let mut right = App::default();
        left.open_project_from(path.clone());
        right.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(left.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [1.0, 0.0, 0.0]
        })));
        assert!(right.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [2.0, 0.0, 0.0]
        })));
        let reference = left
            .target_reference(ReferenceTarget::Node(NodeId(0)))
            .unwrap();
        assert_eq!(
            left.input_reference().generation,
            right.input_reference().generation
        );
        assert_eq!(
            left.input_reference().project,
            right.input_reference().project
        );
        assert_eq!(
            left.input_reference().proposal,
            right.input_reference().proposal
        );
        assert!(right
            .resolve_target_reference(&reference)
            .unwrap_err()
            .contains("入力版"));
    }

    #[test]
    fn 同世代の別波形は部材と振動対象と結果を取り違えない() {
        use sepika_solver::dynamic::timehistory::GroundMotion;
        let mut left = sample();
        calculate(&mut left);
        let mut right = sample();
        right.core.model = left.core.model.clone();
        right.core.scoped.references = left.saved_references();
        right.core.scoped.results = left.core.scoped.results.clone();
        let member = left
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        for (app, accel) in [(&mut left, 10.0), (&mut right, 20.0)] {
            let input = app
                .time_history_input(
                    GroundMotion {
                        dt: 0.02,
                        accel_x: vec![0.0, accel, -accel],
                        accel_y: None,
                        accel_theta: None,
                    },
                    "同名の波".into(),
                )
                .unwrap();
            app.accept_time_history_reference(&input.source, false);
            app.apply_time_history_result(
                Ok(super::super::tests::dummy_th(vec![0.0, 0.02])),
                input.source,
            );
        }
        assert_eq!(
            left.input_reference().generation,
            right.input_reference().generation
        );
        assert_ne!(left.input_reference().input, right.input_reference().input);
        assert!(right.resolve_target_reference(&member).is_err());
        let member = left
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        assert!(right.resolve_target_reference(&member).is_err());
        let target = left
            .target_reference(ReferenceTarget::Vibration(VibrationCaseId(0)))
            .unwrap();
        assert!(right
            .resolve_target_reference(&target)
            .unwrap_err()
            .contains("入力版"));
        let result = left
            .result_reference(
                member,
                ResultInputKey::TimeHistory(VibrationCaseId(0)),
                DisplayConditions::default(),
            )
            .unwrap();
        assert!(right.result_input_snapshot(&result).is_err());
        assert!(left.result_input_snapshot(&result).is_ok());
        let snapshot = left.recorded_result_input(&result).unwrap();
        assert!(matches!(
            snapshot.settings,
            RecordedSettings::Vibration { cfg: Some(_), .. }
        ));
        left.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .reference_vibration_settings
            .clear();
        let snapshot = left.recorded_result_input(&result).unwrap();
        assert!(matches!(
            snapshot.settings,
            RecordedSettings::Vibration { cfg: None, .. }
        ));
    }

    #[test]
    fn 同世代の別質点波形と領域名を取り違えない() {
        let mut left = sample();
        left.core
            .model
            .floor_regions
            .push(sepika_core::model::FloorRegion {
                id: sepika_core::ids::FloorRegionId(0),
                name: "元の領域".into(),
                boundary: Vec::new(),
                secondary_beams: Vec::new(),
                slab_ids: Vec::new(),
            });
        let mut right = sample();
        right.core.model = left.core.model.clone();
        right.core.scoped.references = left.core.scoped.references.clone();
        for (app, name) in [(&mut left, "左の領域"), (&mut right, "右の領域")] {
            assert!(
                app.apply_model_edit(Box::new(sepika_edit::SetFloorRegionName {
                    id: sepika_core::ids::FloorRegionId(0),
                    name: name.into()
                }))
            );
        }
        let member = left
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        assert_eq!(
            left.input_reference().generation,
            right.input_reference().generation
        );
        assert!(right
            .resolve_target_reference(&member)
            .unwrap_err()
            .contains("入力版"));
        right.core.model = left.core.model.clone();
        for (app, identity) in [(&mut left, "wave-A"), (&mut right, "wave-B")] {
            app.core.model.upsert_lumped_vibration_case(
                "同名".into(),
                crate::app::vibration::lumped_vibration_dir_from_seismic(SeismicDir::X),
                false,
                crate::app::vibration::lumped_vibration_dim_from_stick(
                    app.core.analysis_cfg.lumped_dim,
                ),
                Some(identity.into()),
            );
        }
        let target = left
            .target_reference(ReferenceTarget::LumpedVibration(LumpedVibrationCaseId(0)))
            .unwrap();
        assert!(right
            .resolve_target_reference(&target)
            .unwrap_err()
            .contains("存続"));
    }

    #[test]
    fn 結果の図入口は別の結果パネルから立体図へ戻る() {
        let mut app = sample();
        let result = calculate(&mut app);
        app.ui.view.view_mode = crate::viewer::ViewMode::Force;
        let reference = app
            .result_reference(
                result.target,
                result.kind,
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        app.ui.view.results_view = ResultsView::TimeHistory;
        app.show_result_reference(&reference, ReferenceView::Diagram, false)
            .unwrap();
        assert_eq!(
            app.active_workspace_content(),
            super::super::workspace::Content::Drawing
        );
        assert_eq!(app.ui.view.active_tab, Tab::Results);
        assert_eq!(app.ui.view.view_mode, crate::viewer::ViewMode::Force);
    }

    #[test]
    fn 対象詳細は残存部材と混ぜず未対応の表は無変更で拒否する() {
        let mut app = sample();
        app.select_member(ElemId(0));
        app.ui.scoped.nav.focus_section = Some(SectionId(0));
        let story = app
            .target_reference(ReferenceTarget::Story(sepika_core::ids::StoryId(0)))
            .unwrap();
        let revision = app.core.scoped.undo.revision();
        assert!(app
            .show_reference(&story, ReferenceView::Table, false)
            .unwrap_err()
            .contains("未対応"));
        assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(0)));
        assert_eq!(app.ui.scoped.nav.focus_section, Some(SectionId(0)));
        let material = app
            .target_reference(ReferenceTarget::Material(MaterialId(0)))
            .unwrap();
        app.show_reference(&material, ReferenceView::Detail, false)
            .unwrap();
        assert_eq!(app.ui.scoped.selection, GeometrySelection::None);
        assert!(app.ui.scoped.nav.focus_section.is_none());
        assert_eq!(app.ui.scoped.opened_reference, Some(material.clone()));
        let details = app.reference_target_description(material.target);
        assert!(details.contains(&app.core.model.materials[0].name));
        assert!(details.contains("密度="));
        assert_eq!(app.core.scoped.undo.revision(), revision);
    }

    #[test]
    fn 過去の解析検定設定を記録から開き現在値で補わない() {
        let mut app = sample();
        let result = calculate(&mut app);
        let old_c0 = app.core.analysis_cfg.c0;
        let old_damage = app.core.analysis_cfg.rc_damage_control;
        let old_bond = app.core.analysis_cfg.bond_method;
        let old_qd = app.core.analysis_cfg.qd_method;
        app.core.analysis_cfg.c0 = 0.91;
        app.core.analysis_cfg.rc_damage_control = !old_damage;
        let (_, analysis) = app.recorded_calculation_input(&result.kind).unwrap();
        let RecordedSettings::Analysis { cfg, .. } = analysis.settings else {
            panic!("解析条件が必要")
        };
        assert_eq!(cfg.c0, old_c0);
        let (_, design) = app
            .recorded_calculation_input(&ResultInputKey::Design)
            .unwrap();
        let RecordedSettings::Design {
            rc_damage_control,
            bond_method,
            qd_method,
            source,
            ..
        } = design.settings
        else {
            panic!("検定条件が必要")
        };
        assert_eq!(rc_damage_control, old_damage);
        assert_eq!(bond_method, old_bond);
        assert_eq!(qd_method, old_qd);
        assert!(source.is_some_and(|(_, input)| input.is_some()));
        assert_eq!(app.core.analysis_cfg.c0, 0.91);
    }

    #[test]
    fn 固定時刻歴stepは記録時刻へ戻り再生を停止する() {
        for lumped in [false, true] {
            let mut app = sample();
            calculate(&mut app);
            let times = vec![0.0, 0.27, 0.91];
            let kind = if lumped {
                let input = app.lumped_time_history_input(vec![0.0, 10.0, -10.0], "入力波".into());
                let mut result = super::super::tests::dummy_lumped(0.5, true);
                result.response.as_mut().unwrap().time = times.clone();
                app.apply_lumped_mass_result(Ok(result), Some(input.source));
                app.ui.view.view_mode = crate::viewer::ViewMode::LumpedTimeHistory;
                ResultInputKey::LumpedTimeHistory(LumpedVibrationCaseId(0))
            } else {
                let input = app
                    .time_history_input(
                        sepika_solver::dynamic::timehistory::GroundMotion {
                            dt: 0.02,
                            accel_x: vec![0.0, 10.0, -10.0],
                            accel_y: None,
                            accel_theta: None,
                        },
                        "入力波".into(),
                    )
                    .unwrap();
                let mut result = super::super::tests::dummy_th(times.clone());
                result.recording = Some(sepika_solver::dynamic::timehistory::ThRecording {
                    frame_time: times.clone(),
                    ..Default::default()
                });
                app.apply_time_history_result(Ok(result), input.source);
                app.ui.view.view_mode = crate::viewer::ViewMode::TimeHistory;
                ResultInputKey::TimeHistory(VibrationCaseId(0))
            };
            app.ui.view.th_frame = 1;
            let target = app
                .target_reference(ReferenceTarget::Member(ElemId(0)))
                .unwrap();
            let reference = app
                .result_reference(target, kind, app.current_display_conditions().unwrap())
                .unwrap();
            let before = app.input_reference();
            let revision = app.core.scoped.undo.revision();
            app.ui.view.th_frame = 2;
            app.ui.view.th_play_time = times[2];
            app.ui.view.th_playing = true;
            app.show_result_reference(&reference, ReferenceView::Diagram, false)
                .unwrap();
            assert_eq!(app.ui.view.th_frame, 1);
            assert_eq!(app.ui.view.th_play_time, times[1]);
            assert!(!app.ui.view.th_playing);
            assert_eq!(app.input_reference(), before);
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert!(app.resolve_result_reference(&reference).is_ok());
        }
    }

    fn raw_wave(value: f64) -> sepika_solver::dynamic::timehistory::GroundMotion {
        sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 0.02,
            accel_x: vec![0.0, value, -value],
            accel_y: None,
            accel_theta: None,
        }
    }

    #[test]
    fn 受理済み元波形と全解析設定は一般対象の入力版に含める() {
        let mut saved = sample();
        let path =
            std::env::temp_dir().join(format!("sepika-wave-branch-{}.ovika", uuid::Uuid::new_v4()));
        saved.save_project_to(path.clone());
        let mut left = App::default();
        let mut right = App::default();
        left.open_project_from(path.clone());
        right.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        for (app, value) in [(&mut left, 1.0), (&mut right, 2.0)] {
            let mut wave = raw_wave(10.0);
            wave.accel_y = Some(vec![0.0, value, -value]);
            let input = app.time_history_input(wave, "同名".into()).unwrap();
            app.accept_time_history_reference(&input.source, false);
        }
        let reference = left
            .target_reference(ReferenceTarget::Node(NodeId(0)))
            .unwrap();
        assert_eq!(
            left.input_reference().generation,
            right.input_reference().generation
        );
        assert!(right.resolve_target_reference(&reference).is_err());
        right.core.scoped.references = left.saved_references();
        right.core.analysis_cfg.th_record_every += 1;
        assert!(right.resolve_target_reference(&reference).is_err());
        right.core.analysis_cfg = left.core.analysis_cfg;
        for (app, value) in [(&mut left, 1.0), (&mut right, 2.0)] {
            let input = app.lumped_time_history_input(vec![0.0, value, -value], "同名".into());
            app.accept_time_history_reference(&input.source, true);
        }
        let reference = left
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        assert_eq!(
            left.input_reference().generation,
            right.input_reference().generation
        );
        assert!(right.resolve_target_reference(&reference).is_err());
    }

    #[test]
    fn 元波形を戻しても旧世代を再使用せず同波形再受理では世代を変えない() {
        for lumped in [false, true] {
            let mut app = sample();
            let mut versions = Vec::new();
            for value in [10.0, 20.0, 10.0, 10.0] {
                let source = if lumped {
                    app.lumped_time_history_input(vec![0.0, value, -value], "入力波".into())
                        .source
                } else {
                    app.time_history_input(raw_wave(value), "入力波".into())
                        .unwrap()
                        .source
                };
                app.accept_time_history_reference(&source, lumped);
                versions.push(app.input_reference());
            }
            assert!(versions[0].generation < versions[1].generation);
            assert!(versions[1].generation < versions[2].generation);
            assert_ne!(versions[0], versions[2]);
            assert_eq!(versions[2], versions[3]);
        }
    }

    #[test]
    fn 同期非同期の受理入力版は成功時にも維持し同波形ではrunだけ更新する() {
        for background in [false, true] {
            let mut app = sample();
            app.core.analysis_cfg.th_apply_long_term = false;
            app.core.analysis_cfg.th_duration = 0.04;
            app.core.analysis_cfg.th_dt = 0.02;
            let mut previous: Option<ResultReference> = None;
            for _ in 0..2 {
                let started = if background {
                    app.start_time_history_job(raw_wave(10.0));
                    let source = app
                        .core
                        .scoped
                        .job
                        .as_ref()
                        .expect("時刻歴が開始する")
                        .reference_source
                        .clone();
                    super::super::tests::wait_for_job(&mut app);
                    Some(source)
                } else {
                    app.run_time_history(raw_wave(10.0));
                    None
                };
                assert!(
                    app.core.scoped.last_error.is_none(),
                    "{:?}",
                    app.core.scoped.last_error
                );
                let target = app
                    .target_reference(ReferenceTarget::Member(ElemId(0)))
                    .unwrap();
                let result = app
                    .result_reference(
                        target,
                        ResultInputKey::TimeHistory(VibrationCaseId(0)),
                        DisplayConditions::default(),
                    )
                    .unwrap();
                assert_eq!(result.source, app.input_reference());
                if let Some(started) = started {
                    assert_eq!(result.source, started);
                }
                if let Some(previous) = previous {
                    assert_eq!(result.source, previous.source);
                    assert_ne!(result.run, previous.run);
                }
                previous = Some(result);
            }
        }
    }

    #[test]
    fn 結果を除外して保存しても受理済み波形と入力版を再読込する() {
        let mut app = sample();
        app.core.analysis_cfg.c0 = 0.33;
        for lumped in [false, true] {
            let source = if lumped {
                app.lumped_time_history_input(vec![0.0, 10.0, -10.0], "波".into())
                    .source
            } else {
                app.time_history_input(raw_wave(10.0), "波".into())
                    .unwrap()
                    .source
            };
            app.accept_time_history_reference(&source, lumped);
        }
        let reference = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let input = app.input_reference();
        let path = std::env::temp_dir().join(format!(
            "sepika-wave-identity-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.input_reference(), input);
        assert!(loaded.resolve_target_reference(&reference).is_ok());
        assert!(loaded.core.scoped.results.is_none());
        assert!(loaded.core.scoped.job.is_none());
        assert!(loaded
            .core
            .scoped
            .references
            .accepted_time_history
            .is_some());
        assert!(loaded
            .core
            .scoped
            .references
            .accepted_lumped_time_history
            .is_some());
    }

    #[test]
    fn 受理前の波形と設定拒否では入力版と結果を変えない() {
        let mut app = sample();
        calculate(&mut app);
        app.core.analysis_cfg.phase_diff_enabled = true;
        app.core.analysis_cfg.phase_diff_dir_y = true;
        let initial = app.input_reference();
        let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
        app.start_time_history_job(raw_wave(10.0));
        assert_eq!(app.input_reference(), initial);
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            results
        );
        app.core.analysis_cfg.phase_diff_enabled = false;
        app.core.analysis_cfg.phase_diff_dir_y = false;
        let input = app.time_history_input(raw_wave(10.0), "波".into()).unwrap();
        app.core.analysis_cfg.th_dt *= 2.0;
        let changed = app.input_reference();
        app.start_time_history_input_job(input);
        assert_eq!(app.input_reference(), changed);
        assert!(app.core.scoped.references.accepted_time_history.is_none());
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            results
        );
    }

    #[test]
    fn 受理後の計算失敗でも受理した元波形の入力版を残す() {
        let mut app = sample();
        app.core.analysis_cfg.th_apply_long_term = false;
        let before = app.input_reference();
        app.start_time_history_job(raw_wave(10.0));
        let accepted = app
            .core
            .scoped
            .job
            .as_ref()
            .unwrap()
            .reference_source
            .clone();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(JobResult::TimeHistory(
            Box::new(Err("計算失敗fixture".into())),
            Box::new(
                app.time_history_input(raw_wave(10.0), "波".into())
                    .unwrap()
                    .source,
            ),
        ))
        .unwrap();
        app.core.scoped.job.as_mut().unwrap().rx = rx;
        assert!(app.poll_job());
        assert_ne!(before, accepted);
        assert_eq!(app.input_reference(), accepted);
        assert!(app.core.scoped.references.accepted_time_history.is_some());
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .is_some_and(|reason| reason.contains("計算失敗fixture")));
    }

    #[test]
    fn 初回準備と再準備だけでは節点参照を失効させず人のundo_redoは旧版を拒否する() {
        let mut app = sample();
        let reference = app
            .target_reference(ReferenceTarget::Node(NodeId(0)))
            .unwrap();
        let before = app.input_reference();
        assert_eq!(app.core.model.load_cases.len(), 2);
        app.run_preparation();
        assert!(app.core.model.load_cases.len() > 2);
        assert_eq!(app.input_reference(), before);
        assert!(app.resolve_target_reference(&reference).is_ok());
        app.run_preparation();
        assert_eq!(app.input_reference(), before);
        app.undo_action();
        assert!(app.resolve_target_reference(&reference).is_err());
        let after_undo = app.input_reference().generation;
        app.redo_action();
        assert!(app.input_reference().generation > after_undo);
        assert!(app.resolve_target_reference(&reference).is_err());
    }

    #[test]
    fn 明示の空ケースと自動適用前のkindを準備の入力識別から落とさない() {
        use sepika_core::model::{LoadCase, LoadCaseKind, LoadCaseOrigin};
        let mut app = sample();
        app.core.model.load_cases.push(LoadCase {
            origin: LoadCaseOrigin::Explicit,
            id: LoadCaseId(2),
            name: "DL".into(),
            kind: LoadCaseKind::Other,
            nodal: Vec::new(),
            member: Vec::new(),
        });
        let before = app.input_reference();
        let reference = app
            .target_reference(ReferenceTarget::LoadCase(LoadCaseId(2)))
            .unwrap();
        app.run_preparation();
        assert_eq!(
            app.core.model.load_cases[2].origin,
            LoadCaseOrigin::ExplicitWithGeneratedKind(LoadCaseKind::Other)
        );
        assert_eq!(app.input_reference(), before);
        assert!(app.resolve_target_reference(&reference).is_ok());
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetLoadCaseKind {
            id: LoadCaseId(2),
            kind: LoadCaseKind::Live
        })));
        let changed = app.input_reference();
        assert_ne!(changed.input, before.input);
        app.run_preparation();
        assert_eq!(
            app.core.model.load_cases[2].origin,
            LoadCaseOrigin::ExplicitWithGeneratedKind(LoadCaseKind::Live)
        );
        assert_eq!(app.input_reference(), changed);
        assert!(app.apply_model_edit(Box::new(sepika_edit::AddLoadCase {
            name: "空の明示入力".into()
        })));
        assert_ne!(app.input_reference().input, changed.input);
    }

    #[test]
    fn 自動ケースの利用者編集は明示入力へ昇格し最後の手入力削除後も保持する() {
        use sepika_core::model::{LoadCaseOrigin, LoadSource, NodalLoad};
        let mut app = sample();
        app.run_preparation();
        let generated = app
            .core
            .model
            .load_cases
            .iter()
            .find(|case| case.name == "DL")
            .unwrap()
            .id;
        let before = app.input_reference();
        assert_eq!(
            app.core.model.load_cases[generated.index()].origin,
            LoadCaseOrigin::Generated
        );
        assert!(app.apply_model_edit(Box::new(sepika_edit::AddNodalLoad {
            lc: generated,
            load: NodalLoad {
                node: NodeId(2),
                values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                name: "明示".into(),
                source: LoadSource::Manual
            }
        })));
        assert_eq!(
            app.core.model.load_cases[generated.index()].origin,
            LoadCaseOrigin::Explicit
        );
        let index = app.core.model.load_cases[generated.index()]
            .nodal
            .iter()
            .position(|load| load.source == LoadSource::Manual)
            .unwrap();
        assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteNodalLoad {
            lc: generated,
            index
        })));
        assert_eq!(
            app.core.model.load_cases[generated.index()].origin,
            LoadCaseOrigin::Explicit
        );
        assert_ne!(app.input_reference().input, before.input);
        let explicit = app.input_reference();
        app.run_preparation();
        assert_eq!(app.input_reference(), explicit);
        assert!(app.apply_model_edit(Box::new(sepika_edit::SetLoadCaseName {
            id: generated,
            name: "利用者の固定荷重".into()
        })));
        let renamed = app.input_reference();
        app.run_preparation();
        assert_eq!(app.input_reference(), renamed);
        assert_ne!(renamed.input, explicit.input);
    }

    #[test]
    fn 自動case属性と手入力荷重が矛盾していても明示入力を識別から消さない() {
        use sepika_core::model::{LoadSource, NodalLoad};
        let mut app = sample();
        app.run_preparation();
        let index = app
            .core
            .model
            .load_cases
            .iter()
            .position(|case| case.name == "DL")
            .unwrap();
        let before = app.input_reference();
        app.core.model.load_cases[index].nodal.push(NodalLoad {
            node: NodeId(2),
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            name: "明示".into(),
            source: LoadSource::Manual,
        });
        assert_ne!(app.input_reference().input, before.input);
    }

    #[test]
    fn 設定を元へ戻しても旧版を再使用せず同フレーム開始版は確定済み設定となる() {
        let mut app = sample();
        let before = app.input_reference();
        let original = app.core.analysis_cfg;
        let mut changed = original;
        changed.c0 = 0.7;
        assert!(app.commit_analysis_settings(changed));
        let first = app.input_reference();
        assert!(!app.commit_analysis_settings(changed));
        assert_eq!(app.input_reference(), first);
        assert!(app.commit_analysis_settings(original));
        assert_ne!(app.input_reference(), before);
        assert!(app.input_reference().generation > first.generation);
        assert!(app.commit_analysis_settings(changed));
        app.start_eigen_job(2);
        let started = app
            .core
            .scoped
            .job
            .as_ref()
            .unwrap()
            .reference_source
            .clone();
        assert_eq!(started, app.input_reference());
        super::super::tests::wait_for_job(&mut app);
        assert_eq!(
            app.calculation_snapshot(&ResultInputKey::Modal).unwrap().0,
            started
        );
    }

    #[test]
    fn 派生仕口パネルを正規化しても手入力剛域と明示stb識別は保持する() {
        use sepika_core::model::{StbNodeIdentity, ZoneSource};
        let mut app = sample();
        app.core.model.elements[0].rigid_zone.source_i = ZoneSource::Manual;
        app.core.model.elements[0].rigid_zone.length_i = 120.0;
        app.core.model.stb_node_ids.push(StbNodeIdentity {
            node: NodeId(0),
            id: 100,
            guid: Some("12345678-1234-1234-1234-123456789abc".into()),
        });
        let input = app.input_reference();
        assert_eq!(app.core.model.stb_node_ids.len(), 1);
        assert_eq!(app.core.model.elements.len(), 3);
        app.run_preparation();
        assert_eq!(app.input_reference(), input);
        assert_eq!(app.core.model.elements[0].rigid_zone.length_i, 120.0);
        assert_eq!(app.core.model.stb_node_ids[0].id, 100);
        app.core.model.stb_node_ids[0].guid = Some("87654321-1234-1234-1234-123456789abc".into());
        assert_ne!(app.input_reference().input, input.input);
        app.core.model.stb_node_ids[0].guid = Some("12345678-1234-1234-1234-123456789abc".into());
        app.core.model.stb_node_ids[0].id = 200;
        assert_ne!(app.input_reference().input, input.input);
    }

    #[test]
    fn 質量方式を確定した後の準備は同じ入力版を維持する() {
        let mut app = sample();
        let mut cfg = app.core.analysis_cfg;
        cfg.mass_method = sepika_core::model::MassMethod::LumpedOnly;
        app.commit_analysis_settings(cfg);
        let input = app.input_reference();
        app.run_preparation();
        assert_eq!(app.core.model.mass_method, cfg.mass_method);
        assert_eq!(app.input_reference(), input);
    }

    #[test]
    fn 未準備の質量方式を保存再読込して固定参照と入力版を維持する() {
        let mut app = sample();
        calculate(&mut app);
        let mut cfg = app.core.analysis_cfg;
        cfg.mass_method = sepika_core::model::MassMethod::LumpedOnly;
        app.commit_analysis_settings(cfg);
        app.select_member(ElemId(0));
        app.freeze_selected_references().unwrap();
        let member = app.core.scoped.references.fixed[0].clone();
        let node = app
            .target_reference(ReferenceTarget::Node(NodeId(0)))
            .unwrap();
        let input = app.input_reference();
        let model = rmp_serde::to_vec_named(&app.core.model).unwrap();
        let revision = app.core.scoped.undo.revision();
        let path = std::env::temp_dir().join(format!(
            "sepika-pending-mass-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), model);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(app.input_reference(), input);
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.core.analysis_cfg.mass_method, cfg.mass_method);
        assert_eq!(loaded.core.model.mass_method, cfg.mass_method);
        assert_eq!(loaded.input_reference(), input);
        assert!(loaded.resolve_target_reference(&member).is_ok());
        assert!(loaded.resolve_target_reference(&node).is_ok());
        assert!(loaded.core.scoped.preparation.is_none());
        assert!(loaded.core.scoped.results.is_none());
        loaded.run_preparation();
        assert_eq!(loaded.input_reference(), input);
        assert!(loaded.resolve_target_reference(&member).is_ok());
        assert!(loaded.resolve_target_reference(&node).is_ok());
    }
    #[test]
    fn issue975_saved_spatial_and_lumped_reference_rebinds_the_destination_before_restoring_time() {
        for lumped in [false, true] {
            let mut app = sample();
            calculate(&mut app);
            let times = vec![0.0, 0.27, 0.91];
            for value in [1.0, 2.0] {
                if lumped {
                    let input = app
                        .lumped_time_history_input(vec![0.0, value, -value], format!("波{value}"));
                    let mut result = super::super::tests::dummy_lumped(value, true);
                    result.response.as_mut().unwrap().time = times.clone();
                    app.apply_lumped_mass_result(Ok(result), Some(input.source));
                } else {
                    let input = app
                        .time_history_input(raw_wave(value), format!("波{value}"))
                        .unwrap();
                    let mut result = super::super::tests::dummy_th(times.clone());
                    result.peak_disp = vec![[value; 6]; app.core.model.nodes.len()];
                    result.recording = Some(sepika_solver::dynamic::timehistory::ThRecording {
                        frame_time: times.clone(),
                        ..Default::default()
                    });
                    app.apply_time_history_result(Ok(result), input.source);
                }
            }
            app.ui.view.active_tab = Tab::Results;
            app.enter_workspace();
            app.ui.view.workspace.placement.arrangement =
                super::super::workspace::Arrangement::Horizontal;
            app.ui.view.workspace.placement.contents = [
                super::super::workspace::Content::Drawing,
                super::super::workspace::Content::Results,
            ];
            app.ui.view.results_view = if lumped {
                ResultsView::LumpedMass
            } else {
                ResultsView::TimeHistory
            };
            app.ui.view.view_mode = if lumped {
                crate::viewer::ViewMode::LumpedTimeHistory
            } else {
                crate::viewer::ViewMode::TimeHistory
            };
            app.ui.view.th_frame = 1;
            let kind = if lumped {
                ResultInputKey::LumpedTimeHistory(LumpedVibrationCaseId(1))
            } else {
                ResultInputKey::TimeHistory(VibrationCaseId(1))
            };
            let target = app
                .target_reference(ReferenceTarget::Member(ElemId(0)))
                .unwrap();
            let reference = app
                .result_reference(target, kind, app.current_display_conditions().unwrap())
                .unwrap();
            let mut previous = app.ui.view.viewport.clone();
            if lumped {
                previous.select_lumped_case(&app, LumpedVibrationCaseId(0));
            } else {
                previous.select_spatial_case(&app, VibrationCaseId(0));
            }
            app.ui.view.viewport = previous.clone();
            app.ui.view.workspace.secondary = previous;
            app.ui.view.th_playing = true;
            if lumped {
                let result = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .lumped_result_for(LumpedVibrationCaseId(0))
                    .unwrap()
                    .clone();
                app.set_lumped_mass_view(Some(LumpedVibrationCaseId(0)), &result);
            } else {
                let result = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .time_history_for(VibrationCaseId(0))
                    .unwrap()
                    .clone();
                app.set_spatial_time_history_view(VibrationCaseId(0), &result);
            }
            let before = app.input_reference();
            let results_before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
            let graph_kind = app.ui.view.results_view;
            app.show_result_reference(&reference, ReferenceView::Diagram, false)
                .unwrap();
            assert_eq!(app.ui.view.workspace.placement.active, 0);
            assert_eq!(
                app.active_workspace_content(),
                super::super::workspace::Content::Drawing
            );
            assert_eq!(app.ui.view.th_frame, 1);
            assert_eq!(app.ui.view.th_play_time, times[1]);
            assert!(!app.ui.view.th_playing);
            if lumped {
                assert_eq!(
                    app.ui
                        .view
                        .viewport
                        .lumped_result(&app)
                        .unwrap()
                        .modal
                        .period[0],
                    2.0
                );
                assert_eq!(
                    app.ui
                        .view
                        .workspace
                        .secondary
                        .lumped_result(&app)
                        .unwrap()
                        .modal
                        .period[0],
                    1.0
                );
            } else {
                assert_eq!(
                    app.ui.view.viewport.spatial_result(&app).unwrap().peak_disp[0][0],
                    2.0
                );
                assert_eq!(
                    app.ui
                        .view
                        .workspace
                        .secondary
                        .spatial_result(&app)
                        .unwrap()
                        .peak_disp[0][0],
                    1.0
                );
            }
            assert_eq!(app.input_reference(), before);
            assert_eq!(app.ui.view.results_view, graph_kind);
            assert_eq!(
                rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
                results_before
            );
        }
    }
    #[test]
    fn issue975_reference_diagram_preserves_the_adjacent_graph_kind_and_static_case() {
        let mut app = sample();
        let first = calculate(&mut app);
        let second_id = app.core.model.load_cases[1].id;
        app.run_linear_static(second_id);
        app.ui.view.view_mode = crate::viewer::ViewMode::Force;
        let target = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let reference = app
            .result_reference(
                target,
                ResultInputKey::Static(StaticCaseKey::User(second_id)),
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        let ResultInputKey::Static(first_key) = first.kind else {
            unreachable!()
        };
        app.select_displayed_result(StaticKey::Case(first_key));
        app.ui.view.active_tab = Tab::Results;
        app.enter_workspace();
        app.ui.view.workspace.placement.arrangement =
            super::super::workspace::Arrangement::Horizontal;
        app.ui.view.workspace.placement.contents = [
            super::super::workspace::Content::Drawing,
            super::super::workspace::Content::Results,
        ];
        app.ui.view.results_view = ResultsView::Pushover;
        app.ui.view.workspace.secondary.static_key = Some(StaticKey::Case(first_key));
        let before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
        app.show_result_reference(&reference, ReferenceView::Diagram, false)
            .unwrap();
        assert_eq!(app.ui.view.results_view, ResultsView::Pushover);
        assert_eq!(
            app.ui.view.workspace.placement.contents[1],
            super::super::workspace::Content::Results
        );
        assert_eq!(
            app.ui.view.viewport.static_key,
            Some(StaticKey::Case(StaticCaseKey::User(second_id)))
        );
        assert_eq!(
            app.ui.view.workspace.secondary.static_key,
            Some(StaticKey::Case(first_key))
        );
        assert_eq!(
            app.core.scoped.last_static,
            Some(StaticKey::Case(first_key))
        );
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            before
        );
    }
}

pub(crate) const QUANTITY_EVIDENCE_CONTRACT_VERSION: u32 = 1;

/// 保存結果または対象範囲を固定した数量入力を指す。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EvidenceReference {
    Calculation(Box<ResultReference>),
    Quantity(Box<QuantityEvidenceReference>),
}

/// 集計時に固定した入力と対象範囲。現在モデルで欠落を補完しない。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QuantityEvidenceReference {
    pub(crate) source: InputReference,
    pub(crate) model: sepika_core::model::Model,
    pub(crate) contract: u32,
    pub(crate) scope: QuantityEvidenceScope,
    pub(crate) identity: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum QuantityEvidenceScope {
    WholeModel,
    Members(Vec<ElemId>),
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

#[cfg(any(feature = "gui", test))]
impl QuantityEvidenceScope {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::WholeModel => "モデル全体".into(),
            Self::Members(ids) => format!("検討の固定対象 {} 部材", ids.len()),
        }
    }
}

impl QuantityEvidenceReference {
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn takeoff(&self) -> Result<sepika_design_jp::quantity::QuantityTakeoff, String> {
        let mut takeoff = sepika_design_jp::quantity::try_compute_quantity_takeoff(
            &self.model,
            &self.model.quantity_cfg,
        )?;
        if let QuantityEvidenceScope::Members(ids) = &self.scope {
            if ids.is_empty() {
                return Err("固定対象なし".into());
            }
            if ids.iter().any(|id| {
                self.model.element(*id).is_none()
                    || !takeoff.items.iter().any(|item| item.elem == Some(*id))
            }) {
                return Err("固定対象の数量明細が欠落しています".into());
            }
            takeoff
                .items
                .retain(|item| item.elem.is_some_and(|id| ids.contains(&id)));
        }
        Ok(takeoff)
    }
    pub(crate) fn identity(&self) -> String {
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

impl App {
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn member_quantity_evidence_reference(
        &self,
        mut members: Vec<ElemId>,
    ) -> QuantityEvidenceReference {
        members.sort_by_key(|id| id.0);
        members.dedup();
        let EvidenceReference::Quantity(mut quantity) = self.quantity_evidence_reference() else {
            unreachable!()
        };
        quantity.scope = QuantityEvidenceScope::Members(members);
        quantity.identity = quantity.identity();
        *quantity
    }

    /// 現在表示の入力とモデル全体の集計対象を同時に固定する。
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
}
