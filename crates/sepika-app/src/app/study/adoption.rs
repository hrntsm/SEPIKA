use super::*;
use sepika_edit::{CompositeCommand, EditCommand, SetElementSection};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Confirmation {
    pub proposal: usize,
    pub input: StudyInput,
    pub baseline: StudyInput,
    pub run: StudyRun,
    pub baseline_run: Option<StudyRun>,
    pub fixed: Vec<references::TargetReference>,
    pub case: Option<ResultInputKey>,
    pub node: u32,
    pub component: usize,
    pub envelope: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Decision {
    pub confirmation: Confirmation,
    pub accepted: bool,
    pub reason: String,
    pub created: SystemTime,
    pub applied: bool,
    pub adopted_input: Option<StudyInput>,
    #[serde(skip)]
    live_applied: Option<Arc<AtomicBool>>,
}
impl Decision {
    pub fn snapshot(&self) -> Self {
        let mut value = self.clone();
        value.applied = self.is_applied();
        value.live_applied = None;
        value
    }
    pub fn is_applied(&self) -> bool {
        self.live_applied
            .as_ref()
            .map_or(self.applied, |v| v.load(Ordering::SeqCst))
    }
}
pub(super) fn same_input(a: &StudyInput, b: &StudyInput) -> bool {
    let mut a_model = a.model.clone();
    let mut b_model = b.model.clone();
    a_model.mass_method = a.settings.analysis_cfg.mass_method;
    b_model.mass_method = b.settings.analysis_cfg.mass_method;
    for model in [&mut a_model, &mut b_model] {
        for case in &mut model.load_cases {
            case.nodal.retain(|load| !load.source.is_auto());
            case.member.retain(|load| !load.source.is_auto());
        }
    }
    if a_model.assign_stb_node_ids().is_err() || b_model.assign_stb_node_ids().is_err() {
        return false;
    }
    a.references.project == b.references.project
        && a.settings.same_input_conditions(&b.settings)
        && bincode::serialize(&a_model).expect("入力")
            == bincode::serialize(&b_model).expect("入力")
        && a.references.accepted_time_history == b.references.accepted_time_history
        && a.references.accepted_lumped_time_history == b.references.accepted_lumped_time_history
}

struct AdoptionCommand {
    command: Box<dyn EditCommand>,
    state: Arc<AtomicBool>,
    applied: bool,
    before: Vec<(ElemId, Option<SectionId>)>,
    after: Vec<(ElemId, Option<SectionId>)>,
}
struct RejectedAdoption(String);
impl EditCommand for RejectedAdoption {
    fn apply(&self, _model: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
        Box::new(sepika_edit::Noop)
    }
    fn label(&self) -> &str {
        "採用拒否"
    }
    fn is_noop(&self) -> bool {
        true
    }
    fn rejection(&self) -> Option<&str> {
        Some(&self.0)
    }
}
impl EditCommand for AdoptionCommand {
    fn committed(&self) {
        self.state.store(self.applied, Ordering::SeqCst);
    }
    fn label(&self) -> &str {
        "比較案の断面割当を採用"
    }
    fn apply(&self, model: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
        Box::new(Self {
            command: self.command.apply(model),
            state: self.state.clone(),
            applied: !self.applied,
            before: self.after.clone(),
            after: self.before.clone(),
        })
    }
    fn apply_in_transaction(
        &self,
        before: &sepika_core::model::Model,
        candidate: &mut sepika_core::model::Model,
    ) -> Box<dyn EditCommand> {
        if !self
            .before
            .iter()
            .all(|(id, section)| before.element(*id).is_some_and(|e| e.section == *section))
        {
            return Box::new(RejectedAdoption(
                "確認した割当と適用前の本案が異なります".into(),
            ));
        }
        let inverse = self.command.apply_in_transaction(before, candidate);
        if inverse.rejection().is_some() || inverse.is_noop() {
            return inverse;
        }
        if !self.after.iter().all(|(id, section)| {
            candidate
                .element(*id)
                .is_some_and(|e| e.section == *section)
        }) {
            return Box::new(RejectedAdoption(
                "断面変更の一部を適用できません。本案・履歴は未更新です".into(),
            ));
        }
        Box::new(Self {
            command: inverse,
            state: self.state.clone(),
            applied: !self.applied,
            before: self.after.clone(),
            after: self.before.clone(),
        })
    }
}
impl App {
    pub(super) fn main_study_input(&self) -> StudyInput {
        StudyInput::capture(if self.study_is_main() {
            &self.core
        } else {
            &self.studies.main.as_ref().expect("本案").core
        })
    }
    /// 最新の成功計算を確認用に固定する。古い入力・失敗・利用不可の結果は拒否する。
    pub fn confirm_study_run(
        &self,
        s: usize,
        p: usize,
        run: &StudyRun,
    ) -> Result<Confirmation, String> {
        let study = self.studies.items.get(s).ok_or("検討がありません")?;
        if p == 0 || p >= study.proposals.len() {
            return Err("比較案を選んでください".into());
        }
        let input = self.proposal_input(s, p);
        if input.references.proposal != run.proposal
            || !same_input(&input, &run.input)
            || input.references.generation != run.input.references.generation
        {
            return Err("比較案の最新計算が必要です。再表示して計算・確認してください".into());
        }
        if !study
            .runs
            .iter()
            .rev()
            .find(|r| r.proposal == run.proposal)
            .is_some_and(|latest| latest.created == run.created)
        {
            return Err("新しい計算があります。再表示して結果を確認してください".into());
        }
        if run.error.is_some()
            || run
                .results
                .as_ref()
                .is_none_or(|bundle| bundle.statics.is_empty() && bundle.combos.is_empty())
        {
            return Err("未計算または計算失敗です。計算と原因を確認してください".into());
        }
        let mut result = App {
            core: run.prepared.core(),
            ..Default::default()
        };
        result.core.scoped.results = run.results.clone();
        result.core.scoped.last_static = run.last_static;
        result.validate_restored_results();
        for case in &run.prepared.model.load_cases {
            let key = result
                .standard_lateral_case(case.id)
                .unwrap_or(StaticCaseKey::User(case.id));
            comparison::require_static_result(&result, &ResultInputKey::Static(key))?;
        }
        for combo in &run.prepared.model.combinations {
            comparison::require_static_result(&result, &ResultInputKey::Combo(combo.name.clone()))?;
        }
        Ok(Confirmation {
            proposal: p,
            input,
            baseline: study.baseline.clone(),
            run: run.clone(),
            baseline_run: study
                .runs
                .iter()
                .rev()
                .find(|r| {
                    r.proposal
                        == study.proposals[0].state.as_ref().map_or_else(
                            || self.core.scoped.references.proposal.clone(),
                            |v| v.core.scoped.references.proposal.clone(),
                        )
                })
                .cloned(),
            fixed: study.fixed.clone(),
            case: None,
            node: 0,
            component: 0,
            envelope: true,
        })
    }
    /// 確認済み入力を再照合し、採否と任意理由を保存する。採用は断面割当だけを一操作で適用する。
    pub fn decide_study(
        &mut self,
        s: usize,
        confirmation: Confirmation,
        accepted: bool,
        reason: String,
    ) -> Result<(), String> {
        let study = self.studies.items.get(s).ok_or("検討がありません")?;
        if confirmation.proposal == 0 || confirmation.proposal >= study.proposals.len() {
            return Err("比較案がありません。再表示してください".into());
        }
        if !same_input(&confirmation.baseline, &study.baseline) || confirmation.fixed != study.fixed
        {
            return Err("検討の基準・固定対象が変わりました。再表示してください".into());
        }
        let current = self.proposal_input(s, confirmation.proposal);
        if current.references.proposal != confirmation.input.references.proposal
            || current.references.generation != confirmation.input.references.generation
            || !same_input(&current, &confirmation.input)
        {
            return Err(
                "比較案が確認中に変更されました。再表示・再計算・再確認してください".into(),
            );
        }
        self.confirm_study_run(s, confirmation.proposal, &confirmation.run)?;
        if accepted
            && study.decisions.iter().any(|d| {
                d.accepted
                    && d.is_applied()
                    && d.confirmation.input.references.proposal
                        == confirmation.input.references.proposal
                    && same_input(&d.confirmation.input, &confirmation.input)
                    && d.adopted_input
                        .as_ref()
                        .is_some_and(|input| same_input(input, &self.main_study_input()))
            })
        {
            return Err("この変更は採用済みです".into());
        }
        let main = self.main_study_input();
        if accepted && !same_input(&main, &study.baseline) {
            return Err(
                "本案が基準案と異なります。本案を表示して新しい基準で検討を作成してください".into(),
            );
        }
        let mut children: Vec<Box<dyn EditCommand>> = Vec::new();
        for target in &study.fixed {
            self.resolve_study_fixed_target(s, target)?;
            let references::ReferenceTarget::Member(id) = target.target else {
                return Err("固定対象が大梁ではありません".into());
            };
            let before = study
                .baseline
                .model
                .element(id)
                .ok_or("固定対象がありません")?;
            let after = current
                .model
                .element(id)
                .ok_or("比較案の固定対象がありません")?;
            for element in [before, after] {
                if element.kind != sepika_core::model::ElementKind::Beam
                    || !element
                        .section
                        .and_then(|id| current.model.section(id))
                        .is_some_and(|section| {
                            sepika_edit::valid_study_section(&current.model, section)
                        })
                {
                    return Err("対象または割当断面が無効です".into());
                }
            }
            if before.section != after.section {
                children.push(Box::new(SetElementSection {
                    elem: id,
                    section: after.section,
                }));
            }
        }
        let fixed: Vec<_> = study
            .fixed
            .iter()
            .filter_map(|r| {
                if let references::ReferenceTarget::Member(id) = r.target {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        let mut candidate = current.clone();
        for id in &fixed {
            candidate
                .model
                .elements
                .iter_mut()
                .find(|e| e.id == *id)
                .ok_or("対象なし")?
                .section = study.baseline.model.element(*id).ok_or("対象なし")?.section;
        }
        if !same_input(&candidate, &study.baseline) {
            return Err("固定対象の断面割当以外の変更があります".into());
        }
        if children.is_empty() {
            return Err("断面変更がありません。編集と採用記録は追加しません".into());
        }
        let state = Arc::new(AtomicBool::new(false));
        let mut adopted_input = None;
        if accepted {
            self.switch_study(Location::Main)?;
            self.confirm_study_run(s, confirmation.proposal, &confirmation.run)?;
            if !same_input(&self.main_study_input(), &confirmation.baseline)
                || !same_input(
                    &self.proposal_input(s, confirmation.proposal),
                    &confirmation.input,
                )
            {
                return Err("入力確定により案が変わりました。再表示・再確認してください".into());
            }
            let before = fixed
                .iter()
                .map(|id| {
                    (
                        *id,
                        confirmation
                            .baseline
                            .model
                            .element(*id)
                            .expect("固定対象")
                            .section,
                    )
                })
                .collect();
            let after = fixed
                .iter()
                .map(|id| {
                    (
                        *id,
                        confirmation
                            .input
                            .model
                            .element(*id)
                            .expect("固定対象")
                            .section,
                    )
                })
                .collect();
            let command = AdoptionCommand {
                command: Box::new(CompositeCommand {
                    label: "比較案の断面割当を採用".into(),
                    children,
                }),
                state: state.clone(),
                applied: true,
                before,
                after,
            };
            #[cfg(feature = "gui")]
            let applied = self.apply_model_edit(Box::new(command));
            #[cfg(not(feature = "gui"))]
            let applied = {
                let applied = self
                    .core
                    .scoped
                    .undo
                    .run(&mut self.core.model, Box::new(command));
                if applied {
                    self.core.scoped.staleness.mark_edited();
                }
                applied
            };
            if !applied {
                return Err(self
                    .core
                    .scoped
                    .undo
                    .last_error()
                    .unwrap_or("採用できませんでした。本案・履歴は未更新です")
                    .into());
            }
            adopted_input = Some(StudyInput::capture(&self.core));
        }
        self.studies.items[s].decisions.push(Decision {
            confirmation,
            accepted,
            reason,
            created: SystemTime::now(),
            applied: accepted,
            adopted_input,
            live_applied: accepted.then_some(state),
        });
        self.mark_study_dirty();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{comparison::tests::computed, tests::assign};
    use super::*;
    fn confirmation(app: &App) -> Confirmation {
        let run = app.studies.items[0]
            .runs
            .iter()
            .rev()
            .find(|r| r.proposal == app.proposal_input(0, 1).references.proposal)
            .unwrap();
        app.confirm_study_run(0, 1, run).unwrap()
    }
    fn undo(app: &mut App) {
        #[cfg(feature = "gui")]
        app.undo_action();
        #[cfg(not(feature = "gui"))]
        {
            app.core.scoped.undo.undo(&mut app.core.model);
            app.core.scoped.staleness.mark_edited();
        }
    }
    fn redo(app: &mut App) {
        #[cfg(feature = "gui")]
        app.redo_action();
        #[cfg(not(feature = "gui"))]
        {
            app.core.scoped.undo.redo(&mut app.core.model);
            app.core.scoped.staleness.mark_edited();
        }
    }
    #[test]
    fn adoption_undo_following_edit_redo_and_saved_evidence_remain_independent_of_proposal() {
        let mut app = computed();
        let confirmed = confirmation(&app);
        let evidence = bincode::serialize(&confirmed.run).unwrap();
        app.decide_study(0, confirmed, true, "梁成を確認".into())
            .unwrap();
        assert_eq!(
            app.core.model.element(ElemId(2)).unwrap().section,
            Some(SectionId(2))
        );
        assert!(app.core.scoped.staleness.results_stale);
        assert!(app.core.scoped.results.is_none());
        assert!(app.studies.items[0].decisions[0].is_applied());
        assert!(assign(&mut app, 1));
        undo(&mut app);
        assert!(app.studies.items[0].decisions[0].is_applied());
        undo(&mut app);
        assert!(!app.studies.items[0].decisions[0].is_applied());
        assert!(!app.study_main_changed(0));
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 1));
        app.switch_study(Location::Main).unwrap();
        redo(&mut app);
        assert_eq!(
            app.core.model.element(ElemId(2)).unwrap().section,
            Some(SectionId(2))
        );
        assert!(app.studies.items[0].decisions[0].is_applied());
        assert_eq!(
            evidence,
            bincode::serialize(&app.studies.items[0].decisions[0].confirmation.run).unwrap()
        );
        let bytes = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let mut restored = App::default();
        restored
            .restore_studies(rmp_serde::from_slice(&bytes).unwrap())
            .unwrap();
        let decision = &restored.studies.items[0].decisions[0];
        assert!(decision.is_applied());
        assert_eq!(decision.reason, "梁成を確認");
        assert_eq!(
            evidence,
            bincode::serialize(&decision.confirmation.run).unwrap()
        );
    }
    #[test]
    fn changed_main_or_proposal_and_failed_or_old_calculations_do_not_apply() {
        for condition in [
            "main",
            "main_section",
            "proposal",
            "failed",
            "stale",
            "outside",
            "invalid",
        ] {
            let mut app = computed();
            let mut confirmed = confirmation(&app);
            match condition {
                "main" => {
                    app.studies.main.as_mut().unwrap().core.model.sections[1]
                        .name
                        .push('x');
                }
                "main_section" => {
                    app.studies.main.as_mut().unwrap().core.model.elements[2].section =
                        Some(SectionId(2));
                }
                "proposal" => {
                    assert!(assign(&mut app, 1));
                }
                "failed" => {
                    confirmed.run.error = Some("計算失敗".into());
                }
                "stale" => {
                    confirmed.run.input.references.generation += 1;
                }
                "outside" => {
                    app.core.model.elements[0].section = Some(SectionId(2));
                    confirmed.input.model = app.core.model.clone();
                    confirmed.run.input.model = app.core.model.clone();
                }
                "invalid" => {
                    confirmed.fixed[0].lifetime.push('x');
                }
                _ => unreachable!(),
            }
            let before = bincode::serialize(&app.main_study_input().model).unwrap();
            let result = app.decide_study(0, confirmed, true, String::new());
            assert!(result.is_err(), "{condition}");
            assert_eq!(
                before,
                bincode::serialize(&app.main_study_input().model).unwrap(),
                "{condition}"
            );
            assert!(app.studies.items[0].decisions.is_empty());
        }
    }
    #[test]
    fn save_reload_preserves_main_baseline_equality_and_adoption_cancellation() {
        let mut app = computed();
        app.switch_study(Location::Main).unwrap();
        let path =
            std::env::temp_dir().join(format!("sepika-adoption-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert!(!loaded.study_main_changed(0));
        let confirmed = confirmation(&loaded);
        loaded
            .decide_study(0, confirmed, true, String::new())
            .unwrap();
        undo(&mut loaded);
        loaded.save_project_to(path.clone());
        let mut restored = App::default();
        restored.open_project_from(path.clone());
        assert!(!restored.studies.items[0].decisions[0].is_applied());
        assert!(!restored.study_main_changed(0));
        assert!(!restored.core.scoped.undo.can_undo());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn main_equality_includes_dynamic_inputs_and_non_static_settings_but_ignores_display() {
        use sepika_core::model::{
            LumpedVibrationCase, LumpedVibrationDim, LumpedVibrationDir, VibrationCase,
            VibrationThDir,
        };
        let mut app = super::super::tests::app();
        app.create_study("入力照合".into(), vec![ElemId(2)])
            .unwrap();
        let baseline = app.core.model.clone();
        app.core.model.vibration_cases.push(VibrationCase {
            id: sepika_core::ids::VibrationCaseId(0),
            name: "動的".into(),
            wave_name: "波形".into(),
            input_identity: Some("波形条件".into()),
            dir: VibrationThDir::X,
            nonlinear: false,
        });
        assert!(app.study_main_changed(0));
        app.core.model = baseline.clone();
        app.core
            .model
            .lumped_vibration_cases
            .push(LumpedVibrationCase {
                id: sepika_core::ids::LumpedVibrationCaseId(0),
                name: "質点系".into(),
                wave_name: "波形".into(),
                input_identity: Some("条件".into()),
                dir: LumpedVibrationDir::Y,
                nonlinear: false,
                dim: LumpedVibrationDim::Planar,
            });
        assert!(app.study_main_changed(0));
        app.core.model = baseline;
        let ultimate_rp = app.core.ultimate_rp;
        app.core.ultimate_rp += 0.1;
        assert!(app.study_main_changed(0));
        app.core.ultimate_rp = ultimate_rp;
        app.ui
            .scoped
            .selection
            .select_members(vec![ElemId(0)], Some(ElemId(0)));
        assert!(!app.study_main_changed(0));
    }
    #[test]
    fn study_save_retains_main_dynamic_case_inputs_without_restoring_any_result() {
        use sepika_core::model::{
            LumpedVibrationCase, LumpedVibrationDim, LumpedVibrationDir, VibrationCase,
            VibrationThDir,
        };
        let mut app = super::super::tests::app();
        app.core.model.vibration_cases.push(VibrationCase {
            id: sepika_core::ids::VibrationCaseId(0),
            name: "動的入力".into(),
            wave_name: "波形".into(),
            input_identity: Some("条件".into()),
            dir: VibrationThDir::Xy,
            nonlinear: true,
        });
        app.core
            .model
            .lumped_vibration_cases
            .push(LumpedVibrationCase {
                id: sepika_core::ids::LumpedVibrationCaseId(0),
                name: "質点系入力".into(),
                wave_name: "波形".into(),
                input_identity: Some("条件".into()),
                dir: LumpedVibrationDir::Y,
                nonlinear: true,
                dim: LumpedVibrationDim::Spatial,
            });
        app.create_study("動的入力を含む本案".into(), vec![ElemId(2)])
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "sepika-adoption-dynamic-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert!(!loaded.study_main_changed(0));
        assert_eq!(
            loaded.core.model.vibration_cases,
            app.core.model.vibration_cases
        );
        assert_eq!(
            loaded.core.model.lumped_vibration_cases,
            app.core.model.lumped_vibration_cases
        );
        assert!(loaded.core.scoped.results.is_none());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn all_nondefault_design_settings_roundtrip_and_still_allow_adoption() {
        use sepika_design_jp::secondary::holding_capacity::{FrameType, MemberRank};
        let mut app = super::super::tests::app();
        app.core.design_frame = FrameType::SteelBrace;
        app.core.design_rank = MemberRank::FD;
        app.core.design_rank_auto = !app.core.design_rank_auto;
        app.core.wall_structure = !app.core.wall_structure;
        app.core.ultimate_rp = 0.1;
        app.core.ultimate_lightweight = !app.core.ultimate_lightweight;
        app.core.ultimate_include_bond = !app.core.ultimate_include_bond;
        app.core.ultimate_upper_factor = 1.7;
        app.core.ultimate_shear_ductility = !app.core.ultimate_shear_ductility;
        app.core.ultimate_biaxial_shear = !app.core.ultimate_biaxial_shear;
        app.core.ultimate_biaxial_bending = !app.core.ultimate_biaxial_bending;
        app.core.ultimate_use_pushover = !app.core.ultimate_use_pushover;
        let settings = bincode::serialize(&StudySettings::capture(&app.core)).unwrap();
        app.create_study("非既定の検定条件".into(), vec![ElemId(2)])
            .unwrap();
        app.create_comparison(0, "案A".into()).unwrap();
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 2));
        app.start_study_calculation(0, 1).unwrap();
        super::super::tests::wait(&mut app);
        app.switch_study(Location::Main).unwrap();
        let path = std::env::temp_dir().join(format!(
            "sepika-adoption-settings-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(
            settings,
            bincode::serialize(&StudySettings::capture(&loaded.core)).unwrap()
        );
        assert!(!loaded.study_main_changed(0));
        let confirmed = confirmation(&loaded);
        loaded
            .decide_study(0, confirmed, true, String::new())
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn selecting_long_and_short_results_changes_display_term_without_blocking_adoption() {
        let mut app = super::super::tests::app();
        app.core
            .model
            .load_cases
            .push(sepika_core::model::LoadCase {
                origin: Default::default(),
                id: LoadCaseId(2),
                name: "積載".into(),
                kind: sepika_core::model::LoadCaseKind::Live,
                nodal: vec![],
                member: vec![],
            });
        for (name, terms) in [
            ("長期", vec![(LoadCaseId(0), 1.0), (LoadCaseId(2), 1.0)]),
            (
                "短期",
                vec![
                    (LoadCaseId(0), 1.0),
                    (LoadCaseId(2), 1.0),
                    (LoadCaseId(1), 1.0),
                ],
            ),
        ] {
            app.core
                .model
                .combinations
                .push(sepika_core::model::LoadCombination {
                    name: name.into(),
                    terms,
                });
        }
        app.run_static_all();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.select_displayed_result(StaticKey::Combo(0));
        assert_eq!(app.core.design_term, LoadTerm::Long);
        app.create_study("表示の切替".into(), vec![ElemId(2)])
            .unwrap();
        app.create_comparison(0, "案A".into()).unwrap();
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 2));
        app.start_study_calculation(0, 1).unwrap();
        super::super::tests::wait(&mut app);
        app.switch_study(Location::Main).unwrap();
        app.select_displayed_result(StaticKey::Combo(1));
        assert_eq!(app.core.design_term, LoadTerm::Short);
        assert!(!app.study_main_changed(0));
        app.core.model.combinations[1].terms[0].1 = 0.5;
        assert!(app.study_main_changed(0));
        app.core.model.combinations[1].terms[0].1 = 1.0;
        let confirmed = confirmation(&app);
        app.decide_study(0, confirmed, true, String::new()).unwrap();
    }
    #[test]
    fn pending_mass_method_save_reload_does_not_change_main_baseline_match() {
        let mut app = super::super::tests::app();
        app.core.analysis_cfg.mass_method = sepika_core::model::MassMethod::LumpedOnly;
        app.create_study("質量方式".into(), vec![ElemId(2)])
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "sepika-adoption-mass-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert!(!loaded.study_main_changed(0));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "親エージェントの実機検証用fixtureを明示生成する"]
    fn export_adoption_gui_fixture() {
        let mut app = computed();
        app.switch_study(Location::Main).unwrap();
        let path = std::path::PathBuf::from(
            std::env::var_os("SEPIKA_ADOPTION_FIXTURE").expect("専用fixtureパス"),
        );
        app.save_project_to(path);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
    #[test]
    fn partial_assignment_failure_preserves_input_history_and_application_state() {
        let mut app = super::super::tests::app();
        assert!(assign(&mut app, 2));
        undo(&mut app);
        assert!(app.core.scoped.undo.can_redo());
        let before = bincode::serialize(&app.core.model).unwrap();
        let state = Arc::new(AtomicBool::new(false));
        let command = AdoptionCommand {
            command: Box::new(CompositeCommand {
                label: "採用".into(),
                children: vec![
                    Box::new(SetElementSection {
                        elem: ElemId(2),
                        section: Some(SectionId(2)),
                    }),
                    Box::new(SetElementSection {
                        elem: ElemId(0),
                        section: Some(SectionId(999)),
                    }),
                ],
            }),
            state: state.clone(),
            applied: true,
            before: vec![
                (ElemId(2), Some(SectionId(1))),
                (ElemId(0), Some(SectionId(0))),
            ],
            after: vec![
                (ElemId(2), Some(SectionId(2))),
                (ElemId(0), Some(SectionId(999))),
            ],
        };
        assert!(!app
            .core
            .scoped
            .undo
            .run(&mut app.core.model, Box::new(command)));
        assert_eq!(before, bincode::serialize(&app.core.model).unwrap());
        assert!(!app.core.scoped.undo.can_undo());
        assert!(!state.load(Ordering::SeqCst));
        assert!(app.core.scoped.undo.can_redo());
    }
    #[test]
    fn unchanged_or_duplicate_adoption_does_not_add_edit_or_decision() {
        let mut app = computed();
        let proposal = app.proposal_input(0, 2);
        let run = app.studies.items[0]
            .runs
            .iter()
            .find(|r| r.proposal == proposal.references.proposal)
            .unwrap();
        let unchanged = app.confirm_study_run(0, 2, run).unwrap();
        assert!(app.decide_study(0, unchanged, true, String::new()).is_err());
        assert!(app.studies.items[0].decisions.is_empty());
        let confirmed = confirmation(&app);
        app.decide_study(0, confirmed.clone(), true, String::new())
            .unwrap();
        let revision = app.core.scoped.undo.revision();
        assert!(app.decide_study(0, confirmed, true, String::new()).is_err());
        assert_eq!(revision, app.core.scoped.undo.revision());
        assert_eq!(app.studies.items[0].decisions.len(), 1);
    }
    #[test]
    fn missing_unselected_static_or_combination_result_is_rejected_without_any_edit_or_record() {
        for remove_combo in [false, true] {
            let mut app = computed();
            let mut confirmed = confirmation(&app);
            let bundle = confirmed.run.results.as_mut().unwrap();
            if remove_combo {
                assert!(!bundle.combos.is_empty());
                bundle.combos.remove(0);
            } else {
                let index = bundle
                    .statics
                    .iter()
                    .position(|(key, _)| confirmed.run.last_static != Some(StaticKey::Case(*key)))
                    .expect("非表示ケース");
                bundle.statics.remove(index);
            }
            assert!(!bundle.input_records.is_empty());
            let main_before = bincode::serialize(&app.main_study_input()).unwrap();
            let undo_before = app
                .studies
                .main
                .as_ref()
                .unwrap()
                .core
                .scoped
                .undo
                .revision();
            assert!(app.confirm_study_run(0, 1, &confirmed.run).is_err());
            assert!(app.decide_study(0, confirmed, true, String::new()).is_err());
            assert_eq!(
                main_before,
                bincode::serialize(&app.main_study_input()).unwrap()
            );
            let undo = &app.studies.main.as_ref().unwrap().core.scoped.undo;
            assert_eq!(undo_before, undo.revision());
            assert!(!undo.can_undo());
            assert!(!undo.can_redo());
            assert!(app.studies.items[0].decisions.is_empty());
        }
    }
    #[test]
    fn uncalculated_results_and_newer_unreviewed_runs_require_calculation_or_redisplay() {
        let mut app = computed();
        let run = confirmation(&app).run;
        let mut absent = run.clone();
        absent.results = None;
        assert!(app.confirm_study_run(0, 1, &absent).is_err());
        let mut latest = run.clone();
        latest.created += std::time::Duration::from_secs(1);
        app.studies.items[0].runs.push(latest);
        assert!(app.confirm_study_run(0, 1, &run).is_err());
        app.switch_study(Location::Proposal(0, 1)).unwrap();
        assert!(assign(&mut app, 1));
        assert!(app.confirm_study_run(0, 1, &run).is_err());
    }
    #[test]
    fn rejection_is_saved_with_immutable_input_and_no_main_edit() {
        let mut app = computed();
        let confirmed = confirmation(&app);
        let main = bincode::serialize(&app.main_study_input().model).unwrap();
        app.decide_study(0, confirmed, false, "変位を再検討".into())
            .unwrap();
        assert_eq!(
            main,
            bincode::serialize(&app.main_study_input().model).unwrap()
        );
        let bytes = rmp_serde::to_vec_named(&app.saved_studies()).unwrap();
        let mut restored = App::default();
        restored
            .restore_studies(rmp_serde::from_slice(&bytes).unwrap())
            .unwrap();
        let decision = &restored.studies.items[0].decisions[0];
        assert!(!decision.accepted);
        assert_eq!(decision.reason, "変位を再検討");
        assert!(decision.confirmation.run.results.is_some());
    }
}
