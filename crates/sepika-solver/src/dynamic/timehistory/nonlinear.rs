//! 非線形時刻歴応答解析（Newmark-β + Newton 反復 + commit/rollback）。
//!
//! - [`NonlinearThCfg`] — 解析設定（Newton 収束条件・幾何剛性・長期荷重初期化・記録間引き）
//! - [`nonlinear_time_history_analysis`] — 非線形時刻歴応答解析

use super::common::{
    accel_dir_at, accel_xy_at, empty_response, reduced_vec_from, resolve_dt, solve_initial_accel,
    sparse_matvec_into, GroundInfluence, NewmarkCoeffs,
};
use super::config::{GroundMotion, NewmarkCfg};
use super::history::{
    choose_record_dir_y, pick_record_node, record_history_step, total_mass, update_story_drift,
};
use super::recording::{member_forces_nonlinear, ThRecorder};
use super::result::{ResponseHistory, ResponseResult};
use crate::common::assemble::assemble_global_m;
use crate::common::constraint::Reducer;
use crate::common::csc_cache::{CscCache, WeightedSumGuard};
use crate::common::elem_loop::apply_du_trial;
use crate::common::elem_loop::apply_du_trial_with_member_loads;
use crate::common::linear_load::prepare_nonlinear_loads;
use crate::common::newton::{l2_norm, NewtonCriteria, STATIC_NEWTON};
use crate::common::tangent::{
    add_support_spring_f_int, assemble_k, assemble_k_cached_ref, compute_f_int,
};
use crate::common::transaction::StateSnapshot;
use crate::dynamic::damping::{Damping, DampingAccumulation};
use sepika_core::dof::DofMap;
use sepika_core::model::Model;
use sepika_element::behavior::{ElementBehavior, MassOption};
use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};
use sepika_math::solver::{make_solver, LinearSolver, SolveError, SolverBackend};
use sepika_math::sparse::{sparse_matvec, Triplet};

/// 非線形時刻歴応答解析の設定（Newton 収束条件・幾何剛性・長期荷重初期化・記録間引き）。
#[derive(Clone, Copy, Debug)]
pub struct NonlinearThCfg {
    /// 各時刻ステップの Newton 反復の収束規約（反復上限・相対許容誤差）。
    /// 基準ノルムは長期荷重を除いた動的外力ノルムと 1.0 の大きい方。
    pub newton: NewtonCriteria,
    /// 幾何剛性（P-Δ、Kg）を接線剛性に含めるか。
    pub use_kg: bool,
    /// 時刻歴開始前に長期荷重（固定・積載等、`LoadCaseKind::is_long_term`）を
    /// 静的 Newton 反復で載荷し、その変位・応力状態を時刻歴の初期条件とするか
    /// （プッシュオーバーの長期載荷フェーズと同じ考え方）。長期系荷重ケースが
    /// ないモデルでは何もしない。
    pub apply_long_term: bool,
    /// 詳細記録（[`super::ThRecording`]）の間引き係数。`None` は自動決定
    /// （記録フレーム数が概ね 1000 になるよう `super::recording::auto_record_every` で調整）。
    pub record_every: Option<usize>,
}

impl NonlinearThCfg {
    /// 既定値: 長期荷重初期化あり・幾何剛性なし・記録間引きは自動決定。
    /// Newton 反復の上限・相対許容誤差のみ呼び出し側で指定する。
    pub fn new(max_iter: usize, tol: f64) -> Self {
        Self {
            newton: NewtonCriteria::new(max_iter, tol),
            use_kg: false,
            apply_long_term: true,
            record_every: None,
        }
    }
}

impl Default for NonlinearThCfg {
    /// Newton 反復 20 回・相対許容誤差 1e-6・長期荷重初期化あり・幾何剛性なし・
    /// 記録間引きは自動決定。
    fn default() -> Self {
        Self::new(20, 1e-6)
    }
}

/// 非線形時刻歴応答解析（Newmark-β + Newton反復 + commit/rollback）。
///
/// 各時刻ステップで Newton 反復により内力（非線形復元力）と慣性力・減衰力・
/// 地震外力の動的釣合いを満たす解を求める。収束時は要素状態を commit、
/// 未収束・非有限応答時は時刻ステップ開始状態へ復元し、時刻・ステップ・理由付きエラーで停止する。
/// `cfg.apply_long_term` が真の場合、時刻歴開始前に長期荷重を静的載荷し
/// （プッシュオーバーの長期載荷フェーズと同じ考え方）、その変位・応力状態を初期条件とする。
/// `initial_disp`/`initial_vel` は「長期解からの増分」としての動的初期条件で、
/// 記録される変位は長期変位を含む全変位である。
/// 初期剛性を基準とする減衰は、長期載荷・動的初期条件の適用前の剛性を全時刻で用いる。
#[allow(clippy::too_many_arguments)]
pub fn nonlinear_time_history_analysis(
    model: &Model,
    dofmap: &DofMap,
    reducer: &Reducer,
    wave: &GroundMotion,
    newmark: &NewmarkCfg,
    damping: &Damping,
    accumulation: DampingAccumulation,
    initial_disp: &[f64],
    initial_vel: &[f64],
    cfg: NonlinearThCfg,
) -> Result<ResponseResult, SolveError> {
    crate::common::assemble::validate_global_mass_inputs(model)?;
    sepika_math::parallelism::apply_to_faer();

    sepika_element::factory::ensure_nonlinear_input_with_basis(
        model,
        sepika_core::model::AnalysisKind::TimeHistory,
        sepika_element::factory::StrengthBasis::Nominal,
    )
    .map_err(SolveError::InvalidInput)?;
    let active_cases: Vec<_> = model
        .load_cases
        .iter()
        .filter(|case| cfg.apply_long_term && case.kind.is_long_term())
        .map(|case| case.id)
        .collect();
    sepika_element::factory::ensure_rc_beam_reference_loads(model, &active_cases)
        .map_err(SolveError::InvalidInput)?;

    let dt = resolve_dt(newmark.dt, wave)?;

    let n_indep = reducer.n_indep;
    if model.nodes.is_empty() && model.elements.is_empty() {
        return Ok(empty_response(model, true, cfg.apply_long_term));
    }

    let wall_capacities = crate::nonlinear::wall_reference::resolve(
        model,
        cfg.apply_long_term,
        StrengthBasis::Nominal,
    )
    .map_err(SolveError::InvalidInput)?;
    let mut behaviors = build_behaviors(model, &wall_capacities);
    for b in behaviors.iter_mut() {
        b.set_time_step(dt);
    }
    let mut peak_force_scale = 0.0_f64;
    let mut mu_hist: Vec<Vec<f64>> = vec![Vec::new(); model.elements.len()];

    let m_free = assemble_global_m(model, dofmap, MassOption::Consistent)?;
    let m_red = reducer.reduce_k(&m_free);

    let n_free = dofmap.n_active();
    let infl = GroundInfluence::build(model, dofmap)?;
    let m_r_x: &[f64] = &infl.m_r_x;
    let m_r_y: &[f64] = &infl.m_r_y;

    let NewmarkCoeffs {
        c1,
        c2,
        c3,
        c4,
        c5,
        c6,
        ..
    } = NewmarkCoeffs::new(newmark, dt);

    let gravity_case = sepika_core::model::LoadCase {
        origin: Default::default(),
        id: sepika_core::ids::LoadCaseId(0),
        name: "時刻歴の長期荷重".into(),
        kind: sepika_core::model::LoadCaseKind::Other,
        nodal: model
            .load_cases
            .iter()
            .filter(|case| active_cases.contains(&case.id))
            .flat_map(|case| case.nodal.iter().cloned())
            .collect(),
        member: model
            .load_cases
            .iter()
            .filter(|case| active_cases.contains(&case.id))
            .flat_map(|case| case.member.iter().cloned())
            .collect(),
    };
    let (f0_free, gravity_member_loads) =
        prepare_nonlinear_loads(model, dofmap, &behaviors, &gravity_case)
            .map_err(SolveError::InvalidInput)?;
    let f0_red = reducer.reduce_f(&f0_free);
    let has_long_term = !gravity_case.nodal.is_empty() || !gravity_case.member.is_empty();

    let k_initial_free = assemble_k(model, dofmap, &behaviors, cfg.use_kg);
    let k_initial_red = reducer.reduce_k(&k_initial_free);
    let c_red = damping.assemble_c(&m_red, &k_initial_red);

    let mut u = vec![0.0; n_indep];
    if has_long_term {
        apply_long_term_static(
            model,
            dofmap,
            reducer,
            &mut behaviors,
            &f0_red,
            &gravity_member_loads,
            cfg.use_kg,
            &mut u,
        )?;
    }

    {
        let du_dyn = reduced_vec_from(n_indep, initial_disp);
        for i in 0..n_indep {
            u[i] += du_dyn[i];
        }
        let du_free = reducer.expand_u(&du_dyn);
        apply_du_trial(model, dofmap, &mut behaviors, &du_free)
            .map_err(SolveError::InvalidInput)?;
        for b in behaviors.iter_mut() {
            b.commit_state();
        }
    }

    let mut v = reduced_vec_from(n_indep, initial_vel);

    let mut f_damp = sparse_matvec(&c_red, &v);

    let u_mode1: Vec<f64> = if matches!(damping, Damping::TangentStiffnessConstantH { .. }) {
        crate::dynamic::eigen::solve_eigen(model, dofmap, reducer, 1)
            .ok()
            .and_then(|modal| modal.shapes.into_iter().next())
            .filter(|s| s.len() == n_indep)
            .unwrap_or_else(|| vec![0.0; n_indep])
    } else {
        vec![0.0; n_indep]
    };

    let p_red_0 = reducer.reduce_f(&infl.force_at(wave, 0));

    let mut f_int0_free = compute_f_int(model, dofmap, &behaviors);
    {
        let u0_free = reducer.expand_u(&u);
        add_support_spring_f_int(model, dofmap, &u0_free, &mut f_int0_free);
    }
    let f_int0_red = reducer.reduce_f(&f_int0_free);
    let mut rhs_a0 = vec![0.0; n_indep];
    for i in 0..n_indep {
        rhs_a0[i] = p_red_0[i] + f0_red[i] - f_damp[i] - f_int0_red[i];
    }
    let mut a = if n_indep == 0 {
        Vec::new()
    } else {
        solve_initial_accel(&m_red, &rhs_a0, n_indep)?
    };

    let n_steps = wave.accel_x.len() - 1;
    let mut u_free = vec![0.0f64; n_free];
    let mut v_free = vec![0.0f64; n_free];
    let mut a_free = vec![0.0f64; n_free];
    reducer.expand_u_into(&u, &mut u_free);
    reducer.expand_u_into(&v, &mut v_free);
    reducer.expand_u_into(&a, &mut a_free);

    let mut peak_disp_free = vec![0.0f64; n_free];
    for i in 0..n_free {
        peak_disp_free[i] = peak_disp_free[i].max(u_free[i].abs());
    }
    let mut story_drift_angle = vec![0.0f64; model.layer_count()];
    update_story_drift(model, dofmap, &u_free, &mut story_drift_angle);

    let record_dir_y = choose_record_dir_y(wave);
    let dir_idx = if record_dir_y { 1 } else { 0 };
    let m_r_record: &[f64] = if record_dir_y { m_r_y } else { m_r_x };
    let mut history = ResponseHistory {
        node: pick_record_node(model, dofmap, dir_idx),
        record_dir_y,
        ..Default::default()
    };
    let rmr_record = total_mass(m_r_record, dofmap, model.nodes.len(), dir_idx);
    let mut ma_free = vec![0.0f64; n_free];
    sparse_matvec_into(&m_free, &a_free, &mut ma_free);
    {
        let xg_init = accel_dir_at(wave, 0, record_dir_y);
        record_history_step(
            &mut history,
            model,
            dofmap,
            dir_idx,
            rmr_record,
            &u_free,
            &ma_free,
            xg_init,
        );
    }
    let mut time = Vec::with_capacity(n_steps + 1);
    time.push(0.0);

    let mut recorder = ThRecorder::new(model, dofmap, n_steps, cfg.record_every);
    {
        let (xg_x_init, xg_y_init) = accel_xy_at(wave, 0);
        let mf_init = member_forces_nonlinear(model, &behaviors)?;
        recorder.record_step(
            0, 0.0, model, dofmap, m_r_x, m_r_y, &ma_free, &u_free, &v_free, &a_free, xg_x_init,
            xg_y_init, &mf_init,
        );
    }

    let mut p_free_buf = vec![0.0f64; n_free];
    let mut p_dyn_red_buf = vec![0.0f64; n_indep];
    let mut p_red_buf = vec![0.0f64; n_indep];
    let mut u_trial_red_buf = vec![0.0f64; n_indep];
    let mut u_trial_free_buf = vec![0.0f64; n_free];
    let mut f_int_red_buf = vec![0.0f64; n_indep];
    let mut du_free_buf = vec![0.0f64; n_free];
    let mut dv_buf = vec![0.0f64; n_indep];
    let mut c_dv_buf = vec![0.0f64; n_indep];
    let mut c_v_red_buf = vec![0.0f64; n_indep];
    let mut m_a_red_buf = vec![0.0f64; n_indep];
    let mut r_red_buf = vec![0.0f64; n_indep];
    let mut du_red_buf = vec![0.0f64; n_indep];

    let mut k_eff_solver: Box<dyn LinearSolver> = make_solver(SolverBackend::DirectSparseCholesky);
    let mut k_t_free_cache = CscCache::new();
    let mut k_t_red_cache = CscCache::new();
    let mut k_eff_cache = WeightedSumGuard::new();
    let mut k_t_free_triplets_buf: Vec<Triplet> = Vec::new();
    let mut k_t_red_triplets_buf: Vec<Triplet> = Vec::new();

    for n in 0..n_steps {
        let t_next = (n + 1) as f64 * dt;
        let step_snapshot = StateSnapshot::capture(&behaviors);

        infl.force_at_into(wave, n + 1, &mut p_free_buf);
        reducer.reduce_f_into(&p_free_buf, &mut p_dyn_red_buf);
        let p_dyn_red = &p_dyn_red_buf;
        for i in 0..n_indep {
            p_red_buf[i] = p_dyn_red[i] + f0_red[i];
        }
        let p_red = &p_red_buf;

        let mut a_trial = vec![0.0; n_indep];
        let mut v_trial = vec![0.0; n_indep];
        for i in 0..n_indep {
            a_trial[i] = -c3 * v[i] - c4 * a[i];
            v_trial[i] = -c5 * v[i] - c6 * a[i];
        }

        let mut du_total = vec![0.0; n_indep];
        let mut converged = false;

        for _iter in cfg.newton.iters() {
            let (c_tan, k_t_red_precomputed) = if damping.is_tangent_based() {
                let k_t_free = assemble_k_cached_ref(
                    model,
                    dofmap,
                    &behaviors,
                    cfg.use_kg,
                    &mut k_t_free_cache,
                    &mut k_t_free_triplets_buf,
                );
                let k_t_red = reducer.reduce_k_cached_ref(
                    k_t_free,
                    &mut k_t_red_cache,
                    &mut k_t_red_triplets_buf,
                );
                let c = damping.assemble_c_tangent(&m_red, k_t_red, &k_initial_red, &u_mode1);
                (Some(c), Some(k_t_red))
            } else {
                (None, None)
            };
            let c_cur = c_tan.as_ref().unwrap_or(&c_red);

            let mut f_int_free = compute_f_int(model, dofmap, &behaviors);
            {
                for i in 0..n_indep {
                    u_trial_red_buf[i] = u[i] + du_total[i];
                }
                reducer.expand_u_into(&u_trial_red_buf, &mut u_trial_free_buf);
                add_support_spring_f_int(model, dofmap, &u_trial_free_buf, &mut f_int_free);
            }
            reducer.reduce_f_into(&f_int_free, &mut f_int_red_buf);
            let f_int_red = &f_int_red_buf;

            match accumulation {
                DampingAccumulation::NonCumulative => {
                    sparse_matvec_into(c_cur, &v_trial, &mut c_v_red_buf)
                }
                DampingAccumulation::Cumulative => {
                    for i in 0..n_indep {
                        dv_buf[i] = v_trial[i] - v[i];
                    }
                    sparse_matvec_into(c_cur, &dv_buf, &mut c_dv_buf);
                    for i in 0..n_indep {
                        c_v_red_buf[i] = f_damp[i] + c_dv_buf[i];
                    }
                }
            };
            let c_v_red = &c_v_red_buf;
            sparse_matvec_into(&m_red, &a_trial, &mut m_a_red_buf);
            let m_a_red = &m_a_red_buf;

            for i in 0..n_indep {
                r_red_buf[i] = p_red[i] - f_int_red[i] - c_v_red[i] - m_a_red[i];
            }
            let r_red = &r_red_buf;

            let r_norm = l2_norm(r_red);
            let scale = crate::common::newton::dynamic_force_scale(p_dyn_red, m_a_red, c_v_red);
            peak_force_scale = peak_force_scale.max(scale);
            let ref_norm = crate::common::newton::dynamic_reference_norm(scale, peak_force_scale);
            if !r_norm.is_finite()
                || !ref_norm.is_finite()
                || !du_total
                    .iter()
                    .chain(&v_trial)
                    .chain(&a_trial)
                    .all(|v| v.is_finite())
            {
                return Err(rollback_failed_step(
                    &mut behaviors,
                    &step_snapshot,
                    n + 1,
                    t_next,
                    "応答または釣合い残差に非有限値があります",
                ));
            }
            if cfg.newton.converged(r_norm, ref_norm) {
                converged = true;
                break;
            }

            let k_t_red = match k_t_red_precomputed {
                Some(k) => k,
                None => {
                    let k_t_free = assemble_k_cached_ref(
                        model,
                        dofmap,
                        &behaviors,
                        cfg.use_kg,
                        &mut k_t_free_cache,
                        &mut k_t_free_triplets_buf,
                    );
                    reducer.reduce_k_cached_ref(
                        k_t_free,
                        &mut k_t_red_cache,
                        &mut k_t_red_triplets_buf,
                    )
                }
            };
            let k_eff =
                k_eff_cache.combine_ref(n_indep, &[(1.0, k_t_red), (c2, c_cur), (c1, &m_red)]);
            k_eff_solver.factorize(k_eff).map_err(|e| {
                rollback_failed_step(
                    &mut behaviors,
                    &step_snapshot,
                    n + 1,
                    t_next,
                    &format!("接線剛性の分解失敗: {e:?}"),
                )
            })?;
            k_eff_solver
                .solve_into(r_red, &mut du_red_buf)
                .map_err(|e| {
                    rollback_failed_step(
                        &mut behaviors,
                        &step_snapshot,
                        n + 1,
                        t_next,
                        &e.to_string(),
                    )
                })?;
            let du_red = &du_red_buf;
            reducer.expand_u_into(du_red, &mut du_free_buf);
            let du_free = &du_free_buf;

            for i in 0..n_indep {
                a_trial[i] += c1 * du_red[i];
                v_trial[i] += c2 * du_red[i];
                du_total[i] += du_red[i];
            }

            if let Err(reason) = apply_du_trial(model, dofmap, &mut behaviors, du_free) {
                return Err(rollback_failed_step(
                    &mut behaviors,
                    &step_snapshot,
                    n + 1,
                    t_next,
                    &reason,
                ));
            }
        }

        if !converged {
            let finite = du_total
                .iter()
                .chain(&v_trial)
                .chain(&a_trial)
                .all(|v| v.is_finite());
            let reason = if finite {
                format!(
                    "全体釣合いが反復上限 {} 回までに収束しません",
                    cfg.newton.max_iter
                )
            } else {
                "応答に非有限値があり、全体釣合いが収束しません".to_owned()
            };
            return Err(rollback_failed_step(
                &mut behaviors,
                &step_snapshot,
                n + 1,
                t_next,
                &reason,
            ));
        }

        let mf_now =
            checked_step_member_forces(model, &mut behaviors, &step_snapshot, n + 1, t_next)?;
        {
            for i in 0..n_indep {
                u[i] += du_total[i];
            }
            if accumulation == DampingAccumulation::Cumulative {
                f_damp.clone_from(&c_v_red_buf);
            }
            v.copy_from_slice(&v_trial);
            a.copy_from_slice(&a_trial);

            for b in behaviors.iter_mut() {
                b.commit_state();
            }

            for (i, b) in behaviors.iter().enumerate() {
                if let Some(p) = b.ductility_probe() {
                    mu_hist[i].push(p.max_yield_ratio);
                }
            }

            time.push(t_next);

            reducer.expand_u_into(&u, &mut u_free);
            for i in 0..n_free {
                peak_disp_free[i] = peak_disp_free[i].max(u_free[i].abs());
            }
            update_story_drift(model, dofmap, &u_free, &mut story_drift_angle);
            reducer.expand_u_into(&v, &mut v_free);
            reducer.expand_u_into(&a, &mut a_free);
            sparse_matvec_into(&m_free, &a_free, &mut ma_free);
            let xg_next = accel_dir_at(wave, n + 1, record_dir_y);
            record_history_step(
                &mut history,
                model,
                dofmap,
                dir_idx,
                rmr_record,
                &u_free,
                &ma_free,
                xg_next,
            );

            let (xg_x_next, xg_y_next) = accel_xy_at(wave, n + 1);
            recorder.record_step(
                (n + 1) as u64,
                t_next,
                model,
                dofmap,
                m_r_x,
                m_r_y,
                &ma_free,
                &u_free,
                &v_free,
                &a_free,
                xg_x_next,
                xg_y_next,
                &mf_now,
            );
        }
    }

    let peak_disp = dofmap.expand_to_nodes(&peak_disp_free, model.nodes.len());

    let fatigue = crate::damage::FatigueParams::default();
    let cumulative_ductility: Vec<f64> = mu_hist
        .iter()
        .map(|series| crate::damage::cumulative_damage_rainflow(series, fatigue))
        .collect();

    Ok(ResponseResult {
        wall_capacities,
        rc_wall_shear_method: model.stress_cfg.rc_wall_shear_method,
        non_converged_steps: 0,
        time,
        peak_disp,
        story_drift_angle,
        cumulative_ductility,
        history,
        recording: Some(recorder.finish()),
        nonlinear: true,
        applied_long_term: cfg.apply_long_term,
    })
}

pub(super) fn checked_step_member_forces(
    model: &Model,
    behaviors: &mut [Box<dyn ElementBehavior>],
    snapshot: &StateSnapshot,
    step: usize,
    time_s: f64,
) -> Result<Vec<Option<sepika_element::frame::beam::MemberForces>>, SolveError> {
    member_forces_nonlinear(model, behaviors).map_err(|error| {
        rollback_failed_step(behaviors, snapshot, step, time_s, &error.to_string())
    })
}

fn rollback_failed_step(
    behaviors: &mut [Box<dyn ElementBehavior>],
    step_snapshot: &StateSnapshot,
    step: usize,
    time_s: f64,
    reason: &str,
) -> SolveError {
    step_snapshot.restore(behaviors);
    SolveError::Backend(format!(
        "非線形時刻歴・step {step}・時刻 {time_s} s: {reason}"
    ))
}

#[cfg(test)]
#[path = "tests/rollback.rs"]
mod rollback_tests;

/// 長期荷重（`f0_red`、縮約空間）を静的 Newton 反復で載荷する。
/// 全量の収束成功時のみ状態を確定し、変位を `u_out` に加算する。失敗時は載荷前へ復元する。
#[allow(clippy::too_many_arguments)]
fn apply_long_term_static(
    model: &Model,
    dofmap: &DofMap,
    reducer: &Reducer,
    behaviors: &mut [Box<dyn ElementBehavior>],
    f0_red: &[f64],
    gravity_member_loads: &[Vec<sepika_core::model::MemberLoad>],
    use_kg: bool,
    u_out: &mut [f64],
) -> Result<(), SolveError> {
    let snap = StateSnapshot::capture(behaviors);
    match newton_static_converge(
        model,
        dofmap,
        reducer,
        behaviors,
        f0_red,
        gravity_member_loads,
        use_kg,
        u_out,
    ) {
        Ok(Some(du)) => {
            for b in behaviors.iter_mut() {
                b.commit_state();
            }
            for (u, du) in u_out.iter_mut().zip(du) {
                *u += du;
            }
            Ok(())
        }
        result => {
            snap.restore(behaviors);
            let detail = match result {
                Err(error) => error.to_string(),
                _ => "Newton反復回数の上限に達しました".into(),
            };
            Err(SolveError::InvalidInput(format!(
                "長期荷重の初期載荷が収束しません: {detail}"
            )))
        }
    }
}

/// 固定外力 `f_target_red`（縮約空間）に対する静的 Newton 反復。
/// 収束時はステップ内の全修正量の累積を `Some` で返し、要素状態はトライアル反映済み・未確定のまま戻す。
/// 収束しなければ `Ok(None)`。
#[allow(clippy::too_many_arguments)]
fn newton_static_converge(
    model: &Model,
    dofmap: &DofMap,
    reducer: &Reducer,
    behaviors: &mut [Box<dyn ElementBehavior>],
    f_target_red: &[f64],
    absolute_loads: &[Vec<sepika_core::model::MemberLoad>],
    use_kg: bool,
    u_base_red: &[f64],
) -> Result<Option<Vec<f64>>, SolveError> {
    let n_indep = reducer.n_indep;
    let mut solver: Box<dyn LinearSolver> = make_solver(SolverBackend::DirectSparseCholesky);
    let mut k_free_cache = CscCache::new();
    let mut k_red_cache = CscCache::new();
    let mut du_red_buf = vec![0.0f64; n_indep];
    let mut k_free_triplets_buf: Vec<Triplet> = Vec::new();
    let mut k_red_triplets_buf: Vec<Triplet> = Vec::new();
    apply_du_trial_with_member_loads(
        model,
        dofmap,
        behaviors,
        &vec![0.0; dofmap.n_active()],
        absolute_loads,
    )
    .map_err(SolveError::InvalidInput)?;
    let mut du_total = vec![0.0; n_indep];
    for _iter in STATIC_NEWTON.iters() {
        let k_free = assemble_k_cached_ref(
            model,
            dofmap,
            behaviors,
            use_kg,
            &mut k_free_cache,
            &mut k_free_triplets_buf,
        );
        let k_red = reducer.reduce_k_cached_ref(k_free, &mut k_red_cache, &mut k_red_triplets_buf);
        let mut f_int_free = compute_f_int(model, dofmap, behaviors);
        {
            let u_trial_red: Vec<f64> = (0..n_indep).map(|i| u_base_red[i] + du_total[i]).collect();
            let u_trial_free = reducer.expand_u(&u_trial_red);
            add_support_spring_f_int(model, dofmap, &u_trial_free, &mut f_int_free);
        }
        let f_int_red = reducer.reduce_f(&f_int_free);
        let mut r_red = vec![0.0; n_indep];
        for i in 0..n_indep {
            r_red[i] = f_target_red[i] - f_int_red[i];
        }
        let r_norm = l2_norm(&r_red);
        let f_norm = l2_norm(f_target_red);
        if STATIC_NEWTON.converged(r_norm, f_norm.max(1.0)) {
            return Ok(Some(du_total));
        }
        solver
            .factorize(k_red)
            .map_err(|e| SolveError::Backend(format!("factor: {:?}", e)))?;
        solver.solve_into(&r_red, &mut du_red_buf)?;
        let du_free = reducer.expand_u(du_red_buf.as_slice());
        for i in 0..n_indep {
            du_total[i] += du_red_buf[i];
        }
        apply_du_trial(model, dofmap, behaviors, &du_free).map_err(SolveError::InvalidInput)?;
    }
    Ok(None)
}

fn build_behaviors(
    model: &Model,
    wall_capacities: &[(sepika_core::ids::ElemId, [f64; 2])],
) -> Vec<Box<dyn sepika_element::behavior::ElementBehavior>> {
    let mut behaviors = Vec::new();
    for elem in &model.elements {
        let b = build_nonlinear_behavior(
            elem,
            model,
            StrengthBasis::Nominal,
            sepika_element::factory::AnalysisKind::TimeHistory,
            wall_capacities
                .iter()
                .find(|(id, _)| *id == elem.id)
                .map(|(_, q)| *q),
        );
        behaviors.push(b);
    }
    behaviors
}
