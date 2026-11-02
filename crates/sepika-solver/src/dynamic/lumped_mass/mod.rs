//! 質点系（串団子）モデルの生成（せん断型多質点系、構造力学）。
//!
//! 立体フレームのプッシュオーバー結果から層ごとの Q-δ 関係を等包絡面積則でトリリニア骨格へ縮約する。

mod bending;
mod eigen;
mod identification;
mod model;
mod spatial;
mod time_history;

pub use bending::{bending_drifts, bending_shear_element, bending_shear_time_history};
pub use eigen::{lumped_mass_eigen, LumpedMassModal};
pub use identification::{identify_story_trilinear, StoryFit, StoryFitError, StoryFitOptions};
pub use model::{
    build_lumped_mass_model, LumpedMassModel, LumpedMassType, LumpedStiffnessSource, StickDim,
    StoryBending, StoryIdentification, StorySpatial, StoryStick, StoryTrilinear,
};
pub use time_history::{lumped_mass_time_history, StickDirPeaks, StickResponse, STICK_NEWTON};

/// 質点系解析の保存用結果（モデル・固有値・時刻歴）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LumpedMassResult {
    pub model: LumpedMassModel,
    pub modal: LumpedMassModal,
    #[serde(default)]
    pub response: Option<StickResponse>,
}

/// 非線形質点系を成立させられなかった診断。解析モデル・応答値を含まない。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LumpedMassIdentificationFailure {
    pub reason: String,
    pub model_type: LumpedMassType,
    pub dim: StickDim,
    /// 同定に失敗した対象方向。入力の加振方向とは独立。
    pub dir: crate::statics::analysis::SeismicDir,
    pub target_drift_angle: f64,
    pub secant_ratio: f64,
    pub manual_intervals: Vec<sepika_core::model::LumpedSkeletonInterval>,
    pub source_targets: Vec<(
        crate::statics::analysis::SeismicDir,
        Option<crate::nonlinear::pushover::PushoverTarget>,
    )>,
}

impl std::fmt::Display for LumpedMassIdentificationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

#[cfg(test)]
use eigen::stick_omega1;
#[cfg(test)]
use sepika_core::ids::StoryId;
#[cfg(test)]
use time_history::fundamental_omega;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statics::analysis::SeismicDir;

    #[test]
    fn zero_increment_actual_planar_and_spatial_xy_solvers_report_independent_shear() {
        let sk = StoryTrilinear {
            k1: 1000.0,
            d1: 0.01,
            q1: 10.0,
            d2: 0.02,
            q2: 20.0,
            d3: 0.04,
            q3: 30.0,
        };
        // m=1、dt=1、減衰0のNewmark釣合いで、変位[-.03,-.03,-.04]に対応する加速度。
        let acceleration = [0.0, 25.12, 24.64, 30.64];
        for dim in [StickDim::Planar, StickDim::Spatial] {
            for dir in [SeismicDir::X, SeismicDir::Y] {
                let lm = LumpedMassModel {
                    identifications: Vec::new(),
                    bending: Vec::new(),
                    model_type: LumpedMassType::EquivalentShear,
                    stories: vec![StoryStick {
                        story: StoryId(0),
                        mass: 1.0,
                        height: 3000.0,
                        skeleton: sk,
                    }],
                    dim,
                    stiffness_source: LumpedStiffnessSource::StoryQd,
                    dir,
                    nonlinear: true,
                    spatial: if dim == StickDim::Spatial {
                        vec![StorySpatial {
                            j: 1.0,
                            mass_xy: [0.0, 0.0],
                            rigidity_xy: [0.0, 0.0],
                            k1_x: 1000.0,
                            k1_y: 1000.0,
                            kr: 1.0,
                            skeleton_x: sk,
                            skeleton_y: sk,
                        }]
                    } else {
                        vec![]
                    },
                };
                let response = lumped_mass_time_history(&lm, &acceleration, 1.0, 0.0).unwrap();
                assert_eq!(response.non_converged_steps, 0);
                assert_eq!(response.roof_disp.len(), 4);
                for (got, expected) in response.roof_disp.iter().zip([0.0, -0.03, -0.03, -0.04]) {
                    assert!((got - expected).abs() < 1e-10, "{dim:?}/{dir:?}: {got}");
                }
                assert!((response.story_peak_shear[0] - 30.0).abs() < 1e-8);
                let (active, inactive) = match dir {
                    SeismicDir::X => (&response.shear_dir.x, &response.shear_dir.y),
                    SeismicDir::Y => (&response.shear_dir.y, &response.shear_dir.x),
                };
                assert!((active[0] - 30.0).abs() < 1e-8);
                assert_eq!(inactive[0], 0.0);
            }
        }
    }

    #[test]
    fn instantaneous_samples_match_independent_newmark_in_planar_spatial_and_xy() {
        for dim in [StickDim::Planar, StickDim::Spatial] {
            for dir in [SeismicDir::X, SeismicDir::Y] {
                let mut lm = LumpedMassModel::from_stories(
                    LumpedMassType::EquivalentShear,
                    vec![StoryStick {
                        story: StoryId(0),
                        mass: 1.0,
                        height: 3000.0,
                        skeleton: StoryTrilinear::elastic(40.0),
                    }],
                );
                lm.dir = dir;
                lm.dim = dim;
                if dim == StickDim::Spatial {
                    lm.spatial = vec![StorySpatial {
                        j: 1.0,
                        mass_xy: [0.0; 2],
                        rigidity_xy: [0.0; 2],
                        k1_x: 40.0,
                        k1_y: 40.0,
                        kr: 40.0,
                        skeleton_x: StoryTrilinear::elastic(40.0),
                        skeleton_y: StoryTrilinear::elastic(40.0),
                    }];
                }
                for (input, expected) in [
                    (
                        vec![0.0, 100.0, 0.0],
                        vec![0.0, -100.0 / 440.0, -0.8264462809917356],
                    ),
                    (vec![100.0, 0.0], vec![0.0, -100.0 / 440.0]),
                    (vec![100.0], vec![0.0]),
                    (vec![0.0, 0.0], vec![0.0, 0.0]),
                ] {
                    let result = lumped_mass_time_history(&lm, &input, 0.1, 0.0).unwrap();
                    assert_eq!(result.time.len(), input.len());
                    assert_eq!(result.non_converged_steps, 0);
                    for (i, &u) in expected.iter().enumerate() {
                        assert_eq!(result.time[i], i as f64 * 0.1);
                        assert!(
                            (result.roof_disp[i] - u).abs() <= 1e-9,
                            "{dim:?}/{dir:?}: {i}"
                        );
                        let d = if dir == SeismicDir::X { 0 } else { 1 };
                        assert!((result.floor_disp[i][0][d] - u).abs() <= 1e-9);
                    }
                }
                assert!(lumped_mass_time_history(&lm, &[], 0.1, 0.0)
                    .unwrap()
                    .time
                    .is_empty());
            }
        }
    }

    #[test]
    fn zero_increment_planar_story_spring_preserves_negative_peak_and_next_response() {
        use sepika_material::UniaxialMaterial;
        let sk = StoryTrilinear {
            k1: 1000.0,
            d1: 0.01,
            q1: 10.0,
            d2: 0.02,
            q2: 20.0,
            d3: 0.04,
            q3: 30.0,
        };
        let mut direct = time_history::story_spring(&sk);
        assert_eq!(direct.probe(0.0), (0.0, 1000.0));
        assert_eq!(direct.trial(-0.03), (-25.0, 500.0));
        direct.commit();
        let mut repeated = direct.clone();
        let before = repeated.serialize_state();
        assert_eq!(repeated.probe(-0.03), (-25.0, 500.0));
        assert_eq!(before, repeated.serialize_state());
        repeated.trial(-0.03);
        repeated.commit();
        assert_eq!(direct.serialize_state(), repeated.serialize_state());
        assert_eq!(direct.trial(-0.04), (-30.0, 500.0));
        assert_eq!(repeated.trial(-0.04), (-30.0, 500.0));
        direct.revert();
        assert_eq!(before, direct.serialize_state());
        for theta in [-0.04, -0.039, 0.01, 0.03, 0.029, -0.02] {
            assert_eq!(direct.trial(theta), repeated.trial(theta));
            direct.commit();
            repeated.commit();
            repeated.trial(theta);
            repeated.commit();
            assert_eq!(direct.serialize_state(), repeated.serialize_state());
        }
    }

    fn stick(mass: f64, k1: f64, d1: f64, d2: f64, q2: f64, d3: f64, q3: f64) -> StoryStick {
        StoryStick {
            story: StoryId(0),
            mass,
            height: 3000.0,
            skeleton: StoryTrilinear {
                k1,
                d1,
                q1: k1 * d1,
                d2,
                q2,
                d3,
                q3,
            },
        }
    }

    #[test]
    fn test_lumped_mass_eigen_two_dof_analytic() {
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![
                stick(1.0, 1.0, 0.1, 0.3, 0.7, 1.0, 0.8),
                stick(1.0, 1.0, 0.1, 0.3, 0.7, 1.0, 0.8),
            ],
        };
        let modal = lumped_mass_eigen(&lm, 2).expect("固有値分解に成功する");
        assert_eq!(modal.omega2.len(), 2);
        let expected = [(3.0 - 5.0_f64.sqrt()) / 2.0, (3.0 + 5.0_f64.sqrt()) / 2.0];
        assert!(
            (modal.omega2[0] - expected[0]).abs() < 1e-9,
            "1次: {} vs {}",
            modal.omega2[0],
            expected[0]
        );
        assert!(
            (modal.omega2[1] - expected[1]).abs() < 1e-9,
            "2次: {} vs {}",
            modal.omega2[1],
            expected[1]
        );
        assert_eq!(modal.period.len(), 2);
        for (period, omega2) in modal.period.iter().zip(expected) {
            assert!((period - 2.0 * std::f64::consts::PI / omega2.sqrt()).abs() < 1e-9);
        }
    }

    /// `lumped_mass_eigen` の1次モードと逆反復法 `fundamental_omega` が同じ入力で近い値を返すこと。
    #[test]
    fn test_lumped_mass_eigen_matches_power_iteration_omega1() {
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![
                stick(2.0, 2000.0, 0.1, 0.3, 250.0, 1.0, 300.0),
                stick(1.5, 1500.0, 0.1, 0.3, 200.0, 1.0, 260.0),
                stick(1.0, 900.0, 0.1, 0.3, 150.0, 1.0, 200.0),
            ],
        };
        let modal = lumped_mass_eigen(&lm, 1).expect("固有値分解に成功する");
        let mass: Vec<f64> = lm.stories.iter().map(|s| s.mass).collect();
        let k1: Vec<f64> = lm.stories.iter().map(|s| s.skeleton.k1).collect();
        let w_power = fundamental_omega(&mass, &k1);
        let w_eigen = modal.omega2[0].sqrt();
        assert!(
            (w_eigen - w_power).abs() < 1e-6 * w_power,
            "eigen={w_eigen}, power_iteration={w_power}"
        );
    }

    /// 質量が 0 以下の層があると `SolveError::InvalidInput` を返す（実体のない
    /// 極端な高周波モードを紛れ込ませず、明示的にエラー報告する）。
    #[test]
    fn test_lumped_mass_eigen_rejects_non_positive_mass() {
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![stick(0.0, 1000.0, 0.1, 0.3, 250.0, 1.0, 300.0)],
        };
        let err = lumped_mass_eigen(&lm, 1).unwrap_err();
        assert!(matches!(
            err,
            sepika_math::solver::SolveError::InvalidInput(_)
        ));
    }

    /// `stick_omega1` は、質量 0 以下の層があっても ω1 を桁違いに大きくしないこと。
    #[test]
    fn test_stick_omega1_survives_zero_mass_story_without_blowing_up() {
        let healthy = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![
                stick(2.0, 2000.0, 0.1, 0.3, 250.0, 1.0, 300.0),
                stick(1.5, 1500.0, 0.1, 0.3, 200.0, 1.0, 260.0),
            ],
        };
        let w_healthy = stick_omega1(&healthy);

        let mut with_zero_mass_story = healthy.clone();
        with_zero_mass_story.stories[1].mass = 0.0;

        let w_zero_mass = stick_omega1(&with_zero_mass_story);

        assert!(w_zero_mass.is_finite() && w_zero_mass > 0.0);
        // 健全なケースと同程度のオーダー（1桁以内）に収まること。
        assert!(
            w_zero_mass < w_healthy * 10.0,
            "ω1 が異常に大きい（無減衰化の兆候）: zero_mass={w_zero_mass}, healthy={w_healthy}"
        );
    }

    /// `stick_omega1` は、層剛性 K1 が 0 の層があっても ω1 を 0 に潰さないこと。
    #[test]
    fn test_stick_omega1_survives_zero_stiffness_story_without_blowing_up() {
        let healthy = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![
                stick(2.0, 2000.0, 0.1, 0.3, 250.0, 1.0, 300.0),
                stick(1.5, 1500.0, 0.1, 0.3, 200.0, 1.0, 260.0),
            ],
        };
        let w_healthy = stick_omega1(&healthy);

        let mut with_zero_stiffness_story = healthy.clone();
        with_zero_stiffness_story.stories[0].skeleton.k1 = 0.0;

        let w_zero_k1 = stick_omega1(&with_zero_stiffness_story);

        assert!(
            w_zero_k1.is_finite() && w_zero_k1 > 0.0,
            "K1=0 の層があると ω1 が 0 に潰れ、a1=2h/ω1 が無条件にゼロになる: w_zero_k1={w_zero_k1}"
        );
        assert!(
            w_zero_k1 < w_healthy * 10.0,
            "ω1 が異常に大きい（クランプ後の逆反復法へのフォールバックが桁違いになっていないか）: \
             zero_k1={w_zero_k1}, healthy={w_healthy}"
        );
    }

    /// n_modes が層数を超える場合、返るモード数は層数まで切り詰められる
    /// （立体モデルの `solve_eigen` と同じ規約）。
    #[test]
    fn test_lumped_mass_eigen_truncates_to_story_count() {
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![stick(1.0, 1000.0, 0.1, 0.3, 250.0, 1.0, 300.0)],
        };
        let modal = lumped_mass_eigen(&lm, 5).expect("固有値分解に成功する");
        assert_eq!(modal.omega2.len(), 1);
        assert_eq!(modal.shapes.len(), 1);
        // 単一質点のモード形状は最上階=1.0 に正規化される。
        assert!((modal.shapes[0][0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_stick_th_zero_input_zero_response() {
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![stick(1.0, 1000.0, 0.1, 0.3, 140.0, 1.0, 160.0)],
        };
        let res = lumped_mass_time_history(&lm, &vec![0.0; 200], 0.01, 0.02).unwrap();
        assert!(res.roof_disp.iter().all(|&v| v.abs() < 1e-9));
        assert_eq!(res.story_ductility[0], 0.0);
    }

    #[test]
    fn test_stick_th_responds_and_bounded() {
        // 正弦地動で応答が非ゼロかつ有限。
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![
                stick(1.0, 2000.0, 0.1, 0.3, 250.0, 1.0, 300.0),
                stick(1.0, 1500.0, 0.1, 0.3, 200.0, 1.0, 260.0),
            ],
        };
        let dt = 0.01;
        let accel: Vec<f64> = (0..300)
            .map(|i| 2000.0 * (2.0 * std::f64::consts::PI * 1.5 * i as f64 * dt).sin())
            .collect();
        let res = lumped_mass_time_history(&lm, &accel, dt, 0.03).unwrap();
        assert_eq!(res.time.len(), 300);
        assert!(res.roof_disp.iter().all(|v| v.is_finite()));
        assert!(
            res.roof_disp.iter().any(|&v| v.abs() > 1e-3),
            "should show nonzero roof response"
        );
        assert_eq!(res.story_peak_drift.len(), 2);
        assert_eq!(res.drift_dir.x.len(), 2);
        assert!(
            (res.drift_dir.x[0] - res.story_peak_drift[0]).abs() < 1e-12
                && res.drift_dir.y[0].abs() < 1e-12,
            "2 次元 X 加振の層間は X=最大・Y=0"
        );
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        assert!((res.drift_dir.deg45[0] - res.story_peak_drift[0] * s2).abs() < 1e-9);
    }

    #[test]
    fn test_stick_th_yields_under_strong_input() {
        // 強い地動で層が降伏（塑性率 μ>1）。
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            stories: vec![stick(2.0, 1000.0, 0.5, 2.0, 700.0, 8.0, 800.0)],
        };
        let dt = 0.01;
        // 一定方向の強い引き込みで大変形。
        let accel: Vec<f64> = (0..400)
            .map(|i| {
                let t = i as f64 * dt;
                3000.0 * (2.0 * std::f64::consts::PI * 0.8 * t).sin()
            })
            .collect();
        let res = lumped_mass_time_history(&lm, &accel, dt, 0.02).unwrap();
        assert!(res.roof_disp.iter().all(|v| v.is_finite()));
        assert!(
            res.story_ductility[0] > 1.0,
            "strong input should yield the story: μ={}",
            res.story_ductility[0]
        );
    }
}
