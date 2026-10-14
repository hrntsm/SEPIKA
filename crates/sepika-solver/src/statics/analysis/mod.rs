//! 静的解析のファサード [`Analysis`]。
//!
//! 基準剛性の分解を固有値・周期と共有し、静的解析はケース種別・応力設定を採用する。
//! 地震荷重の生成は `seismic`、設定型は `config`、入力検証は [`precheck`] が受け持つ。

use crate::common::assemble::{assemble_global_f, assemble_global_k};
use crate::common::constraint::Reducer;
use crate::dynamic::damping::Damping;
use crate::dynamic::eigen::{self, ModalResult};
use crate::dynamic::timehistory::{GroundMotion, NewmarkCfg, ResponseResult};
use crate::statics::linear::{
    has_tension_only_brace, solve_static_case, uses_long_axial_stiffness, StaticOnce,
    StaticStiffness, TENSION_ONLY_MAX_ITER,
};
use std::collections::HashMap;

pub type StaticResult = StaticOnce;
use sepika_core::dof::DofMap;
use sepika_core::ids::LoadCaseId;
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

/// `model.load_cases` 全件の自由 DOF 荷重ベクトルを1回ずつ計算してマップに詰める
/// （[`Analysis::f_free_cache`] の構築。`prepare` から使う）。
fn build_f_free_cache(model: &Model, dofmap: &DofMap) -> HashMap<LoadCaseId, Vec<f64>> {
    model
        .load_cases
        .iter()
        .map(|lc| (lc.id, assemble_global_f(model, dofmap, lc.id)))
        .collect()
}

/// `model.elements` 全件の `(ElementBehavior, global_dofs)` を1回だけ構築する
/// （[`Analysis::behavior_cache`] の構築。`prepare` から使う）。
/// 静解析経路（[`build_behavior`]）は常に弾性要素を返す
/// （履歴状態を持たない）ため、`&self` から複数回・複数スレッドで参照しても
/// 安全（[`ElementBehavior`] の `Send + Sync` supertrait 経由）。
fn build_behavior_cache(model: &Model, dofmap: &DofMap) -> Vec<crate::statics::BehaviorEntry> {
    model
        .elements
        .iter()
        .map(|elem| {
            let behavior = build_behavior(elem, model);
            let gdofs = behavior.global_dofs(dofmap);
            (behavior, gdofs)
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
    /// `Model::load_cases` 各ケースの自由 DOF 荷重ベクトル（`assemble_global_f`）の
    /// メモ化。`&self` のみで参照する
    /// （書き込みは `prepare` 構築時のみ）ため、`run_batch` の rayon
    /// 並列からも安全に共有できる。
    f_free_cache: HashMap<LoadCaseId, Vec<f64>>,
    /// `model.elements` 各要素の `(ElementBehavior, global_dofs)` のメモ化
    /// （[`build_behavior_cache`]）。`f_free_cache` と同様、書き込みは構築時のみで
    /// `run_batch` の rayon 並列からも安全に共有できる（`ElementBehavior: Send + Sync`）。
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
        let n_active = dofmap.n_active();

        if n_active == 0 {
            return Ok(Self {
                model,
                dofmap,
                reducer: Reducer::empty(),
                solver: make_solver(SolverBackend::DirectSparseCholesky),
                n_indep: 0,
                semi_precise_t: std::sync::OnceLock::new(),
                f_free_cache: HashMap::new(),
                behavior_cache: Vec::new(),
                long_stiffness: std::sync::OnceLock::new(),
            });
        }

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

        let f_free_cache = build_f_free_cache(model, &dofmap);
        let behavior_cache = build_behavior_cache(model, &dofmap);

        Ok(Self {
            model,
            dofmap,
            reducer,
            solver,
            n_indep,
            semi_precise_t: std::sync::OnceLock::new(),
            f_free_cache,
            behavior_cache,
            long_stiffness: std::sync::OnceLock::new(),
        })
    }

    /// 全自由度ゼロの結果（有効自由度なしのモデル用）。
    fn zero_result(&self) -> StaticOnce {
        StaticOnce {
            disp: vec![[0.0; 6]; self.model.nodes.len()],
            member_forces: Vec::new(),
            panel_moments: Vec::new(),
        }
    }

    fn solve_and_recover(
        &self,
        load_case: &LoadCase,
        f_free: &[f64],
    ) -> Result<StaticOnce, SolveError> {
        let iterative =
            self.model.stress_cfg.tension_only_iteration && has_tension_only_brace(self.model);
        let fixed = if iterative || self.n_indep == 0 {
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
            f_free,
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
        if self.n_indep == 0 {
            return Ok(self.zero_result());
        }
        self.solve_and_recover(load_case, &self.f_free_cache[&lc])
    }

    /// 準備済みの基準剛性の分解を再利用し、指定数の固有モードを返す。
    pub fn eigen(&self, n_modes: usize) -> Result<ModalResult, SolveError> {
        self.eigen_solver_dispatch(n_modes)
    }

    pub(crate) fn eigen_solver_dispatch(&self, n_modes: usize) -> Result<ModalResult, SolveError> {
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
        let n_indep = self.n_indep;
        let init = vec![0.0; n_indep];
        crate::dynamic::timehistory::linear_time_history_analysis(
            self.model,
            &self.dofmap,
            &self.reducer,
            wave,
            &newmark,
            &damping,
            &init,
            &init,
            false,
            record_every,
        )
    }

    /// LoadCase の節点荷重リストから自由 DOF 空間の荷重ベクトルを組み立てる
    /// （地震荷重・風荷重など静的荷重ケースの共通処理）。
    fn assemble_f_free_from_nodal(&self, nodal: &[sepika_core::model::NodalLoad]) -> Vec<f64> {
        let n_active = self.dofmap.n_active();
        let mut f_free = vec![0.0; n_active];
        for nodal_load in nodal {
            let ni = nodal_load.node.index();
            for d in 0..sepika_core::dof::DOF_PER_NODE {
                let g = ni * sepika_core::dof::DOF_PER_NODE + d;
                if let Some(active) = self.dofmap.active(g) {
                    f_free[active as usize] += nodal_load.values[d];
                }
            }
        }
        f_free
    }
}

#[cfg(test)]
mod tests;
