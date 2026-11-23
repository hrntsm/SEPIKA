//! 静的解析のファサード [`Analysis`]。
//!
//! 基準剛性の分解を固有値・周期と共有し、静的解析はケース種別・応力設定を採用する。
//! 地震荷重の生成は `seismic`、設定型は `config`、入力検証は [`precheck`] が受け持つ。

use crate::common::assemble::assemble_global_k;
use crate::common::constraint::Reducer;
use crate::dynamic::damping::Damping;
use crate::dynamic::eigen::{self, ModalResult};
use crate::dynamic::timehistory::{GroundMotion, NewmarkCfg, ResponseResult};
use crate::statics::linear::{
    has_tension_only_brace, solve_static_case, uses_long_axial_stiffness, StaticOnce,
    StaticStiffness, TENSION_ONLY_MAX_ITER,
};

pub type StaticResult = StaticOnce;
use sepika_core::dof::{Dof, DofMap, DOF_PER_NODE};
use sepika_core::ids::{LoadCaseId, NodeId};
use sepika_core::model::{LoadCase, Model};
use sepika_element::factory::build_behavior;
use sepika_math::solver::{make_solver, LinearSolver, SolveError, SolverBackend};

mod combination;
mod config;
pub mod precheck;
mod seismic;

pub use combination::StaticBatch;
pub use config::{AiMode, SeismicCfg, SeismicDir};
pub use seismic::{
    base_elevation, build_seismic_load_case_from_model, building_height_mm, ground_elevation,
    seismic_distribution_for_model, steel_height_ratio, validate_basement_ground,
};

/// `model.elements` と同順の構築時 ID・弾性応答要素・自由度対応を返す。
fn build_behavior_cache(model: &Model, dofmap: &DofMap) -> Vec<crate::statics::BehaviorEntry> {
    model
        .elements
        .iter()
        .map(|elem| {
            let behavior = build_behavior(elem, model);
            let gdofs = behavior.global_dofs(dofmap);
            (elem.id, behavior, gdofs)
        })
        .collect()
}

pub struct Analysis<'m> {
    model: &'m Model,
    dofmap: DofMap,
    reducer: Reducer,
    solver: Box<dyn LinearSolver>,
    n_indep: usize,
    /// SemiPrecise の 1 次固有周期キャッシュ [s]。固有周期は載荷方向に依存しない
    /// ため、同一 `Analysis` 上で X・Y 両方向の地震荷重を構築しても固有値解析は
    /// 1 回で済む（`build_seismic_load_case`）。
    semi_precise_t: std::sync::OnceLock<f64>,
    /// `model.elements` と同順の構築時 ID・弾性応答要素・自由度対応。
    behavior_cache: Vec<crate::statics::BehaviorEntry>,
    long_stiffness: std::sync::OnceLock<Result<StaticStiffness, SolveError>>,
}

impl<'m> Analysis<'m> {
    /// 解析前にモデルの静的検証（`Model::validate` の不変条件・参照整合・拘束・
    /// 断面/材料割当・孤立節点）を行い、問題があればユーザー向けの日本語診断
    /// メッセージ付きでエラーを返す。
    pub fn prepare(model: &'m Model) -> Result<Self, SolveError> {
        sepika_math::parallelism::apply_to_faer();
        precheck::precheck_model(model)?;
        let dofmap = DofMap::build(model);
        let k_free = assemble_global_k(model, &dofmap);
        let reducer = Reducer::build(model, &dofmap);
        let n_indep = reducer.n_indep;
        let k_red = reducer.reduce_k(&k_free);

        let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
        if n_indep > 0 {
            solver.factorize(&k_red).map_err(|e| match e {
                SolveError::NotPositiveDefinite => {
                    SolveError::InvalidInput(precheck::singular_diagnosis(model))
                }
                other => other,
            })?;
        }

        let behavior_cache = build_behavior_cache(model, &dofmap);

        Ok(Self {
            model,
            dofmap,
            reducer,
            solver,
            n_indep,
            semi_precise_t: std::sync::OnceLock::new(),
            behavior_cache,
            long_stiffness: std::sync::OnceLock::new(),
        })
    }

    fn zero_result(&self) -> StaticOnce {
        StaticOnce {
            story_shears: Some(std::array::from_fn(|_| {
                vec![0.0; self.model.layers().len()]
            })),
            stiffness_ratio_disp: None,
            story_rigidity: Vec::new(),
            disp: vec![[0.0; 6]; self.model.nodes.len()],
            member_forces: Vec::new(),
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
            steel_panel_column_face_forces: Vec::new(),
            column_end_axial_forces: Vec::new(),
            column_end_horizontal_forces: Vec::new(),
        }
    }

    fn solve_and_recover(&self, load_case: &LoadCase) -> Result<StaticOnce, SolveError> {
        let iterative =
            self.model.stress_cfg.tension_only_iteration && has_tension_only_brace(self.model);
        let fixed = if iterative {
            None
        } else if uses_long_axial_stiffness(self.model, load_case.kind) {
            let stiffness = self.long_stiffness.get_or_init(|| {
                StaticStiffness::build(self.model, &self.dofmap, &self.reducer, load_case.kind)
            });
            let stiffness = stiffness.as_ref().map_err(Clone::clone)?;
            Some((stiffness.solver.as_ref(), stiffness.behaviors.as_slice()))
        } else {
            Some((self.solver.as_ref(), self.behavior_cache.as_slice()))
        };
        solve_static_case(
            self.model,
            &self.dofmap,
            &self.reducer,
            load_case,
            fixed,
            TENSION_ONLY_MAX_ITER,
        )
    }

    /// 不存在の荷重ケース ID は、拘束状態に関係なく ID を含む `SolveError::InvalidInput` を返す。
    pub fn linear_static(&self, lc: LoadCaseId) -> Result<StaticOnce, SolveError> {
        let load_case = self
            .model
            .load_cases
            .iter()
            .find(|c| c.id == lc)
            .ok_or_else(|| {
                SolveError::InvalidInput(format!("荷重ケース {} が存在しません", lc.0))
            })?;
        self.solve_and_recover(load_case)
    }

    /// 節点の Fx・Fy・Mz 各単位荷重に対する軸順 Ux・Uy・Rz の変位を返す（行が変位、列が荷重）。
    /// 基準弾性剛性を用いる。節点がない場合、3成分が独立自由度でない場合、荷重依存の剛性を使うモデルでは Err。
    pub fn nodal_planar_flexibility(&self, node: NodeId) -> Result<[[f64; 3]; 3], SolveError> {
        if self.model.node(node).is_none() {
            return Err(SolveError::InvalidInput(format!(
                "節点{}が存在しません",
                node.0
            )));
        }
        if self.model.stress_cfg.tension_only_iteration && has_tension_only_brace(self.model) {
            return Err(SolveError::InvalidInput(
                "引張専用ブレースの活動状態が加力によって変わります".into(),
            ));
        }
        let indices = [Dof::Ux, Dof::Uy, Dof::Rz].map(|dof| {
            let active =
                self.dofmap
                    .active(node.index() * DOF_PER_NODE + dof as usize)? as usize;
            let [(reduced, 1.0)] = self.reducer.t_rows[active].as_slice() else {
                return None;
            };
            (self.reducer.free_dof_of(*reduced) == Some(active)).then_some(*reduced)
        });
        let [Some(i), Some(j), Some(k)] = indices else {
            return Err(SolveError::InvalidInput(format!(
                "節点{}のUx・Uy・Rzが独立自由度ではありません",
                node.0
            )));
        };
        let indices = [i, j, k];
        let mut result = [[0.0; 3]; 3];
        for (column, load_index) in indices.into_iter().enumerate() {
            let mut load = vec![0.0; self.n_indep];
            load[load_index] = 1.0;
            let displacement = self.solver.solve(&load)?;
            for (row, index) in indices.into_iter().enumerate() {
                result[row][column] = displacement[index];
            }
        }
        Ok(result)
    }

    /// 準備済みの基準剛性の分解を再利用し、指定数の固有モードを返す。
    pub fn eigen(&self, n_modes: usize) -> Result<ModalResult, SolveError> {
        eigen::solve_eigen_with_solver(
            self.model,
            &self.dofmap,
            &self.reducer,
            n_modes,
            &*self.solver,
        )
    }

    /// 複数の荷重ケースを一括で解く。固定剛性は採用条件が一致するケースで共有する。
    ///
    /// 並列度設定（[`sepika_math::parallelism`]）が並列（`Auto`/`Threads`）の
    /// 場合は荷重ケース単位に rayon で並列実行する。各ケースの計算はケース間で
    /// 可変状態を共有しない（`&self` のみ）ため実行順に依存せず、結果の順序は
    /// 入力 `lcs` の順で固定される。`Deterministic`（既定）では
    /// 逐次実行し、`linear_static` を順に呼んだ場合とビット一致する。
    pub fn linear_static_batch(&self, lcs: &[LoadCaseId]) -> Vec<Result<StaticOnce, SolveError>> {
        self.run_batch(lcs, |lc| self.linear_static(*lc))
    }

    /// バッチ API の共通経路。並列設定時は項目単位に rayon で並列実行する。
    fn run_batch<T: Sync, R: Send>(
        &self,
        items: &[T],
        f: impl Fn(&T) -> R + Send + Sync,
    ) -> Vec<R> {
        use rayon::prelude::*;
        if sepika_math::parallelism::is_parallel() && items.len() > 1 {
            let cores = sepika_math::parallelism::effective_threads();
            let outer = items.len().min(cores);
            let inner = (cores / outer).max(1);
            if inner == 1 {
                faer::set_global_parallelism(faer::Par::Seq);
            } else {
                faer::set_global_parallelism(faer::Par::rayon(inner));
            }
            let out = sepika_math::parallelism::run(|| items.par_iter().map(f).collect());
            sepika_math::parallelism::apply_to_faer();
            out
        } else {
            items.iter().map(f).collect()
        }
    }

    /// 時刻歴応答解析（Newmark-β、減衰込み）。
    /// `record_every` は詳細記録（`ThRecording`）の間引き係数。`None` は自動決定。
    pub fn time_history(
        &self,
        wave: &GroundMotion,
        newmark: NewmarkCfg,
        damping: Damping,
        record_every: Option<usize>,
    ) -> Result<ResponseResult, sepika_math::solver::SolveError> {
        self.time_history_with_force_observer(wave, newmark, damping, record_every, None)
    }

    /// 詳細記録の間引きにかかわらず、初期点と各確定ステップの断面力を観測する。
    pub fn time_history_with_force_observer(
        &self,
        wave: &GroundMotion,
        newmark: NewmarkCfg,
        damping: Damping,
        record_every: Option<usize>,
        observer: Option<&mut crate::dynamic::timehistory::CommittedMemberForcesObserver<'_>>,
    ) -> Result<ResponseResult, sepika_math::solver::SolveError> {
        let init = vec![0.0; self.n_indep];
        crate::dynamic::timehistory::linear_time_history_until_with_force_observer(
            self.model,
            &self.dofmap,
            &self.reducer,
            wave,
            &newmark,
            &damping,
            &init,
            &init,
            record_every,
            None,
            observer,
        )
        .map(|(result, _)| result)
    }
}

#[cfg(test)]
mod tests;
