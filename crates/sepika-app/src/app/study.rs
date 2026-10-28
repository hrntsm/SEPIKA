use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StudySettings {
    pub design_term: LoadTerm,
    pub design_frame: sepika_design_jp::secondary::holding_capacity::FrameType,
    pub design_rank: sepika_design_jp::secondary::holding_capacity::MemberRank,
    pub design_rank_auto: bool,
    pub wall_structure: bool,
    pub ultimate_rp: f64,
    pub ultimate_lightweight: bool,
    pub ultimate_include_bond: bool,
    pub ultimate_upper_factor: f64,
    pub ultimate_shear_ductility: bool,
    pub ultimate_biaxial_shear: bool,
    pub ultimate_biaxial_bending: bool,
    pub ultimate_use_pushover: bool,
    pub analysis_cfg: AnalysisSettings,
}
impl StudySettings {
    fn capture(core: &AppCore) -> Self {
        Self {
            design_term: core.design_term,
            design_frame: core.design_frame,
            design_rank: core.design_rank,
            design_rank_auto: core.design_rank_auto,
            wall_structure: core.wall_structure,
            ultimate_rp: core.ultimate_rp,
            ultimate_lightweight: core.ultimate_lightweight,
            ultimate_include_bond: core.ultimate_include_bond,
            ultimate_upper_factor: core.ultimate_upper_factor,
            ultimate_shear_ductility: core.ultimate_shear_ductility,
            ultimate_biaxial_shear: core.ultimate_biaxial_shear,
            ultimate_biaxial_bending: core.ultimate_biaxial_bending,
            ultimate_use_pushover: core.ultimate_use_pushover,
            analysis_cfg: core.analysis_cfg,
        }
    }
    fn apply(&self, core: &mut AppCore) {
        core.design_term = self.design_term;
        core.design_frame = self.design_frame;
        core.design_rank = self.design_rank;
        core.design_rank_auto = self.design_rank_auto;
        core.wall_structure = self.wall_structure;
        core.ultimate_rp = self.ultimate_rp;
        core.ultimate_lightweight = self.ultimate_lightweight;
        core.ultimate_include_bond = self.ultimate_include_bond;
        core.ultimate_upper_factor = self.ultimate_upper_factor;
        core.ultimate_shear_ductility = self.ultimate_shear_ductility;
        core.ultimate_biaxial_shear = self.ultimate_biaxial_shear;
        core.ultimate_biaxial_bending = self.ultimate_biaxial_bending;
        core.ultimate_use_pushover = self.ultimate_use_pushover;
        core.analysis_cfg = self.analysis_cfg;
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    #[default]
    Main,
    Proposal(usize, usize),
    History(usize, usize),
    StartedInput(usize, usize),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StudyInput {
    pub model: sepika_core::model::Model,
    pub settings: StudySettings,
    pub references: references::ProjectReferences,
}
impl StudyInput {
    fn capture(core: &AppCore) -> Self {
        let mut references = core.scoped.references.clone();
        references.generation +=
            core.scoped.undo.revision() - core.scoped.reference_preparation_revisions;
        references.saved_input = None;
        Self {
            model: core.model.clone(),
            settings: StudySettings::capture(core),
            references,
        }
    }
    fn core(&self) -> AppCore {
        let mut core = AppCore {
            model: self.model.clone(),
            ..Default::default()
        };
        self.settings.apply(&mut core);
        core.scoped.references = self.references.clone();
        core
    }
}

pub struct Slot {
    pub core: AppCore,
    pub ui: UiModelScoped,
}
impl Slot {
    fn new(input: &StudyInput) -> Self {
        Self {
            core: input.core(),
            ui: UiModelScoped::default(),
        }
    }
}

pub struct Proposal {
    pub name: String,
    pub state: Option<Slot>,
}
pub struct Study {
    pub purpose: String,
    pub notes: String,
    pub fixed: Vec<references::TargetReference>,
    pub baseline: StudyInput,
    pub proposals: Vec<Proposal>,
    pub runs: Vec<StudyRun>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReferencedResult {
    pub input: StudyInput,
    pub bundle: ResultsBundle,
    pub last_static: Option<StaticKey>,
}
fn bundle_contains_reference(
    bundle: &ResultsBundle,
    reference: &references::ResultReference,
) -> bool {
    bundle
        .reference_runs
        .iter()
        .any(|(key, id)| *key == reference.kind && *id == reference.run)
        && bundle
            .reference_sources
            .iter()
            .any(|(key, source)| *key == reference.kind && *source == reference.source)
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ViewReferences {
    pub fixed: Vec<references::TargetReference>,
    pub results: Vec<references::ResultReference>,
    pub evidence: Vec<ReferencedResult>,
}
impl ViewReferences {
    fn capture(core: &AppCore, previous: &Self, original: Option<&ResultsBundle>) -> Self {
        let mut captured = Self {
            fixed: core.scoped.references.fixed.clone(),
            results: core.scoped.references.fixed_results.clone(),
            evidence: previous
                .evidence
                .iter()
                .filter(|e| {
                    core.scoped
                        .references
                        .fixed_results
                        .iter()
                        .any(|r| bundle_contains_reference(&e.bundle, r))
                })
                .cloned()
                .collect(),
        };
        if let Some(bundle) = &core.scoped.results {
            let needed = captured.results.iter().any(|r| {
                r.kind == ResultInputKey::Design
                    && bundle_contains_reference(bundle, r)
                    && !original.is_some_and(|b| bundle_contains_reference(b, r))
                    && !captured
                        .evidence
                        .iter()
                        .any(|e| bundle_contains_reference(&e.bundle, r))
            });
            if needed {
                captured.evidence.push(ReferencedResult {
                    input: StudyInput::capture(core),
                    bundle: bundle.clone(),
                    last_static: core.scoped.last_static,
                });
            }
        }
        captured
    }
    fn apply(&self, core: &mut AppCore) {
        core.scoped.references.fixed = self.fixed.clone();
        core.scoped.references.fixed_results = self.results.clone();
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StudyRun {
    pub proposal: String,
    pub proposal_name: String,
    pub input_view_references: ViewReferences,
    pub result_view_references: ViewReferences,
    pub input: StudyInput,
    pub prepared: StudyInput,
    pub results: Option<ResultsBundle>,
    pub preparation: Option<PreparationResult>,
    pub last_static: Option<StaticKey>,
    pub app_version: String,
    pub created: SystemTime,
    pub notice: Option<String>,
    pub error: Option<String>,
    pub quantity_csv: String,
    pub log: Vec<LogEntry>,
    pub diagnostics: Vec<Diagnostic>,
}
impl StudyRun {
    fn referenced_result(
        &self,
        reference: &references::ResultReference,
    ) -> Option<(&StudyInput, &ResultsBundle, Option<StaticKey>)> {
        if let Some(bundle) = self
            .results
            .as_ref()
            .filter(|b| bundle_contains_reference(b, reference))
        {
            return Some((&self.prepared, bundle, self.last_static));
        }
        self.result_view_references
            .evidence
            .iter()
            .chain(&self.input_view_references.evidence)
            .find(|e| bundle_contains_reference(&e.bundle, reference))
            .map(|e| (&e.input, &e.bundle, e.last_static))
    }
}
pub struct RunningStudy {
    study: usize,
    name: String,
    #[cfg(feature = "gui")]
    input: std::sync::Arc<StudyInput>,
    worker: Box<App>,
    preparing: Option<std::sync::mpsc::Receiver<Result<StudyWorkerOutcome, String>>>,
}
#[derive(Default)]
pub struct Studies {
    pub items: Vec<Study>,
    pub active: Location,
    pub main: Option<Slot>,
    pub running: Option<RunningStudy>,
    pub dirty: bool,
    pub window_open: bool,
}
#[derive(Serialize, Deserialize)]
pub struct SavedStudies {
    items: Vec<SavedStudy>,
}
#[derive(Serialize, Deserialize)]
struct SavedStudy {
    purpose: String,
    notes: String,
    fixed: Vec<references::TargetReference>,
    baseline: StudyInput,
    proposals: Vec<(String, StudyInput)>,
    runs: Vec<StudyRun>,
}

enum StudyWorkerOutcome {
    Confirmation(Box<AppCore>),
    Finished(Box<StudyRun>),
}

fn finish_study_run(worker: &mut App, input: &StudyInput, name: String) -> StudyRun {
    let log = worker.core.log.finish_recording();
    let diagnostics = worker
        .core
        .scoped
        .recorded_diagnostics
        .take()
        .unwrap_or_else(|| worker.core.scoped.diagnostics.clone());
    StudyRun {
        log,
        diagnostics,
        input_view_references: ViewReferences::default(),
        result_view_references: ViewReferences::default(),
        proposal_name: name,
        proposal: input.references.proposal.clone(),
        input: input.clone(),
        prepared: StudyInput::capture(&worker.core),
        results: worker.core.scoped.results.take(),
        preparation: worker.core.scoped.preparation.take(),
        last_static: worker.core.scoped.last_static,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created: SystemTime::now(),
        quantity_csv: crate::summary::build_quantity_csv(&worker.core.model),
        notice: worker.core.scoped.last_notice.take(),
        error: worker.core.scoped.last_error.take(),
    }
}
fn prepare_study_worker(
    core: Option<Box<AppCore>>,
    input: std::sync::Arc<StudyInput>,
    name: String,
    resume: bool,
) -> std::sync::mpsc::Receiver<Result<StudyWorkerOutcome, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut worker = App {
                core: core.map(|c| *c).unwrap_or_else(|| input.core()),
                ..Default::default()
            };
            worker.core.log.begin_recording();
            worker
                .core
                .scoped
                .recorded_diagnostics
                .get_or_insert_with(Vec::new);
            #[cfg(feature = "gui")]
            if resume {
                worker.resume_pending_analysis();
            } else {
                worker.request_analysis(PendingAnalysis::StaticAll);
            }
            #[cfg(not(feature = "gui"))]
            {
                let _ = resume;
                worker.start_static_all_job();
            }
            if worker.core.scoped.pending_unset_analysis.is_some() {
                return StudyWorkerOutcome::Confirmation(Box::new(worker.core));
            }
            while worker.core.scoped.job.is_some() {
                worker.poll_job();
                if worker.core.scoped.job.is_some() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            StudyWorkerOutcome::Finished(Box::new(finish_study_run(&mut worker, &input, name)))
        }))
        .map_err(|_| "検討の計算スレッドが異常終了しました".to_string());
        let _ = tx.send(result);
    });
    rx
}

impl App {
    pub fn study_read_only(&self) -> bool {
        matches!(
            self.studies.active,
            Location::History(..) | Location::StartedInput(..) | Location::Proposal(_, 0)
        )
    }
    pub fn study_is_main(&self) -> bool {
        self.studies.active == Location::Main
    }
    pub fn create_study(&mut self, purpose: String, targets: Vec<ElemId>) -> Result<usize, String> {
        if !self.study_is_main() {
            return Err("検討は本案から作成してください".into());
        }
        if self.core.scoped.job.is_some() {
            return Err("解析実行中です".into());
        }
        let fixed = targets
            .into_iter()
            .map(|id| {
                let e = self.core.model.element(id).ok_or("固定対象がありません")?;
                if e.kind != sepika_core::model::ElementKind::Beam
                    || !e
                        .section
                        .and_then(|id| self.core.model.section(id))
                        .is_some_and(|s| sepika_edit::valid_study_section(&self.core.model, s))
                {
                    return Err("固定対象には既存の鉄骨大梁を指定してください".to_string());
                }
                self.target_reference(references::ReferenceTarget::Member(id))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let baseline = StudyInput::capture(&self.core);
        let mut input = baseline.clone();
        input.references.proposal = uuid::Uuid::new_v4().to_string();
        input.references.generation = 0;
        let mut state = Slot::new(&input);
        state.core.scoped.undo.restrict_sections(Vec::new());
        let index = self.studies.items.len();
        self.studies.items.push(Study {
            purpose,
            notes: String::new(),
            fixed,
            baseline,
            proposals: vec![Proposal {
                name: "基準案".into(),
                state: Some(state),
            }],
            runs: Vec::new(),
        });
        self.mark_study_dirty();
        self.studies.window_open = true;
        Ok(index)
    }
    fn mark_study_dirty(&mut self) {
        self.studies.dirty = true;
        self.core.scoped.staleness.mark_non_calc_edited();
    }
    fn take_active_slot(&mut self) -> Slot {
        Slot {
            core: std::mem::take(&mut self.core),
            ui: std::mem::take(&mut self.ui.scoped),
        }
    }
    fn put_slot(&mut self, slot: Slot) {
        self.core = slot.core;
        self.ui.scoped = slot.ui;
    }
    fn store_slot(&mut self, location: Location, slot: Slot) {
        match location {
            Location::Main => self.studies.main = Some(slot),
            Location::Proposal(s, p) => self.studies.items[s].proposals[p].state = Some(slot),
            Location::History(..) | Location::StartedInput(..) => {}
        }
    }
    pub fn switch_study(&mut self, next: Location) -> Result<(), String> {
        if self.studies.active == next {
            return Ok(());
        }
        if self.core.scoped.job.is_some() {
            return Err("本案の解析実行中は案を切り替えられません".into());
        }
        let mut next_slot = match next {
            Location::Main => self.studies.main.take().ok_or("本案がありません")?,
            Location::Proposal(s, p) => self
                .studies
                .items
                .get_mut(s)
                .and_then(|s| s.proposals.get_mut(p))
                .and_then(|p| p.state.take())
                .ok_or("案がありません")?,
            Location::StartedInput(s, r) => {
                let run = self
                    .studies
                    .items
                    .get(s)
                    .and_then(|s| s.runs.get(r))
                    .ok_or("計算履歴がありません")?;
                let mut slot = Slot::new(&run.input);
                run.input_view_references.apply(&mut slot.core);
                slot.core.log.entries = run.log.clone();
                slot.core.scoped.diagnostics = run.diagnostics.clone();
                slot.core.scoped.staleness.diagnostics_stale = false;
                slot.core.scoped.undo.restrict_sections(Vec::new());
                slot
            }
            Location::History(s, r) => {
                let run = self
                    .studies
                    .items
                    .get(s)
                    .and_then(|s| s.runs.get(r))
                    .ok_or("計算履歴がありません")?;
                let mut slot = Slot::new(&run.prepared);
                run.result_view_references.apply(&mut slot.core);
                slot.core.log.entries = run.log.clone();
                slot.core.scoped.diagnostics = run.diagnostics.clone();
                slot.core.scoped.staleness.diagnostics_stale = false;
                slot.core.scoped.results = run.results.clone();
                slot.core.scoped.preparation = run.preparation.clone();
                slot.core.scoped.last_static = run.last_static;
                slot.core.scoped.staleness.results_stale = false;
                slot.core.scoped.staleness.preparation_stale = false;
                slot.core.scoped.undo.restrict_sections(Vec::new());
                slot
            }
        };
        self.retain_study_view_references();
        next_slot.core.scoped.project_path = self.core.scoped.project_path.clone();
        next_slot.core.scoped.pending_save_recording =
            self.core.scoped.pending_save_recording.take();
        let previous = self.studies.active;
        let slot = self.take_active_slot();
        self.store_slot(previous, slot);
        self.put_slot(next_slot);
        self.studies.active = next;
        Ok(())
    }
    fn proposal_input(&self, s: usize, p: usize) -> StudyInput {
        if self.studies.active == Location::Proposal(s, p) {
            StudyInput::capture(&self.core)
        } else {
            StudyInput::capture(
                &self.studies.items[s].proposals[p]
                    .state
                    .as_ref()
                    .expect("非表示案")
                    .core,
            )
        }
    }
    pub fn create_comparison(&mut self, s: usize, name: String) -> Result<usize, String> {
        let study = self.studies.items.get(s).ok_or("検討がありません")?;
        let mut input = study.baseline.clone();
        input.references.proposal = uuid::Uuid::new_v4().to_string();
        input.references.generation = 0;
        let targets = study
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
        let mut state = Slot::new(&input);
        state.core.scoped.undo.restrict_sections(targets);
        let p = study.proposals.len();
        self.studies.items[s].proposals.push(Proposal {
            name,
            state: Some(state),
        });
        self.mark_study_dirty();
        Ok(p)
    }
    pub fn delete_comparison(&mut self, s: usize, p: usize) -> Result<(), String> {
        if p == 0 {
            return Err("基準案は削除できません".into());
        }
        if self.studies.running.as_ref().is_some_and(|r| r.study == s) {
            return Err("この検討は計算中です".into());
        }
        let input = self.proposal_input(s, p);
        if self.studies.items[s]
            .runs
            .iter()
            .any(|r| r.proposal == input.references.proposal)
        {
            return Err("保持した計算結果の根拠となる案は削除できません".into());
        }
        if self.studies.active != Location::Main {
            self.switch_study(Location::Main)?;
        }
        self.studies.items[s].proposals.remove(p);
        self.mark_study_dirty();
        Ok(())
    }
    pub(crate) fn retain_study_view_references(&mut self) {
        let (s, r, history) = match self.studies.active {
            Location::History(s, r) => (s, r, true),
            Location::StartedInput(s, r) => (s, r, false),
            _ => return,
        };
        let run = &self.studies.items[s].runs[r];
        let old = if history {
            &run.result_view_references
        } else {
            &run.input_view_references
        };
        let refs_changed = old.fixed != self.core.scoped.references.fixed
            || old.results != self.core.scoped.references.fixed_results;
        let needed = self.core.scoped.results.as_ref().is_some_and(|bundle| {
            self.core
                .scoped
                .references
                .fixed_results
                .iter()
                .any(|reference| {
                    reference.kind == ResultInputKey::Design
                        && bundle_contains_reference(bundle, reference)
                        && !run
                            .results
                            .as_ref()
                            .is_some_and(|b| bundle_contains_reference(b, reference))
                        && !old
                            .evidence
                            .iter()
                            .any(|e| bundle_contains_reference(&e.bundle, reference))
                })
        });
        if !refs_changed && !needed {
            return;
        }
        let saved = ViewReferences::capture(&self.core, old, run.results.as_ref());
        let changed = old.fixed != saved.fixed
            || old.results != saved.results
            || old.evidence.len() != saved.evidence.len();
        if changed {
            let run = &mut self.studies.items[s].runs[r];
            if history {
                run.result_view_references = saved;
            } else {
                run.input_view_references = saved;
            }
            self.studies.dirty = true;
        }
    }
    #[cfg(feature = "gui")]
    fn set_study_result_selection(&mut self, last: Option<StaticKey>) {
        self.core.scoped.last_static = last;
        self.ui.scoped.nav.focus_result = last;
        #[cfg(feature = "gui")]
        {
            self.active_viewport_mut().static_key = last;
            self.active_viewport_mut().static_unavailable = false;
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn activate_study_result_reference(
        &mut self,
        reference: &references::ResultReference,
    ) {
        if self
            .core
            .scoped
            .results
            .as_ref()
            .is_some_and(|b| bundle_contains_reference(b, reference))
        {
            if reference.kind == ResultInputKey::Design {
                self.set_study_result_selection(self.core.scoped.last_static);
            }
            return;
        }
        let Some(Location::History(s, r)) = self.study_result_location(reference) else {
            return;
        };
        self.retain_study_view_references();
        let run = &self.studies.items[s].runs[r];
        if let Some((input, bundle, last)) = run.referenced_result(reference) {
            self.core.model = input.model.clone();
            input.settings.apply(&mut self.core);
            self.core.scoped.references = input.references.clone();
            run.result_view_references.apply(&mut self.core);
            self.core.scoped.results = Some(bundle.clone());
            self.set_study_result_selection(last);
        }
    }
    fn saved_study_runs(&self, s: usize) -> Vec<StudyRun> {
        let mut runs = self.studies.items[s].runs.clone();
        match self.studies.active {
            Location::History(active, r) if active == s => {
                runs[r].result_view_references = ViewReferences::capture(
                    &self.core,
                    &runs[r].result_view_references,
                    runs[r].results.as_ref(),
                )
            }
            Location::StartedInput(active, r) if active == s => {
                runs[r].input_view_references = ViewReferences::capture(
                    &self.core,
                    &runs[r].input_view_references,
                    runs[r].results.as_ref(),
                )
            }
            _ => {}
        }
        runs
    }
    pub fn study_target_location(&self, target: &references::TargetReference) -> Option<Location> {
        // 派生値の再算定で共通参照の入力版は変わらないため、両ビューが同じ参照を持つときは表示先を維持する。
        if matches!(
            self.studies.active,
            Location::StartedInput(..) | Location::History(..)
        ) && self.core.scoped.references.fixed.contains(target)
        {
            return Some(self.studies.active);
        }
        self.studies
            .items
            .iter()
            .enumerate()
            .find_map(|(s, study)| {
                study.runs.iter().enumerate().find_map(|(r, run)| {
                    if run.result_view_references.fixed.contains(target) {
                        Some(Location::History(s, r))
                    } else if run.input_view_references.fixed.contains(target) {
                        Some(Location::StartedInput(s, r))
                    } else {
                        None
                    }
                })
            })
    }
    pub fn saved_studies(&self) -> SavedStudies {
        SavedStudies {
            items: self
                .studies
                .items
                .iter()
                .enumerate()
                .map(|(s, study)| SavedStudy {
                    purpose: study.purpose.clone(),
                    notes: study.notes.clone(),
                    fixed: study.fixed.clone(),
                    baseline: study.baseline.clone(),
                    proposals: study
                        .proposals
                        .iter()
                        .enumerate()
                        .map(|(p, proposal)| (proposal.name.clone(), self.proposal_input(s, p)))
                        .collect(),
                    runs: self.saved_study_runs(s),
                })
                .collect(),
        }
    }
    pub fn restore_studies(&mut self, saved: SavedStudies) -> Result<(), String> {
        let mut items = Vec::new();
        for study in saved.items {
            let targets: Vec<_> = study
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
            let mut proposals = Vec::new();
            if study.proposals.is_empty() {
                return Err("検討の基準案がありません".into());
            }
            for (p, (name, input)) in study.proposals.into_iter().enumerate() {
                input
                    .model
                    .validate_reference_ids()
                    .map_err(|e| e.to_string())?;
                let mut slot = Slot::new(&input);
                slot.core.scoped.undo.restrict_sections(if p == 0 {
                    Vec::new()
                } else {
                    targets.clone()
                });
                proposals.push(Proposal {
                    name,
                    state: Some(slot),
                });
            }
            items.push(Study {
                purpose: study.purpose,
                notes: study.notes,
                fixed: study.fixed,
                baseline: study.baseline,
                proposals,
                runs: study.runs,
            });
        }
        self.studies = Studies {
            items,
            ..Default::default()
        };
        Ok(())
    }
    pub fn start_study_calculation(&mut self, s: usize, p: usize) -> Result<(), String> {
        if self.studies.running.is_some()
            || self.core.scoped.job.is_some()
            || self
                .studies
                .main
                .as_ref()
                .is_some_and(|m| m.core.scoped.job.is_some())
        {
            return Err("別の解析を実行中です".into());
        }
        let input = std::sync::Arc::new(self.proposal_input(s, p));
        let name = self.studies.items[s].proposals[p].name.clone();
        let preparing = Some(prepare_study_worker(
            None,
            input.clone(),
            name.clone(),
            false,
        ));
        self.studies.running = Some(RunningStudy {
            study: s,
            name,
            #[cfg(feature = "gui")]
            input,
            worker: Box::default(),
            preparing,
        });
        self.mark_study_dirty();
        Ok(())
    }
    #[cfg(feature = "gui")]
    pub fn resume_study_calculation(&mut self) {
        if let Some(running) = &mut self.studies.running {
            if running.preparing.is_none()
                && running.worker.core.scoped.pending_unset_analysis.is_some()
            {
                let core = std::mem::take(&mut running.worker.core);
                running.preparing = Some(prepare_study_worker(
                    Some(Box::new(core)),
                    running.input.clone(),
                    running.name.clone(),
                    true,
                ));
            }
        }
    }
    pub fn poll_study_calculation(&mut self) -> bool {
        let Some(mut running) = self.studies.running.take() else {
            return false;
        };
        if let Some(preparing) = &running.preparing {
            match preparing.try_recv() {
                Ok(Ok(StudyWorkerOutcome::Confirmation(core))) => {
                    running.worker.core = *core;
                    running.preparing = None;
                    self.studies.window_open = true;
                }
                Ok(Ok(StudyWorkerOutcome::Finished(run))) => {
                    self.studies.items[running.study].runs.push(*run);
                    self.mark_study_dirty();
                    return true;
                }
                Ok(Err(reason)) => {
                    self.report_error(format!("検討「{}」: {reason}", running.name));
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.report_error(format!(
                        "検討「{}」の計算スレッドが異常終了しました",
                        running.name
                    ));
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        } else if running.worker.core.scoped.pending_unset_analysis.is_some() {
            self.studies.window_open = true;
        }
        self.studies.running = Some(running);
        false
    }
    pub fn study_main_changed(&self, s: usize) -> bool {
        let main = if self.study_is_main() {
            &self.core
        } else {
            &self.studies.main.as_ref().expect("本案").core
        };
        let baseline = &self.studies.items[s].baseline;
        main.scoped.references.generation + main.scoped.undo.revision()
            - main.scoped.reference_preparation_revisions
            != baseline.references.generation
            || bincode::serialize(&StudySettings::capture(main)).expect("設定")
                != bincode::serialize(&baseline.settings).expect("設定")
    }
}

impl App {
    pub fn study_result_location(
        &self,
        reference: &references::ResultReference,
    ) -> Option<Location> {
        self.studies
            .items
            .iter()
            .enumerate()
            .find_map(|(s, study)| {
                study.runs.iter().enumerate().find_map(|(r, run)| {
                    (run.results
                        .as_ref()
                        .is_some_and(|b| bundle_contains_reference(b, reference))
                        || run
                            .result_view_references
                            .evidence
                            .iter()
                            .chain(&run.input_view_references.evidence)
                            .any(|e| bundle_contains_reference(&e.bundle, reference)))
                    .then_some(Location::History(s, r))
                })
            })
    }
    pub(crate) fn study_result_view(&self, reference: &references::ResultReference) -> Option<App> {
        let Location::History(s, r) = self.study_result_location(reference)? else {
            unreachable!()
        };
        if self.core.scoped.references.project == reference.source.project
            && self.core.scoped.references.proposal == reference.source.proposal
            && self
                .core
                .scoped
                .results
                .as_ref()
                .is_some_and(|b| bundle_contains_reference(b, reference))
        {
            return None;
        }
        let (input, bundle, last) = self.studies.items[s].runs[r].referenced_result(reference)?;
        let mut app = App {
            core: input.core(),
            ..Default::default()
        };
        app.core.scoped.results = Some(bundle.clone());
        app.core.scoped.last_static = last;
        Some(app)
    }
    pub(crate) fn resolve_study_result(
        &self,
        reference: &references::ResultReference,
    ) -> Option<Result<references::ReferenceTarget, String>> {
        self.study_result_view(reference)
            .map(|app| app.resolve_result_reference(reference))
    }
    pub fn project_unsaved(&self) -> bool {
        self.studies.dirty
            || self.core.scoped.staleness.unsaved_changes
            || self
                .studies
                .main
                .as_ref()
                .is_some_and(|s| s.core.scoped.staleness.unsaved_changes)
            || self.studies.items.iter().any(|s| {
                s.proposals.iter().any(|p| {
                    p.state
                        .as_ref()
                        .is_some_and(|s| s.core.scoped.staleness.unsaved_changes)
                })
            })
    }
}

#[cfg(feature = "gui")]
impl App {
    pub(crate) fn study_window(&mut self, ctx: &egui::Context) {
        if !self.studies.window_open {
            return;
        }
        let mut open = true;
        egui::Window::new("検討・設計案")
            .open(&mut open)
            .collapsible(self.studies.running.is_none())
            .vscroll(true)
            .show(ctx, |ui| {
                let active = match self.studies.active {
                    Location::Main => "本案".to_string(),
                    Location::Proposal(s, p) => self.studies.items[s].proposals[p].name.clone(),
                    Location::History(..) => "準備後入力・結果（読取専用）".into(),
                    Location::StartedInput(..) => "計算開始時入力（読取専用）".into(),
                };
                ui.strong(format!("表示・編集先: {active}"));
                if ui.button("本案を表示").clicked() {
                    if let Err(reason) = self.switch_study(Location::Main) { self.report_error(reason); }
                }
                let create_button = egui::Button::new("選択した鉄骨大梁・全ケース・全組合せで検討作成");
                if ui.add_enabled(self.study_is_main(), create_button).clicked() {
                    let targets = self.ui.scoped.selection.members().to_vec();
                    if let Err(reason) = self.create_study("断面の検討".into(), targets) { self.report_error(reason); }
                }
                ui.label("条件: 作成時の解析・検定設定、全荷重ケース・全組合せを固定して計算します。");
                let mut switch = None;
                let mut create = None;
                let mut delete = None;
                let mut calculate = None;
                for s in 0..self.studies.items.len() {
                    ui.separator();
                    let main_changed = self.study_main_changed(s);
                    let study = &mut self.studies.items[s];
                    let mut changed = ui.text_edit_singleline(&mut study.purpose).changed();
                    ui.label(format!("固定対象: {} 部材", study.fixed.len()));
                    changed |= ui.text_edit_multiline(&mut study.notes).changed();
                    if main_changed {
                        ui.colored_label(crate::theme::WARN_TEXT, "本案は検討開始時から変更されています。履歴閲覧は可能です。採用可否の判定・採用は未対応です。");
                    }
                    if ui.button("基準案から比較案を作成").clicked() { create = Some(s); }
                    for (p, proposal) in study.proposals.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            if p == 0 { ui.label(&proposal.name); }
                            else { changed |= ui.text_edit_singleline(&mut proposal.name).changed(); }
                            if ui.button("表示").clicked() { switch = Some(Location::Proposal(s, p)); }
                            if ui.button("計算").clicked() { calculate = Some((s, p)); }
                            if p > 0 && ui.button("削除").clicked() { delete = Some((s, p)); }
                        });
                    }
                    for (r, run) in study.runs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let outcome = if run.error.is_some() { "エラーを含む履歴" } else { "保持した結果" };
                            ui.label(format!("計算 {} · {} · 入力世代 {}: {outcome}", r + 1, run.proposal_name, run.input.references.generation));
                            if ui.button("準備後入力・結果を表示").clicked() { switch = Some(Location::History(s, r)); }
                            if ui.button("計算開始時入力を表示").clicked() { switch = Some(Location::StartedInput(s, r)); }
                        });
                        ui.collapsing(format!("計算 {} の注意・診断",r+1),|ui| {
                            for entry in &run.log {
                                if entry.level!=LogLevel::Info {ui.colored_label(entry.level.color(),&entry.message);}
                            }
                            for diagnostic in &run.diagnostics {
                                if diagnostic.severity!=DiagSeverity::Info {ui.label(&diagnostic.message);}
                            }
                            if let Some(notice)=&run.notice {
                                if !run.log.iter().any(|e|e.message==*notice) {ui.label(notice);}
                            }
                            if let Some(error)=&run.error {
                                if !run.log.iter().any(|e|e.message==*error) {ui.colored_label(crate::theme::ERROR_RED,error);}
                            }
                        });
                    }
                    if changed { self.studies.dirty = true; }
                }
                let result = if let Some(next) = switch { self.switch_study(next) }
                    else if let Some(s) = create { self.create_comparison(s, "比較案".into()).map(|_| ()) }
                    else if let Some((s, p)) = delete { self.delete_comparison(s, p) }
                    else if let Some((s, p)) = calculate { self.start_study_calculation(s, p) }
                    else { Ok(()) };
                if let Err(reason) = result { self.report_error(reason); }
                let mut resume=false;
                if let Some(running) = &mut self.studies.running {
                    ui.separator();
                    ui.label(format!("{} を計算中です。別の計算は開始できません。", running.name));
                    if running.worker.core.scoped.pending_unset_analysis.is_some() {
                        let (floors, walls) = running.worker.core.model.unset_plate_assignment_regions();
                        ui.colored_label(crate::theme::WARN_TEXT, "未設定の床板・壁版割当領域が残っています。未設定の領域は床荷重・躯体自重と壁の剛性が解析に入らず、応力・変形を過小評価し得ます。計算前に内容を確認してください。");
                        ui.label(format!("床板割当領域: {floors:?} / 壁版割当領域: {walls:?}"));
                        if ui.button("続行して計算").clicked() { resume=true; }
                        if ui.button("キャンセル").clicked() { self.studies.running = None; }
                    }
                }
                if resume {self.resume_study_calculation();}
            });
        self.studies.window_open = open;
    }
}

impl App {
    pub fn resolve_study_fixed_target(
        &self,
        study: usize,
        target: &references::TargetReference,
    ) -> Result<references::ReferenceTarget, String> {
        let study = self.studies.items.get(study).ok_or("検討がありません")?;
        if !study.fixed.contains(target) {
            return Err("検討の固定対象ではありません".into());
        }
        let app = App {
            core: study.baseline.core(),
            ..Default::default()
        };
        app.resolve_target_reference(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(value: &impl Serialize) -> Vec<u8> {
        rmp_serde::to_vec_named(value).unwrap()
    }
    fn app() -> App {
        let mut app = App::default();
        let mut model = crate::sample::portal_frame();
        model.panel_zone = sepika_core::model::PanelZoneMode::None;
        let mut section = sepika_core::section_shape::SectionShape::SteelH {
            root_r: Some(13.0),
            height: 500.0,
            width: 250.0,
            web_thick: 10.0,
            flange_thick: 16.0,
        }
        .to_section(SectionId(2), "比較用大梁".into());
        section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        section.material = model.sections[1].material;
        model.sections.push(section);
        assert!(app.load_model(model));
        app.core.analysis_cfg.threads = 1;
        app
    }
    fn assign(app: &mut App, section: u32) -> bool {
        let result = app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetElementSection {
                elem: ElemId(2),
                section: Some(SectionId(section)),
            }),
        );
        if result {
            app.core.scoped.staleness.mark_edited();
        }
        result
    }
    fn wait(app: &mut App) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.studies.running.is_some() {
            #[cfg(feature = "gui")]
            assert!(app
                .studies
                .running
                .as_ref()
                .unwrap()
                .worker
                .core
                .scoped
                .pending_unset_analysis
                .is_none());
            app.poll_study_calculation();
            assert!(std::time::Instant::now() < deadline, "計算が完了しない");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    fn proposals_isolate_input_undo_and_incomplete_drafts() {
        let mut app = app();
        let main = bytes(&app.core.model);
        let s = app
            .create_study("梁断面の検討".into(), vec![ElemId(2)])
            .unwrap();
        let fixed = app.studies.items[s].fixed[0].clone();
        assert_eq!(
            app.resolve_study_fixed_target(s, &fixed).unwrap(),
            references::ReferenceTarget::Member(ElemId(2))
        );
        let a = app.create_comparison(s, "案A".into()).unwrap();
        let b = app.create_comparison(s, "案B".into()).unwrap();
        app.switch_study(Location::Proposal(s, a)).unwrap();
        app.ui.scoped.node_edit = vec![["-".into(), "".into(), "".into()]];
        assert!(assign(&mut app, 2));
        assert!(app.core.scoped.undo.can_undo());
        assert!(!app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetNodeCoord {
                node: NodeId(0),
                coord: [20., 0., 0.]
            })
        ));
        assert!(!app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetElementSection {
                elem: ElemId(0),
                section: Some(SectionId(2))
            })
        ));
        assert!(!assign(&mut app, 0));
        app.switch_study(Location::Proposal(s, b)).unwrap();
        assert_eq!(bytes(&app.core.model), main);
        assert!(!app.core.scoped.undo.can_undo());
        app.switch_study(Location::Proposal(s, 0)).unwrap();
        assert!(!assign(&mut app, 2));
        assert_eq!(bytes(&app.core.model), main);
        app.switch_study(Location::Proposal(s, a)).unwrap();
        assert_eq!(app.ui.scoped.node_edit[0][0], "-");
        app.core.scoped.undo.undo(&mut app.core.model);
        assert_eq!(bytes(&app.core.model), main);
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(app.core.model.elements[2].section, Some(SectionId(2)));
        app.switch_study(Location::Main).unwrap();
        assert_eq!(bytes(&app.core.model), main);
        assert!(!app.core.scoped.undo.can_undo());
    }
    #[test]
    fn calculation_retains_start_input_after_edit_and_roundtrip_resolves_history() {
        let mut app = app();
        let main = bytes(&app.core.model);
        let s = app.create_study("検討".into(), vec![ElemId(2)]).unwrap();
        let p = app.create_comparison(s, "案A".into()).unwrap();
        app.switch_study(Location::Proposal(s, p)).unwrap();
        assert!(assign(&mut app, 2));
        let changed = bytes(&app.core.model);
        app.start_study_calculation(s, p).unwrap();
        assert!(app
            .start_study_calculation(s, 0)
            .unwrap_err()
            .contains("実行中"));
        assert!(assign(&mut app, 1));
        let revised = bytes(&app.core.model);
        app.switch_study(Location::Main).unwrap();
        assert_eq!(bytes(&app.core.model), main);
        wait(&mut app);
        let run = &app.studies.items[s].runs[0];
        assert_eq!(bytes(&run.input.model), changed);
        assert!(
            run.results.as_ref().is_some_and(|b| !b.statics.is_empty()),
            "{:?}",
            run.error
        );
        assert_eq!(run.prepared.model.elements.len(), 3);
        assert_eq!(run.input.model.elements[2].section, Some(SectionId(2)));
        assert!(app.delete_comparison(s, p).is_err());
        app.switch_study(Location::StartedInput(s, 0)).unwrap();
        assert_eq!(bytes(&app.core.model), changed);
        assert!(app.core.scoped.results.is_none());
        assert!(!assign(&mut app, 1));
        app.switch_study(Location::History(s, 0)).unwrap();
        assert!(!assign(&mut app, 1));
        let key = ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)));
        let target = app
            .target_reference(references::ReferenceTarget::Member(ElemId(2)))
            .unwrap();
        let reference = app
            .result_reference(target, key, references::DisplayConditions::default())
            .unwrap();
        assert!(app.resolve_result_reference(&reference).is_ok());
        app.switch_study(Location::Proposal(s, p)).unwrap();
        assert_eq!(bytes(&app.core.model), revised);
        let before = bytes(&app.saved_studies());
        let path =
            std::env::temp_dir().join(format!("sepika-study-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert_eq!(app.studies.active, Location::Proposal(s, p));
        assert!(!app.project_unsaved());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(bytes(&loaded.core.model), main);
        assert_eq!(bytes(&loaded.saved_studies()), before);
        assert!(loaded.resolve_result_reference(&reference).is_ok());
        assert!(loaded
            .resolve_study_fixed_target(s, &loaded.studies.items[s].fixed[0])
            .is_ok());
        loaded.switch_study(Location::History(s, 0)).unwrap();
        assert!(loaded.resolve_result_reference(&reference).is_ok());
        loaded.start_study_calculation(s, 0).unwrap();
        wait(&mut loaded);
        assert_eq!(loaded.studies.items[s].runs.len(), 2);
        assert_eq!(
            loaded.studies.items[s].runs[0]
                .input
                .model
                .load_cases
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            loaded.studies.items[s].runs[1]
                .input
                .model
                .load_cases
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>()
        );
        assert_ne!(
            loaded.studies.items[s].runs[0].quantity_csv,
            loaded.studies.items[s].runs[1].quantity_csv
        );
        let larger = &loaded.studies.items[s].runs[0];
        let baseline = &loaded.studies.items[s].runs[1];
        assert_eq!(
            bytes(&larger.input.settings.analysis_cfg),
            bytes(&baseline.input.settings.analysis_cfg)
        );
        assert_eq!(
            bytes(&larger.input.model.combinations),
            bytes(&baseline.input.model.combinations)
        );
        let larger_weight: f64 = larger
            .prepared
            .model
            .stories
            .iter()
            .filter_map(|s| s.seismic_weight)
            .sum();
        let baseline_weight: f64 = baseline
            .prepared
            .model
            .stories
            .iter()
            .filter_map(|s| s.seismic_weight)
            .sum();
        assert!(
            larger_weight > baseline_weight,
            "大断面 {larger_weight} N / 基準 {baseline_weight} N"
        );
        assert_ne!(
            bytes(
                &larger
                    .prepared
                    .model
                    .stories
                    .iter()
                    .map(|s| &s.dynamic_mass)
                    .collect::<Vec<_>>()
            ),
            bytes(
                &baseline
                    .prepared
                    .model
                    .stories
                    .iter()
                    .map(|s| &s.dynamic_mass)
                    .collect::<Vec<_>>()
            )
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn failed_save_preserves_current_proposal_and_dirty_and_running_is_not_restored() {
        let mut app = app();
        let s = app.create_study("確認".into(), vec![]).unwrap();
        assert_eq!(app.studies.items[s].proposals.len(), 1);
        let p = app.create_comparison(s, "削除可能".into()).unwrap();
        app.delete_comparison(s, p).unwrap();
        app.switch_study(Location::Proposal(s, 0)).unwrap();
        let baseline = bytes(&app.core.model);
        app.save_project_to(
            std::env::temp_dir()
                .join(uuid::Uuid::new_v4().to_string())
                .join("absent.ovika"),
        );
        assert!(app.core.scoped.last_error.is_some());
        assert!(app.project_unsaved());
        assert_eq!(app.studies.active, Location::Proposal(s, 0));
        assert_eq!(bytes(&app.core.model), baseline);
        app.start_study_calculation(s, 0).unwrap();
        let saved = app.saved_studies();
        let mut restored = App::default();
        restored.restore_studies(saved).unwrap();
        assert!(restored.studies.running.is_none());
        assert!(restored.studies.items[s].runs.is_empty());
        wait(&mut app);
    }
    #[cfg(feature = "gui")]
    #[test]
    fn unset_regions_require_confirmation_on_calculation_copy_and_save_prompt_survives_switch() {
        let mut app = app();
        let mut bottom = app.core.model.elements[2].clone();
        bottom.id = ElemId(3);
        bottom.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
        app.core.model.elements.push(bottom);
        app.core.model.rebuild_assignment_regions().unwrap();
        assert!(!app.core.model.unset_plate_assignment_regions().1.is_empty());
        let s = app
            .create_study("未設定の確認".into(), vec![ElemId(2)])
            .unwrap();
        let baseline = bytes(&app.studies.items[s].baseline.model);
        app.start_study_calculation(s, 0).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.studies.running.as_ref().unwrap().preparing.is_some() {
            app.poll_study_calculation();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let worker = &app.studies.running.as_ref().unwrap().worker;
        assert!(worker.core.scoped.pending_unset_analysis.is_some());
        assert!(worker.core.scoped.job.is_none());
        assert!(!app.poll_study_calculation());
        app.switch_study(Location::Proposal(s, 0)).unwrap();
        let path = std::env::temp_dir().join("study-pending-confirm.ovika");
        app.core.scoped.pending_save_recording = Some((path.clone(), 513));
        app.switch_study(Location::Main).unwrap();
        assert_eq!(
            app.core.scoped.pending_save_recording.as_ref().unwrap().0,
            path
        );
        app.switch_study(Location::Proposal(s, 0)).unwrap();
        app.resolve_pending_save_recording(None);
        assert!(app.core.scoped.pending_save_recording.is_none());
        assert!(app.project_unsaved());
        app.resume_study_calculation();
        wait(&mut app);
        assert_eq!(bytes(&app.studies.items[s].baseline.model), baseline);
        assert!(app.studies.items[s].runs[0].results.is_some());
        assert!(app.studies.items[s].runs[0]
            .preparation
            .as_ref()
            .unwrap()
            .has_unset_regions());
    }
    #[cfg(feature = "gui")]
    #[test]
    fn historical_design_frames_are_silent_and_quantity_browsing_and_copy_remain_enabled() {
        fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(2200.0, 2000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.design_tab_panel(ui),
            )
        }
        fn click(
            ctx: &egui::Context,
            app: &mut App,
            output: &egui::FullOutput,
            label: &str,
        ) -> egui::FullOutput {
            let pos = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == label => {
                        Some(t.visual_bounding_rect().center())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("操作が見つからない: {label}"));
            let mut out = None;
            for pressed in [true, false] {
                out = Some(frame(
                    ctx,
                    app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                ));
            }
            out.unwrap()
        }
        let mut app = app();
        let s = app.create_study("閲覧".into(), vec![ElemId(2)]).unwrap();
        app.start_study_calculation(s, 0).unwrap();
        wait(&mut app);
        app.switch_study(Location::History(s, 0)).unwrap();
        let before = bytes(&app.core.model);
        let cfg = bytes(&app.core.analysis_cfg);
        let log = app.core.log.entries.len();
        let generation = app.input_reference();
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.core.log.entries.len(), log);
        assert!(app.core.scoped.last_error.is_none());
        assert_eq!(app.input_reference(), generation);
        app.ui.view.design_view = DesignView::Quantities;
        let output = frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, &output, "明細");
        assert_eq!(
            app.ui.view.quantity_view.grouping,
            crate::quantity_view::QuantityGrouping::Detail
        );
        let output = frame(&ctx, &mut app, vec![]);
        let copied = click(&ctx, &mut app, &output, "📋 クリップボードへコピー");
        assert!(copied.platform_output.commands.iter().any(|c|matches!(c,egui::OutputCommand::CopyText(text) if text==&crate::summary::build_quantity_csv(&app.core.model))));
        assert_eq!(bytes(&app.core.model), before);
        assert_eq!(bytes(&app.core.analysis_cfg), cfg);
        assert_eq!(app.core.log.entries.len(), log);
        assert_eq!(app.input_reference(), generation);
        let mut changed = app.core.analysis_cfg;
        changed.threads += 1;
        assert!(!app.commit_analysis_settings(changed));
        assert_eq!(bytes(&app.core.analysis_cfg), cfg);
    }
    #[cfg(feature = "gui")]
    #[test]
    fn annotations_on_both_retained_input_views_survive_switch_save_and_reload_without_mutating_evidence(
    ) {
        let mut app = app();
        let s = app
            .create_study("参照の保持".into(), vec![ElemId(2)])
            .unwrap();
        app.start_study_calculation(s, 0).unwrap();
        wait(&mut app);
        let input = bytes(&app.studies.items[s].runs[0].input);
        let prepared = bytes(&app.studies.items[s].runs[0].prepared);
        let results = bytes(&app.studies.items[s].runs[0].results);
        app.switch_study(Location::History(s, 0)).unwrap();
        app.select_member(ElemId(2));
        app.freeze_selected_references().unwrap();
        let target = app.core.scoped.references.fixed[0].clone();
        let result = app
            .result_reference(
                target.clone(),
                ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0))),
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        app.core
            .scoped
            .references
            .fixed_results
            .push(result.clone());
        app.core.scoped.staleness.mark_non_calc_edited();
        app.switch_study(Location::Main).unwrap();
        assert!(app.project_unsaved());
        app.switch_study(Location::History(s, 0)).unwrap();
        assert_eq!(app.core.scoped.references.fixed, vec![target.clone()]);
        assert_eq!(
            app.core.scoped.references.fixed_results,
            vec![result.clone()]
        );
        let path = std::env::temp_dir().join(format!(
            "sepika-study-annotations-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        assert!(!app.project_unsaved());
        assert_eq!(
            app.core.scoped.references.fixed_results,
            vec![result.clone()]
        );
        let mut first = App::default();
        first.open_project_from(path.clone());
        first
            .show_reference(&target, references::ReferenceView::Table, true)
            .unwrap();
        assert_eq!(first.studies.active, Location::History(s, 0));
        assert!(first.resolve_result_reference(&result).is_ok());
        app.switch_study(Location::StartedInput(s, 0)).unwrap();
        app.select_member(ElemId(2));
        app.freeze_selected_references().unwrap();
        let raw = app.core.scoped.references.fixed[0].clone();
        app.switch_study(Location::Main).unwrap();
        assert!(app.project_unsaved());
        app.switch_study(Location::StartedInput(s, 0)).unwrap();
        assert_eq!(app.core.scoped.references.fixed, vec![raw.clone()]);
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        assert!(!app.project_unsaved());
        assert_eq!(app.core.scoped.references.fixed, vec![raw.clone()]);
        let run = &app.studies.items[s].runs[0];
        assert_eq!(bytes(&run.input), input);
        assert_eq!(bytes(&run.prepared), prepared);
        assert_eq!(bytes(&run.results), results);
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        loaded.switch_study(Location::StartedInput(s, 0)).unwrap();
        loaded
            .show_reference(&raw, references::ReferenceView::Table, true)
            .unwrap();
        assert_eq!(loaded.studies.active, Location::StartedInput(s, 0));
        assert!(loaded.resolve_target_reference(&raw).is_ok());
        loaded
            .show_result_reference(&result, references::ReferenceView::Table, true)
            .unwrap();
        assert_eq!(loaded.studies.active, Location::History(s, 0));
        assert_eq!(
            loaded.core.scoped.references.fixed_results,
            vec![result.clone()]
        );
        assert!(loaded.resolve_result_reference(&result).is_ok());
        let run = &loaded.studies.items[s].runs[0];
        assert_eq!(bytes(&run.input), input);
        assert_eq!(bytes(&run.prepared), prepared);
        assert_eq!(bytes(&run.results), results);
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(feature = "gui")]
    #[test]
    fn all_preparation_warnings_and_diagnostics_survive_completion_roundtrip_and_history_display() {
        let mut app = app();
        let mut pipe = sepika_core::section_shape::SectionShape::SteelPipe {
            outer_dia: 400.0,
            thick: 12.0,
        }
        .to_section(SectionId(1), "鋼管大梁".into());
        pipe.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        pipe.material = app.core.model.sections[1].material;
        app.core.model.sections[1] = pipe;
        app.core.model.panel_zone = sepika_core::model::PanelZoneMode::Model;
        app.core
            .model
            .load_cases
            .push(sepika_core::model::LoadCase {
                id: LoadCaseId(2),
                name: "未設定の風荷重".into(),
                kind: sepika_core::model::LoadCaseKind::Wind,
                origin: Default::default(),
                nodal: Vec::new(),
                member: Vec::new(),
            });
        app.core
            .model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "空水平力の診断".into(),
                terms: vec![(LoadCaseId(2), 1.0)],
            });
        let s = app
            .create_study("複数警告の確認".into(), vec![ElemId(2)])
            .unwrap();
        app.start_study_calculation(s, 0).unwrap();
        wait(&mut app);
        let run = &app.studies.items[s].runs[0];
        let warnings = run
            .log
            .iter()
            .filter(|e| {
                e.level == LogLevel::Notice
                    && e.message.contains("パネルせん断変形をモデル化できません")
            })
            .map(|e| e.message.clone())
            .collect::<Vec<_>>();
        assert!(
            warnings.len() >= 2,
            "保持したログ: {:?}",
            run.log.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        assert!(warnings.iter().any(|m| m.contains("接合部 ID 2")));
        assert!(warnings.iter().any(|m| m.contains("接合部 ID 3")));
        let log = bytes(&run.log);
        let diagnostics = bytes(&run.diagnostics);
        assert!(run
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagSeverity::Warning && d.message.contains("未設定の風荷重")));
        let path = std::env::temp_dir().join(format!(
            "sepika-study-warnings-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.switch_study(Location::History(s, 0)).unwrap();
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        assert_eq!(bytes(&loaded.studies.items[s].runs[0].log), log);
        assert_eq!(
            bytes(&loaded.studies.items[s].runs[0].diagnostics),
            diagnostics
        );
        loaded.switch_study(Location::History(s, 0)).unwrap();
        assert_eq!(bytes(&loaded.core.log.entries), log);
        assert_eq!(bytes(&loaded.core.scoped.diagnostics), diagnostics);
        assert!(!loaded.core.scoped.staleness.diagnostics_stale);
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(4000.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| loaded.core.log.show(ui),
        );
        let rendered = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for warning in warnings {
            assert!(rendered.contains(&warning), "表示されない警告: {warning}");
        }
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(feature = "gui")]
    #[test]
    fn saved_design_references_retain_only_their_recomputed_evidence_across_selection_save_and_reload(
    ) {
        fn assert_popup(app: &App, reference: &references::ResultReference) {
            let Some(Location::History(s, r)) = app.study_result_location(reference) else {
                panic!("参照根拠がない")
            };
            let (input, bundle, _) = app.studies.items[s].runs[r]
                .referenced_result(reference)
                .unwrap();
            let before = bytes(&app.core.scoped.results);
            let generation = app.input_reference();
            let evidence = bytes(&app.studies.items[s].runs[r]);
            let popup = app.recorded_result_input(reference).unwrap();
            assert_eq!(bytes(&popup.model), bytes(&input.model));
            assert_eq!(
                bytes(&app.result_input_snapshot(reference).unwrap()),
                bytes(&input.model)
            );
            match popup.settings {
                references::RecordedSettings::Design {
                    short_term,
                    rc_damage_control,
                    bond_method,
                    qd_method,
                    source,
                } => {
                    assert_eq!(short_term, input.settings.design_term == LoadTerm::Short);
                    assert_eq!(
                        rc_damage_control,
                        input.settings.analysis_cfg.rc_damage_control
                    );
                    assert_eq!(bond_method, input.settings.analysis_cfg.bond_method);
                    assert_eq!(qd_method, input.settings.analysis_cfg.qd_method);
                    assert_eq!(source.map(|(key, _)| key), bundle.design_source);
                }
                _ => panic!("検定条件ではない"),
            }
            assert_eq!(bytes(&app.core.scoped.results), before);
            assert_eq!(app.input_reference(), generation);
            assert_eq!(bytes(&app.studies.items[s].runs[r]), evidence);
        }
        let mut app = app();
        let s = app
            .create_study("検定参照".into(), vec![ElemId(2)])
            .unwrap();
        app.start_study_calculation(s, 0).unwrap();
        wait(&mut app);
        let original = bytes(&app.studies.items[s].runs[0].results);
        app.switch_study(Location::History(s, 0)).unwrap();
        let key = StaticKey::Case(StaticCaseKey::User(LoadCaseId(0)));
        app.select_member(ElemId(2));
        app.freeze_selected_references().unwrap();
        let original_reference = app
            .result_reference(
                app.core.scoped.references.fixed[0].clone(),
                ResultInputKey::Design,
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        app.core
            .scoped
            .references
            .fixed_results
            .push(original_reference.clone());
        app.core.scoped.staleness.mark_non_calc_edited();
        app.retain_study_view_references();
        let mut saved = vec![original_reference];
        for _ in 0..2 {
            app.select_displayed_result(key);
            app.select_member(ElemId(2));
            app.freeze_selected_references().unwrap();
            let target = app.core.scoped.references.fixed[0].clone();
            let reference = app
                .result_reference(
                    target,
                    ResultInputKey::Design,
                    app.current_display_conditions().unwrap(),
                )
                .unwrap();
            app.core
                .scoped
                .references
                .fixed_results
                .push(reference.clone());
            app.core.scoped.staleness.mark_non_calc_edited();
            app.retain_study_view_references();
            saved.push(reference);
        }
        let static_reference = app
            .result_reference(
                app.core.scoped.references.fixed[0].clone(),
                ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0))),
                app.current_display_conditions().unwrap(),
            )
            .unwrap();
        app.core
            .scoped
            .references
            .fixed_results
            .push(static_reference.clone());
        app.core.scoped.staleness.mark_non_calc_edited();
        app.retain_study_view_references();
        assert_ne!(saved[0].run, saved[1].run);
        assert_eq!(
            app.studies.items[s].runs[0]
                .result_view_references
                .evidence
                .len(),
            2
        );
        assert!(app.resolve_result_reference(&saved[0]).is_ok());
        app.switch_study(Location::Main).unwrap();
        for reference in &saved {
            assert_popup(&app, reference);
        }
        app.switch_study(Location::History(s, 0)).unwrap();
        for reference in [&saved[0], &saved[1], &saved[0], &saved[2]] {
            assert_popup(&app, reference);
            assert!(app.resolve_result_reference(reference).is_ok());
            app.show_result_reference(reference, references::ReferenceView::Table, true)
                .unwrap();
            assert_eq!(
                app.selected_design_source(),
                app.core.scoped.results.as_ref().unwrap().design_source
            );
            assert_eq!(app.ui.scoped.nav.focus_result, app.core.scoped.last_static);
            assert_eq!(
                app.active_viewport().static_key,
                app.core.scoped.last_static
            );
            assert!(app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .reference_runs
                .iter()
                .any(|(key, id)| *key == ResultInputKey::Design && *id == reference.run));
        }
        app.show_result_reference(&static_reference, references::ReferenceView::Table, true)
            .unwrap();
        assert_eq!(app.core.scoped.last_static, Some(key));
        assert_eq!(app.ui.scoped.nav.focus_result, Some(key));
        assert_eq!(app.active_viewport().static_key, Some(key));
        app.show_result_reference(&saved[0], references::ReferenceView::Table, true)
            .unwrap();
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .reference_runs
            .iter()
            .any(|(key, id)| *key == ResultInputKey::Design && *id == saved[0].run));
        app.select_displayed_result(key);
        let path = std::env::temp_dir().join(format!(
            "sepika-study-design-reference-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        assert!(!app.project_unsaved());
        assert_eq!(
            app.studies.items[s].runs[0]
                .result_view_references
                .evidence
                .len(),
            2
        );
        assert_eq!(bytes(&app.studies.items[s].runs[0].results), original);
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        assert_eq!(
            loaded.studies.items[s].runs[0]
                .result_view_references
                .evidence
                .len(),
            2
        );
        for reference in [&saved[0], &saved[1], &saved[0], &saved[2]] {
            assert_popup(&loaded, reference);
            assert!(loaded.resolve_result_reference(reference).is_ok());
            loaded
                .show_result_reference(reference, references::ReferenceView::Table, true)
                .unwrap();
            assert_eq!(
                loaded.selected_design_source(),
                loaded.core.scoped.results.as_ref().unwrap().design_source
            );
            assert_eq!(
                loaded.ui.scoped.nav.focus_result,
                loaded.core.scoped.last_static
            );
            assert_eq!(
                loaded.active_viewport().static_key,
                loaded.core.scoped.last_static
            );
            assert!(loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .reference_runs
                .iter()
                .any(|(key, id)| *key == ResultInputKey::Design && *id == reference.run));
            let (source, input) = loaded
                .calculation_snapshot(&ResultInputKey::Design)
                .unwrap();
            assert_eq!(source, reference.source);
            assert_eq!(bytes(&input), bytes(&loaded.core.model));
        }
        loaded
            .show_result_reference(&static_reference, references::ReferenceView::Table, true)
            .unwrap();
        assert_eq!(loaded.core.scoped.last_static, Some(key));
        assert_eq!(loaded.ui.scoped.nav.focus_result, Some(key));
        assert_eq!(loaded.active_viewport().static_key, Some(key));
        loaded
            .show_result_reference(&saved[0], references::ReferenceView::Table, true)
            .unwrap();
        assert!(loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .reference_runs
            .iter()
            .any(|(key, id)| *key == ResultInputKey::Design && *id == saved[0].run));
        assert_eq!(bytes(&loaded.studies.items[s].runs[0].results), original);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejecting_a_main_background_request_while_study_runs_preserves_main_results() {
        let mut app = app();
        app.run_linear_static(LoadCaseId(0));
        assert!(app.core.scoped.results.is_some());
        let before = bytes(&app.core.scoped.results);
        let s = app
            .create_study("二重要求".into(), vec![ElemId(2)])
            .unwrap();
        app.start_study_calculation(s, 0).unwrap();
        app.start_linear_static_job(LoadCaseId(0));
        assert!(app
            .core
            .scoped
            .last_error
            .as_deref()
            .unwrap()
            .contains("解析実行中"));
        assert!(app.core.scoped.job.is_none());
        assert_eq!(bytes(&app.core.scoped.results), before);
        app.run_linear_static(LoadCaseId(0));
        assert_eq!(bytes(&app.core.scoped.results), before);
        wait(&mut app);
        assert_eq!(bytes(&app.core.scoped.results), before);
    }
}
