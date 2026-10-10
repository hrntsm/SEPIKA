//! 各解析の純粋計算（所有モデル＋解析条件 → 結果）。
//!
//! いずれも `&self` を取らない自由関数で、GUI の状態に触れない。
//! 渡すモデルは [`crate::prepare::prepare_model_for_analysis`] で前処理済み
//! （剛域・仕口パネル・DL/LL/EX/EY 同期）であること。
//! 壁の解析要素は入力の `Model` に存在しない生成物のため、各公開関数は
//! 受け取った `model` を壁展開してから解く（呼び出し側に展開を要求しない）。

use crate::error::{JobError, JobResult};
use crate::settings::{AnalysisSettings, ThDampingModel};
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadSource, EX_CASE_NAME, EY_CASE_NAME};
use sepika_solver::statics::analysis::Analysis;

/// 壁展開モデルを組み立てる（本モジュール共通のエントリポイント）。
fn expand_walls(model: sepika_core::model::Model) -> sepika_core::model::Model {
    sepika_load::wall_expand::expand_wall_elements_owned(model).0
}

fn dynamic_solve_error(error: sepika_math::solver::SolveError) -> JobError {
    match error {
        sepika_math::solver::SolveError::InvalidInput(message) => JobError::InvalidInput(message),
        other => JobError::Solve(other.to_string()),
    }
}

#[cfg(test)]
#[test]
fn 動的solverの入力不備は公開ジョブでも入力不備の分類を維持する() {
    let error = dynamic_solve_error(sepika_math::solver::SolveError::InvalidInput(
        "質量不備".into(),
    ));
    assert!(matches!(error, JobError::InvalidInput(message) if message == "質量不備"));
    assert!(matches!(
        dynamic_solve_error(sepika_math::solver::SolveError::NotPositiveDefinite),
        JobError::Solve(_)
    ));
}

/// 標準 EX/EY に再生成済みの Auto 水平力が欠けているか。
pub fn missing_seismic_horizontal_load(case: &LoadCase) -> bool {
    let axis = match (case.name.as_str(), case.kind) {
        (EX_CASE_NAME, LoadCaseKind::Seismic) => 0,
        (EY_CASE_NAME, LoadCaseKind::Seismic) => 1,
        _ => return false,
    };
    !case
        .nodal
        .iter()
        .any(|load| load.source == LoadSource::Auto && load.values[axis] != 0.0)
        && !case.member.iter().any(|load| {
            load.source == LoadSource::Auto
                && load.dir[axis] != 0.0
                && match load.kind {
                    sepika_core::model::MemberLoadKind::Point { p, .. } => p != 0.0,
                    sepika_core::model::MemberLoadKind::Distributed { w1, w2, .. } => {
                        w1 != 0.0 || w2 != 0.0
                    }
                }
        })
}

/// 線形静的解析。前処理を通したモデルを渡すこと。
pub fn compute_linear_static(
    model: sepika_core::model::Model,
    lc: LoadCaseId,
) -> JobResult<sepika_solver::statics::linear::StaticOnce> {
    if let Some(case) = model.load_cases.iter().find(|case| case.id == lc) {
        if missing_seismic_horizontal_load(case) {
            return Err(JobError::InvalidInput(format!(
                "荷重ケース「{}」の Ai 地震水平力を再生成できていません。準備計算の条件を修正し、Ai 地震力を再生成してください（手入力水平力だけでは解析できません）。",
                case.name
            )));
        }
    }
    let model = expand_walls(model);
    match Analysis::prepare(&model) {
        Ok(analysis) => analysis
            .linear_static(lc)
            .map_err(|e| JobError::Solve(format!("{e:?}"))),
        Err(e) => Err(JobError::Prepare(format!("{e:?}"))),
    }
}

/// 固有値解析。前処理を通したモデルを渡すこと。
pub fn compute_eigen(
    model: sepika_core::model::Model,
    n_modes: usize,
) -> JobResult<sepika_solver::dynamic::eigen::ModalResult> {
    model
        .validate_damper_mass_placement()
        .map_err(JobError::InvalidInput)?;
    let model = expand_walls(model);
    match Analysis::prepare(&model) {
        Ok(analysis) => analysis.eigen(n_modes).map_err(dynamic_solve_error),
        Err(e) => Err(JobError::Prepare(format!("{e:?}"))),
    }
}

/// 地震静的（Ai 分布）解析。前処理を通したモデルを渡すこと。
pub fn compute_seismic(
    model: sepika_core::model::Model,
    cfg: sepika_solver::statics::analysis::SeismicCfg,
    t: f64,
) -> JobResult<sepika_solver::statics::linear::StaticOnce> {
    let model = expand_walls(model);
    match Analysis::prepare(&model) {
        Ok(analysis) => analysis
            .seismic_static_with_period(cfg, t)
            .map_err(|e| JobError::Solve(format!("{e:?}"))),
        Err(e) => Err(JobError::Prepare(format!("{e:?}"))),
    }
}

/// 位相差入力（ねじれ加振）を `wave` へ付加する。
/// `phase_diff_enabled` が false なら `wave` をそのまま返す。
fn apply_phase_diff(
    cfg: &AnalysisSettings,
    mut wave: sepika_solver::dynamic::timehistory::GroundMotion,
) -> sepika_solver::dynamic::timehistory::GroundMotion {
    if !cfg.phase_diff_enabled {
        return wave;
    }
    use sepika_solver::dynamic::phase_diff::{phase_lag_time, torsional_accel_series};
    let lag = phase_lag_time(
        cfg.phase_diff_length_m,
        cfg.phase_diff_incidence_deg,
        cfg.phase_diff_vs,
    );
    let base: Vec<f64> = if cfg.phase_diff_dir_y {
        wave.accel_y.clone().unwrap_or_else(|| wave.accel_x.clone())
    } else {
        wave.accel_x.clone()
    };
    let l_mm = (cfg.phase_diff_length_m * 1000.0).max(1.0);
    let theta = torsional_accel_series(&base, wave.dt, lag, l_mm);
    wave.accel_theta = Some(theta);
    wave
}

/// 増分解析（プッシュオーバー）。モデルは呼び出し側で複製したものを渡すこと。
/// 剛域・仕口パネルは [`crate::prepare`] で適用済みのモデルを渡すこと。
pub fn compute_pushover(
    model: sepika_core::model::Model,
    cfg: AnalysisSettings,
) -> JobResult<sepika_solver::nonlinear::pushover::PushoverResult> {
    let (work, wall_index, _) = sepika_load::wall_expand::expand_wall_elements_owned(model);
    Analysis::prepare(&work).map_err(|e| JobError::Prepare(e.to_string()))?;
    let dofmap = sepika_core::dof::DofMap::build(&work);
    let reducer = sepika_solver::common::constraint::Reducer::build(&work, &dofmap);
    let target = sepika_solver::nonlinear::pushover::PushoverTarget {
        max_disp: cfg.push_use_max_disp.then_some(cfg.push_max_disp),
        max_drift_angle: cfg
            .push_use_drift_angle
            .then_some(1.0 / cfg.push_drift_denom.max(1.0)),
    };
    let mut result = sepika_solver::nonlinear::pushover::pushover_analysis_recording(
        &work,
        &dofmap,
        &reducer,
        cfg.push_dir,
        cfg.push_steps,
        target,
        cfg.push_control,
        cfg.push_apply_long_term,
        false,
        false,
        0.0,
        cfg.ductility_method,
    )
    .map_err(|e| JobError::Convergence(e.to_string()))?;
    if let Some(records) = &mut result.wall_history {
        for record in records {
            record.plate = wall_index.plate_of(record.elem);
        }
    }
    Ok(result)
}

/// 時刻歴応答解析。減衰モデル・積分法は `cfg` に従う。前処理を通したモデルを渡すこと。
pub fn compute_time_history(
    model: sepika_core::model::Model,
    cfg: AnalysisSettings,
    wave: sepika_solver::dynamic::timehistory::GroundMotion,
) -> JobResult<sepika_solver::dynamic::timehistory::ResponseResult> {
    model
        .validate_damper_mass_placement()
        .map_err(JobError::InvalidInput)?;
    let wave = apply_phase_diff(&cfg, wave);
    let model = expand_walls(model);
    let analysis = Analysis::prepare(&model).map_err(|e| JobError::Prepare(e.to_string()))?;
    let damping = match cfg.th_damping_model {
        ThDampingModel::StiffnessProportional => {
            let omega1 = match analysis.eigen(1) {
                Ok(modal) => match modal.omega2.first() {
                    Some(&w2) if w2 > 0.0 => w2.sqrt(),
                    _ => {
                        return Err(JobError::InvalidInput(
                            "固有値が得られず減衰を設定できません。".to_string(),
                        ))
                    }
                },
                Err(e) => return Err(dynamic_solve_error(e)),
            };
            sepika_solver::dynamic::damping::Damping::StiffnessProportional {
                h: cfg.th_damping,
                omega: omega1,
                basis: sepika_solver::dynamic::damping::StiffnessKind::Initial,
            }
        }
        ThDampingModel::Rayleigh => {
            let modal = match analysis.eigen(2) {
                Ok(m) => m,
                Err(e) => return Err(dynamic_solve_error(e)),
            };
            let (w1, w2) = match (modal.omega2.first(), modal.omega2.get(1)) {
                (Some(&a), Some(&b)) if a > 0.0 && b > 0.0 => (a.sqrt(), b.sqrt()),
                _ => {
                    return Err(JobError::InvalidInput(
                        "Rayleigh 減衰には 2 次までの固有値が必要です（モード数を確保できませんでした）。"
                            .to_string(),
                    ));
                }
            };
            sepika_solver::dynamic::damping::Damping::Rayleigh {
                h1: cfg.th_damping,
                w1,
                h2: cfg.th_h2,
                w2,
            }
        }
        ThDampingModel::Modal => {
            let mut modal = None;
            for k in (1..=6).rev() {
                if let Ok(m) = analysis.eigen(k) {
                    if !m.shapes.is_empty() {
                        modal = Some(m);
                        break;
                    }
                }
            }
            let modal = modal.ok_or_else(|| {
                JobError::InvalidInput("固有値が得られず減衰を設定できません。".to_string())
            })?;
            let omegas: Vec<f64> = modal
                .omega2
                .iter()
                .map(|&w2| if w2 > 0.0 { w2.sqrt() } else { 0.0 })
                .collect();
            let ratios = vec![cfg.th_damping; modal.shapes.len()];
            sepika_solver::dynamic::damping::Damping::modal(&modal.shapes, &omegas, &ratios)
        }
        ThDampingModel::TangentAlpha1 | ThDampingModel::TangentH1 => {
            let omega1 = match analysis.eigen(1) {
                Ok(modal) => match modal.omega2.first() {
                    Some(&w2) if w2 > 0.0 => w2.sqrt(),
                    _ => {
                        return Err(JobError::InvalidInput(
                            "固有値が得られず減衰を設定できません。".to_string(),
                        ))
                    }
                },
                Err(e) => return Err(dynamic_solve_error(e)),
            };
            if cfg.th_damping_model == ThDampingModel::TangentAlpha1 {
                sepika_solver::dynamic::damping::Damping::StiffnessProportional {
                    h: cfg.th_damping,
                    omega: omega1,
                    basis: sepika_solver::dynamic::damping::StiffnessKind::Tangent,
                }
            } else {
                sepika_solver::dynamic::damping::Damping::TangentStiffnessConstantH {
                    h1: cfg.th_damping,
                    omega1e: omega1,
                }
            }
        }
    };
    if cfg.th_nonlinear {
        return compute_nonlinear_time_history(model, cfg, wave, damping);
    }
    let record_every = (cfg.th_record_every > 0).then_some(cfg.th_record_every);
    let newmark = sepika_solver::dynamic::timehistory::NewmarkCfg::average_accel();
    analysis
        .time_history(&wave, newmark, damping, record_every)
        .map_err(dynamic_solve_error)
}

/// 非線形時刻歴応答解析（[`compute_time_history`] の非線形分岐）。
/// `use_kg`（幾何剛性）は `false`、`DampingAccumulation` は既定を用いる。
fn compute_nonlinear_time_history(
    model: sepika_core::model::Model,
    cfg: AnalysisSettings,
    wave: sepika_solver::dynamic::timehistory::GroundMotion,
    damping: sepika_solver::dynamic::damping::Damping,
) -> JobResult<sepika_solver::dynamic::timehistory::ResponseResult> {
    let model = model;
    sepika_element::factory::ensure_nonlinear_input_with_basis(
        &model,
        sepika_core::model::AnalysisKind::TimeHistory,
        sepika_element::factory::StrengthBasis::Nominal,
    )
    .map_err(|e| {
        JobError::InvalidInput(format!("非線形時刻歴（部材耐力を算定できません）:\n{e}"))
    })?;
    let dofmap = sepika_core::dof::DofMap::build(&model);
    let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dofmap);
    let n_indep = reducer.n_indep;
    let init = vec![0.0; n_indep];
    let newmark = sepika_solver::dynamic::timehistory::NewmarkCfg::average_accel();
    let record_every = (cfg.th_record_every > 0).then_some(cfg.th_record_every);
    let nl_cfg = sepika_solver::dynamic::timehistory::NonlinearThCfg {
        newton: sepika_solver::common::newton::NewtonCriteria::new(cfg.th_max_iter, cfg.th_tol),
        use_kg: false,
        apply_long_term: cfg.th_apply_long_term,
        record_every,
    };
    sepika_solver::dynamic::timehistory::nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        sepika_solver::dynamic::damping::DampingAccumulation::default(),
        &init,
        &init,
        nl_cfg,
    )
    .map_err(|e| match e {
        sepika_math::solver::SolveError::InvalidInput(message) => JobError::InvalidInput(message),
        other => JobError::Convergence(other.to_string()),
    })
}

/// 質点系（固有値。`accel` があれば時刻歴も）。
/// 3 次元は EX と EY の両方が必須。
pub fn compute_lumped_mass(
    model: sepika_core::model::Model,
    cfg: AnalysisSettings,
    res_x: Option<sepika_solver::statics::linear::StaticOnce>,
    res_y: Option<sepika_solver::statics::linear::StaticOnce>,
    po_x: Option<sepika_solver::nonlinear::pushover::PushoverResult>,
    po_y: Option<sepika_solver::nonlinear::pushover::PushoverResult>,
    accel: Option<&[f64]>,
) -> JobResult<sepika_solver::dynamic::lumped_mass::LumpedMassResult> {
    let model = expand_walls(model);
    let lm = crate::lumped_mass::build_lumped_mass(crate::lumped_mass::LumpedMassBuildInput {
        model: &model,
        dim: cfg.lumped_dim,
        source: cfg.lumped_stiffness,
        dir: cfg.lumped_dir,
        nonlinear: cfg.lumped_nonlinear,
        secant_ratio: cfg.lumped_secant_ratio,
        res_x: res_x.as_ref(),
        res_y: res_y.as_ref(),
        po_x: po_x.as_ref(),
        po_y: po_y.as_ref(),
    })?;
    let n_modes = cfg.lumped_n_modes.max(1);
    let modal = sepika_solver::dynamic::lumped_mass::lumped_mass_eigen(&lm, n_modes)
        .map_err(|e| JobError::Solve(e.to_string()))?;
    let response = if let Some(a) = accel {
        if a.is_empty() {
            return Err(JobError::InvalidInput(
                "質点系時刻歴の地動加速度が空です".into(),
            ));
        }
        if cfg.lumped_th_dt <= 0.0 {
            return Err(JobError::InvalidInput(
                "質点系時刻歴の時間刻み dt が 0 以下です".into(),
            ));
        }
        let resp = sepika_solver::dynamic::lumped_mass::lumped_mass_time_history(
            &lm,
            a,
            cfg.lumped_th_dt,
            cfg.lumped_th_damping,
        );
        if !lm.stories.is_empty() && resp.time.is_empty() {
            return Err(JobError::Solve(
                "質点系時刻歴を解けませんでした。質量または回転慣性を確認してください".into(),
            ));
        }
        Some(resp)
    } else {
        None
    };
    Ok(sepika_solver::dynamic::lumped_mass::LumpedMassResult {
        model: lm,
        modal,
        response,
    })
}

#[cfg(test)]
mod seismic_guard_tests {
    use super::*;
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::{MemberLoad, MemberLoadKind, Model, NodalLoad};

    #[test]
    fn tip_ex_ey_require_their_own_horizontal_axis() {
        let mut model = Model::default();
        model.load_cases = sepika_core::model::default_load_cases();
        for (index, axis, other) in [(3, 0, 1), (4, 1, 0)] {
            let case = &mut model.load_cases[index];
            case.member.push(MemberLoad {
                source: LoadSource::SlabTip,
                ..MemberLoad::manual(
                    ElemId(0),
                    if axis == 0 {
                        [1.0, 0.0, 0.0]
                    } else {
                        [0.0, 1.0, 0.0]
                    },
                    MemberLoadKind::Point { a: 100.0, p: 100.0 },
                )
            });
            assert!(missing_seismic_horizontal_load(case));
            let mut work = Model {
                load_cases: sepika_core::model::default_load_cases(),
                ..Default::default()
            };
            work.load_cases[index] = case.clone();
            let error = compute_linear_static(work, case.id).unwrap_err();
            assert!(error.to_string().contains("地震水平力"));

            let mut values = [0.0; 6];
            values[other] = 1.0;
            case.nodal.push(NodalLoad::manual(NodeId(0), values));
            assert!(missing_seismic_horizontal_load(case));
            case.nodal.push(NodalLoad::auto(NodeId(0), values));
            assert!(missing_seismic_horizontal_load(case));
            case.nodal.pop();
            case.member.push(MemberLoad::manual(
                ElemId(0),
                if other == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                },
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 100.0,
                    w1: 1.0,
                    w2: 1.0,
                },
            ));
            assert!(missing_seismic_horizontal_load(case));
            values[other] = 0.0;
            values[axis] = 1.0;
            case.nodal.push(NodalLoad::manual(NodeId(0), values));
            assert!(missing_seismic_horizontal_load(case));
            case.nodal.pop();
            case.member.push(MemberLoad::manual(
                ElemId(0),
                if axis == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                },
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 100.0,
                    w1: 1.0,
                    w2: 1.0,
                },
            ));
            assert!(missing_seismic_horizontal_load(case));
        }
    }

    #[test]
    fn tip_and_wrong_axis_manual_are_rejected_but_ai_and_other_cases_are_unchanged() {
        let mut cases = sepika_core::model::default_load_cases();
        for (index, axis, other) in [(3, 0, 1), (4, 1, 0)] {
            let case = &mut cases[index];
            assert!(missing_seismic_horizontal_load(case));
            case.member.push(MemberLoad {
                source: LoadSource::SlabTip,
                ..MemberLoad::manual(
                    ElemId(0),
                    [1.0, 0.0, 0.0],
                    MemberLoadKind::Point { a: 1.0, p: 1.0 },
                )
            });
            let mut values = [0.0; 6];
            values[other] = 10.0;
            case.nodal.push(NodalLoad::manual(NodeId(0), values));
            assert!(missing_seismic_horizontal_load(case));
            let mut model = Model {
                load_cases: sepika_core::model::default_load_cases(),
                ..Default::default()
            };
            model.load_cases[index] = case.clone();
            let error = compute_linear_static(model.clone(), case.id).unwrap_err();
            assert!(error.to_string().contains("地震水平力"));
            values[axis] = 20.0;
            values[other] = 0.0;
            case.nodal.push(NodalLoad::auto(NodeId(0), values));
            assert!(!missing_seismic_horizontal_load(case));
            model.load_cases[index] = case.clone();
            assert!(!matches!(
                compute_linear_static(model, case.id),
                Err(JobError::InvalidInput(_))
            ));
            case.nodal.clear();
            case.member.clear();
        }
        assert!(!missing_seismic_horizontal_load(&cases[0]));
        cases[3].kind = LoadCaseKind::Other;
        assert!(!missing_seismic_horizontal_load(&cases[3]));
    }
}
