use super::*;

mod timeline;

use crate::common::constraint::Reducer;
use crate::dynamic::damping::{Damping, DampingAccumulation, StiffnessKind};
use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, Node, Section,
};

/// Ux のみ自由。
const FREE_UX: Dof6Mask = Dof6Mask(0b111110);

/// Uy のみ自由（Y 方向専用モデル用）。
const FREE_UY: Dof6Mask = Dof6Mask(0b111101);

/// SDOF: m=1.0 N·s²/mm, k=1000 N/mm。
/// ω = √(k/m) = 31.6228 rad/s, T = 0.198692 s。
///
/// 弾性ばねを模擬する検証モデルのため、降伏強度は塑性化しない十分大きな値
/// （`fy = 1e10`。`fiber_column_model(1e10)` と同じ扱い）とする。非線形解析は
/// 材料強度が未入力の部材（＝耐力を算定できず降伏しない部材）を
/// `factory::ensure_nonlinear_input` で拒否するため、`fy`・`Fc` のいずれかが要る。
fn sdof_model() -> Model {
    let k = 1000.0_f64;
    let m = 1.0_f64;
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [1000.0, 0.0, 0.0],
                restraint: FREE_UX,
                mass: Some([m, 0.0, 0.0, 0.0, 0.0, 0.0]),
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(1),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "spring".into(),
            area: 1.0,
            iy: 1.0,
            iz: 1.0,
            j: 1.0,
            depth: 1.0,
            width: 1.0,
            as_y: 1e12,
            as_z: 1e12,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "mat".into(),
            category: MaterialCategory::Steel,
            young: k * 1000.0 / 1.0,
            poisson: 0.0,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(1e10),
        }],
        ..Default::default()
    }
}

/// SDOF（Y 方向自由度のみ）: `sdof_model()` と同形状・同剛性の梁を、
/// 拘束を Uy のみ自由に変えたもの。Y 方向加振時の記録方向自動選択を
/// 検証するために使う。
fn sdof_model_y() -> Model {
    let mut model = sdof_model();
    model.nodes[1].restraint = FREE_UY;
    model.nodes[1].mass = Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    model
}

fn zero_wave(dt: f64, n_steps: usize) -> GroundMotion {
    GroundMotion {
        dt,
        accel_x: vec![0.0; n_steps + 1],
        accel_y: None,
        accel_theta: None,
    }
}

/// SDOF 自由振動の解析解: u(t) = e^{−ζωt}(cos ωd t + (ζω/ωd) sin ωd t)
fn sdof_analytical(t: f64, omega: f64, zeta: f64) -> f64 {
    let omega_d = omega * (1.0 - zeta * zeta).sqrt();
    let decay = (-zeta * omega * t).exp();
    decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin())
}

/// SDOF 自由振動（u0=1, v0=0, ζ=0.02）の変位時刻歴が解析解
/// u(t)=e^{−ζωt}(cos ωd t + (ζω/ωd) sin ωd t) に一致すること。
/// 剛性比例減衰の減衰比と応答波形の両方を、解析解との全フレーム照合で確認する。
#[test]
fn test_sdof_free_vibration_matches_analytical() {
    let model = sdof_model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let omega = (1000.0_f64 / 1.0).sqrt();
    let zeta = 0.02;
    let damping = Damping::StiffnessProportional {
        h: zeta,
        omega,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.0002;
    // 自由振動 1 周期（T≈0.1987 s）ぶんを照合する。
    let total_time = 0.2;
    let n_steps = (total_time / dt) as usize;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let result = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[1.0],
        &[0.0],
        false,
        Some(1),
    )
    .expect("time history should converge");

    assert!(!result.nonlinear);
    assert!(!result.applied_long_term);
    assert_eq!(result.time.len(), n_steps + 1);
    assert!((result.time.last().copied().unwrap_or(0.0) - total_time).abs() < 1e-9);

    // 各フレームの自由節点（node 1）の X 変位を解析解と照合する。
    // 許容値は Newmark-β（平均加速度法）の 1 周期ぶんの積分誤差（オーダー 1e-3）を
    // 吸収しつつ、減衰比・周期の誤り（数 % 以上の差）を検出できる大きさとする。
    let recording = result.recording.expect("recording");
    assert_eq!(recording.frame_time.len(), n_steps + 1);
    for (frame, &t) in recording.frame_time.iter().enumerate() {
        let u_num = recording.node_disp[frame][1][0];
        let u_ana = sdof_analytical(t, omega, zeta);
        assert!(
            (u_num - u_ana).abs() < 5e-3,
            "t={t}: numerical {u_num} vs analytical {u_ana}"
        );
    }
    // 減衰系の自由振動なのでピークは初期変位 1.0。
    assert!((result.peak_disp[1][0] - 1.0).abs() < 1e-9);
}

/// 2DOFせん断モデルの1次モード純励振がモード重ね合わせと一致すること。
#[test]
fn test_2dof_mode_superposition_consistency() {
    let k = 1000.0_f64;
    let m = 1.0_f64;
    let mut model = sdof_model();
    model.materials[0].fy = None;
    let mut top = model.nodes[1].clone();
    top.id = NodeId(2);
    top.coord = [2000.0, 0.0, 0.0];
    model.nodes.push(top);
    let mut upper_spring = model.elements[0].clone();
    upper_spring.id = ElemId(2);
    upper_spring.nodes = smallvec::smallvec![NodeId(1), NodeId(2)];
    model.elements.push(upper_spring);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    // 1次モード: λ1 = (k/m)(3-√5)/2, φ2/φ1 = k/(k-λ1) ≈ 1.618
    let lam1 = (k / m) * (3.0 - 5.0_f64.sqrt()) / 2.0;
    let omega1 = lam1.sqrt();
    let phi_ratio = k / (k - lam1);

    // 1次モードの剛性比例減衰
    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: omega1,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.0002;
    let n_steps = 500;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let (result, final_state) = linear_time_history_with_state(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[1.0, phi_ratio],
        &[0.0, 0.0],
        false,
        None,
    )
    .expect("mode superposition test");

    // ピーク比がモード形状比に一致（モード重ね合わせの一致性）
    let peak_ratio = result.peak_disp[2][0] / result.peak_disp[1][0];
    assert!(
        (peak_ratio - phi_ratio).abs() / phi_ratio < 0.01,
        "peak ratio {} should match 1st mode shape ratio {} within 1%",
        peak_ratio,
        phi_ratio
    );

    // 最終状態でもモード形状比が維持されている（1次モードのみ励起）
    let final_ratio = final_state.disp_red[1].abs() / final_state.disp_red[0].abs();
    assert!(
        (final_ratio - phi_ratio).abs() / phi_ratio < 0.02,
        "final disp ratio {} should match mode shape ratio {} within 2%",
        final_ratio,
        phi_ratio
    );
}

#[test]
fn test_newmark_schemes_in_stable_ranges() {
    let model = sdof_model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: 1000.0_f64.sqrt(),
        basis: StiffnessKind::Initial,
    };
    for (beta, dt, n_steps) in [(0.25, 1.0, 10), (1.0 / 6.0, 0.01, 100)] {
        let result = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &zero_wave(dt, n_steps),
            &NewmarkCfg {
                beta,
                gamma: 0.5,
                dt,
            },
            &damping,
            &[1.0],
            &[0.0],
            false,
            None,
        )
        .expect("Newmark scheme should remain stable in its tested range");
        let peak = result.peak_disp[1][0];
        assert!(
            peak.is_finite() && peak < 1e6,
            "beta={beta}, dt={dt}, peak={peak}"
        );
    }
}

/// チェックポイント再開のビット一致。
/// 連続実行の最終状態と、途中保存→再開の最終状態がビット完全一致すること。
#[test]
fn test_checkpoint_restart_bit_exact() {
    let model = sdof_model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let omega = (1000.0_f64 / 1.0).sqrt();
    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.001;
    let n_total = 500;
    let m = 200;

    // 全波形
    let wave_full = zero_wave(dt, n_total);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    // 1) 連続実行 0→N
    let (_result_cont, state_cont) = linear_time_history_with_state(
        &model,
        &dofmap,
        &reducer,
        &wave_full,
        &newmark,
        &damping,
        &[1.0],
        &[0.0],
        false,
        None,
    )
    .expect("continuous run");

    // 2) 前半 0→M（短縮波）
    let wave_half = zero_wave(dt, m);
    let (_result_half, state_half) = linear_time_history_with_state(
        &model,
        &dofmap,
        &reducer,
        &wave_half,
        &newmark,
        &damping,
        &[1.0],
        &[0.0],
        false,
        None,
    )
    .expect("first half");
    assert_eq!(state_half.step, m as u64);

    // 3) チェックポイント経由で bincode 往復（保存→読込をシミュレート）
    let bytes = bincode::serialize(&state_half).expect("serialize state");
    let state_loaded: TimeStepState = bincode::deserialize(&bytes).expect("deserialize state");

    // 4) 再開 M→N
    let (_result_restart, state_restart) = linear_time_history_from_state(
        &model,
        &dofmap,
        &reducer,
        &wave_full,
        &newmark,
        &damping,
        &state_loaded,
        false,
        None,
    )
    .expect("restart");

    // 5) ビット一致判定
    assert_eq!(state_restart.step, state_cont.step);
    assert_eq!(state_restart.disp_red.len(), state_cont.disp_red.len());
    for i in 0..state_cont.disp_red.len() {
        assert_eq!(
            state_restart.disp_red[i].to_bits(),
            state_cont.disp_red[i].to_bits(),
            "disp[{}] restart={} continuous={}",
            i,
            state_restart.disp_red[i],
            state_cont.disp_red[i]
        );
    }
    for i in 0..state_cont.vel_red.len() {
        assert_eq!(
            state_restart.vel_red[i].to_bits(),
            state_cont.vel_red[i].to_bits(),
            "vel[{}] mismatch",
            i
        );
    }
    for i in 0..state_cont.accel_red.len() {
        assert_eq!(
            state_restart.accel_red[i].to_bits(),
            state_cont.accel_red[i].to_bits(),
            "accel[{}] mismatch",
            i
        );
    }
}

/// Y 方向のみの加振（accel_x 全ゼロ、accel_y 正弦波）で記録方向が自動的に
/// Y へ切り替わり（`record_dir_y == true`）、代表応答（`node_disp`）が
/// 非ゼロになることを検証する。Y 方向にのみ自由度を持つ SDOF モデル
/// （`sdof_model_y`）を使用。
#[test]
fn test_y_direction_wave_selects_y_record_dir() {
    let model = sdof_model_y();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let omega = (1000.0_f64 / 1.0).sqrt();
    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.001;
    let n_steps = 500;
    let accel_y: Vec<f64> = (0..n_steps)
        .map(|i| {
            let t = i as f64 * dt;
            500.0 * (2.0 * std::f64::consts::PI * 2.0 * t).sin()
        })
        .collect();
    let wave = GroundMotion {
        dt,
        accel_x: vec![0.0; n_steps],
        accel_y: Some(accel_y),
        accel_theta: None,
    };
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let result = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[0.0],
        &[0.0],
        false,
        None,
    )
    .expect("Y-direction time history should converge");

    // accel_x が全ゼロ、accel_y が非ゼロなので記録方向は Y。
    assert!(
        result.history.record_dir_y,
        "record_dir_y should be true when only accel_y is nonzero"
    );
    // 代表応答（Y 方向変位）が非ゼロ応答を持つ。
    assert!(
        result.history.node_disp.iter().any(|v| v.abs() > 1e-6),
        "node_disp should show nonzero Y-direction response"
    );
    // X 方向は自由度自体が存在しないため応答なし。
    assert_eq!(result.peak_disp[1][0], 0.0);
    // Y 方向は実際に応答している。
    assert!(result.peak_disp[1][1] > 0.0);
}

/// 位相差入力（ねじれ地動加速度 `accel_theta`）が、節点重心から偏心した自由節点の
/// 並進応答を励起することを検証する。並進入力（accel_x/y）はゼロで、ねじれ入力のみ。
#[test]
fn test_phase_diff_torsion_excites_eccentric_node() {
    // sdof_model と同構成だが、自由節点を (1000,1000,0) へ置き、重心(500,500)から
    // Y 方向に偏心させる。ねじれ加振の回転影響 ax=−(y−yc)≠0 が ux を励起する。
    let mut model = sdof_model();
    model.nodes[1].coord = [1000.0, 1000.0, 0.0];
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let omega = (1000.0_f64 / 1.0).sqrt();
    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega,
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 500;
    let theta: Vec<f64> = (0..n_steps)
        .map(|i| {
            let t = i as f64 * dt;
            // ねじれ地動加速度 [rad/s²]。
            1e-3 * (2.0 * std::f64::consts::PI * 3.0 * t).sin()
        })
        .collect();
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    // (1) ねじれ入力ありの応答。
    let wave_t = GroundMotion {
        dt,
        accel_x: vec![0.0; n_steps],
        accel_y: None,
        accel_theta: Some(theta.clone()),
    };
    let res_t = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave_t,
        &newmark,
        &damping,
        &[0.0],
        &[0.0],
        false,
        None,
    )
    .expect("torsion time history should converge");

    // (2) ねじれ入力なし（並進もゼロ）→ 応答ゼロ。
    let wave_0 = GroundMotion {
        dt,
        accel_x: vec![0.0; n_steps],
        accel_y: None,
        accel_theta: None,
    };
    let res_0 = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave_0,
        &newmark,
        &damping,
        &[0.0],
        &[0.0],
        false,
        None,
    )
    .expect("zero input should converge");

    // ねじれ入力ありは偏心節点の ux を励起（非ゼロ）。
    assert!(
        res_t.peak_disp[1][0] > 1e-9,
        "torsion should excite eccentric node ux: {}",
        res_t.peak_disp[1][0]
    );
    // 入力ゼロは応答ゼロ。
    assert_eq!(res_0.peak_disp[1][0], 0.0);
}

use sepika_core::ids::StoryId;
use sepika_core::model::Story;

fn fiber_column_model(fy: f64) -> Model {
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask(0b111110),
                mass: Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                story: Some(StoryId(1)),
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Fiber,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "col".to_string(),
            area: 10000.0,
            iy: 8.333e6,
            iz: 8.333e6,
            j: 1.0e6,
            depth: 100.0,
            width: 100.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "steel".to_string(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(fy),
        }],
        stories: vec![
            // 階は床であり、先頭は基部の床（`Model::layers` の不変条件）。
            Story {
                wall_weights: Vec::new(),
                level_kind: Default::default(),
                structure: Default::default(),
                id: StoryId(0),
                name: "1F".to_string(),
                elevation: Some(0.0),
                sl_offset_mm: Some(0.0),
                source_story: None,
                node_ids: vec![NodeId(0)],
                seismic_weight: None,
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            },
            Story {
                wall_weights: Vec::new(),
                level_kind: Default::default(),
                structure: Default::default(),
                id: StoryId(1),
                name: "2F".to_string(),
                elevation: Some(3000.0),
                sl_offset_mm: Some(0.0),
                source_story: None,
                node_ids: vec![NodeId(1)],
                seismic_weight: Some(10000.0),
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            },
        ],
        ..Default::default()
    }
}

/// 弾性 SDOF（降伏しないファイバ柱）の非線形時刻歴が線形解と一致。
#[test]
fn test_nonlinear_time_history_sdof_elastic() {
    // fy を非常に高く設定し、塑性化しないようにする
    let model = fiber_column_model(1e10);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    // 線形解との比較用: SDOF 線形剛性を計算
    let m_free = assemble_global_m(&model, &dofmap, MassOption::Consistent).unwrap();
    let k_free = assemble_global_k(&model, &dofmap);
    let m_red_lin = reducer.reduce_k(&m_free);
    let k_red_lin = reducer.reduce_k(&k_free);

    // 初期剛性から SDOF 固有円振動数を推定
    let k_val = *k_red_lin.get(0, 0).unwrap_or(&0.0);
    let m_val = *m_red_lin.get(0, 0).unwrap_or(&0.0);
    let omega = (k_val / m_val).sqrt();

    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.001;
    let n_steps = 100;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    // 非線形解析
    let result_nl = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[1.0],
        &[0.0],
        NonlinearThCfg::new(20, 1e-6),
    )
    .expect("nonlinear elastic should converge");

    let result_lin = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[1.0],
        &[0.0],
        false,
        None,
    )
    .expect("linear should run");

    assert_eq!(result_nl.time.len(), n_steps + 1);
    assert_eq!(result_nl.time, result_lin.time);
    assert_eq!(result_lin.history.node_disp.len(), n_steps + 1);
    assert_eq!(result_nl.history.node_disp.len(), n_steps + 1);
    let peak_nl = result_nl.peak_disp[1][0];
    let peak_lin = result_lin.peak_disp[1][0];
    assert!(
        (peak_nl - peak_lin).abs() / peak_lin.max(1e-6) < 0.05,
        "nonlinear peak={peak_nl}, linear peak={peak_lin}"
    );
    let tolerance = result_lin.peak_disp[1][0].max(1e-6) * 0.05;
    for (&nonlinear, &linear) in result_nl
        .history
        .node_disp
        .iter()
        .zip(&result_lin.history.node_disp)
    {
        assert!(
            (nonlinear - linear).abs() < tolerance,
            "nonlinear={nonlinear}, linear={linear}"
        );
    }
}

/// 接線剛性比例(α1一定・h1一定)・モード別の減衰が非線形時刻歴で収束し有限応答を返す
/// ことを確認する（剛性変更に伴う減衰項の変更、構造動力学）。
#[test]
fn test_nonlinear_time_history_extended_damping_models_run() {
    let base = fiber_column_model(100.0);
    let dofmap0 = DofMap::build(&base);
    let reducer0 = Reducer::build(&base, &dofmap0);
    let m_red =
        reducer0.reduce_k(&assemble_global_m(&base, &dofmap0, MassOption::Consistent).unwrap());
    let k_red = reducer0.reduce_k(&assemble_global_k(&base, &dofmap0));
    let m_val = *m_red.get(0, 0).unwrap_or(&1.0);
    let omega = (*k_red.get(0, 0).unwrap_or(&0.0) / m_val).sqrt();

    let dt = 0.001;
    let n_steps = 30;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let dampings = vec![
        Damping::StiffnessProportional {
            h: 0.05,
            omega,
            basis: StiffnessKind::Tangent,
        },
        Damping::TangentStiffnessConstantH {
            h1: 0.05,
            omega1e: omega,
        },
        Damping::modal(&[vec![1.0 / m_val.sqrt()]], &[omega], &[0.05]),
    ];

    for damping in &dampings {
        let result = nonlinear_time_history_analysis(
            &base,
            &dofmap0,
            &reducer0,
            &wave,
            &newmark,
            damping,
            DampingAccumulation::NonCumulative,
            &[50.0],
            &[0.0],
            NonlinearThCfg::new(20, 1e-6),
        )
        .expect("extended damping nonlinear TH should converge");
        assert_eq!(result.time.len(), n_steps + 1);
        assert_eq!(result.history.node_disp.len(), n_steps + 1);
        assert!(result.peak_disp[1][0].is_finite() && result.peak_disp[1][0] >= 1.0);
        assert!(result
            .history
            .node_disp
            .iter()
            .all(|value| value.is_finite()));
    }
}

/// 累積型/非累積型（減衰力の評価方式、構造動力学）: C 一定なら両者は一致し、接線比例（C 変化）でも
/// 累積型が収束して有限応答を返す。
#[test]
fn test_nonlinear_time_history_cumulative_vs_noncumulative() {
    let base = fiber_column_model(100.0);
    let dof0 = DofMap::build(&base);
    let red0 = Reducer::build(&base, &dof0);
    let m_red = red0.reduce_k(&assemble_global_m(&base, &dof0, MassOption::Consistent).unwrap());
    let k_red = red0.reduce_k(&assemble_global_k(&base, &dof0));
    let omega = (*k_red.get(0, 0).unwrap_or(&0.0) / *m_red.get(0, 0).unwrap_or(&1.0)).sqrt();
    let dt = 0.001;
    let n_steps = 30;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let run = |damping: &Damping, acc: DampingAccumulation| {
        nonlinear_time_history_analysis(
            &base,
            &dof0,
            &red0,
            &wave,
            &newmark,
            damping,
            acc,
            &[50.0],
            &[0.0],
            NonlinearThCfg::new(20, 1e-6),
        )
        .expect("should converge")
    };

    // C 一定（初期剛性比例）: 累積型 ≈ 非累積型。
    let const_c = Damping::StiffnessProportional {
        h: 0.05,
        omega,
        basis: StiffnessKind::Initial,
    };
    let non = run(&const_c, DampingAccumulation::NonCumulative);
    assert_eq!(non.non_converged_steps, 0);
    assert!(non.peak_disp[1][0].is_finite() && non.peak_disp[1][0] >= 1.0);
    assert_eq!(non.cumulative_ductility.len(), base.elements.len());
    assert!(
        non.cumulative_ductility.iter().any(|&damage| damage > 0.0),
        "塑性化した要素の累積損傷度が非ゼロであるべき: {:?}",
        non.cumulative_ductility
    );
    let cum = run(&const_c, DampingAccumulation::Cumulative);
    let non_peak = non.peak_disp[1][0];
    let cum_peak = cum.peak_disp[1][0];
    assert!(
        (non_peak - cum_peak).abs() < non_peak.abs() * 1e-4 + 1e-6,
        "constant C: non peak={non_peak}, cumulative peak={cum_peak}"
    );
    assert_eq!(non.time.len(), n_steps + 1);
    assert_eq!(non.time, cum.time);
    assert_eq!(non.history.node_disp.len(), n_steps + 1);
    assert_eq!(cum.history.node_disp.len(), n_steps + 1);
    let tolerance = non.peak_disp[1][0].abs() * 1e-4 + 1e-6;
    for (&non, &cum) in non.history.node_disp.iter().zip(&cum.history.node_disp) {
        assert!(
            (non - cum).abs() < tolerance,
            "constant C: non={non}, cum={cum}"
        );
    }

    // 接線比例（C 変化）でも累積型が収束し有限応答を返す。
    let tangent_c = Damping::StiffnessProportional {
        h: 0.05,
        omega,
        basis: StiffnessKind::Tangent,
    };
    let cum_t = run(&tangent_c, DampingAccumulation::Cumulative);
    assert_eq!(cum_t.time.len(), n_steps + 1);
    assert_eq!(cum_t.history.node_disp.len(), n_steps + 1);
    assert!(cum_t.peak_disp[1][0].is_finite() && cum_t.peak_disp[1][0] >= 1.0);
    assert!(cum_t
        .history
        .node_disp
        .iter()
        .all(|value| value.is_finite()));
}

/// 有限な試行応答でも未収束時は停止する。
#[test]
fn test_nonlinear_time_history_convergence() {
    let model = fiber_column_model(100.0);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let m_free = assemble_global_m(&model, &dofmap, MassOption::Consistent).unwrap();
    let k_free = assemble_global_k(&model, &dofmap);
    let m_red_lin = reducer.reduce_k(&m_free);
    let k_red_lin = reducer.reduce_k(&k_free);

    let k_val = *k_red_lin.get(0, 0).unwrap_or(&0.0);
    let m_val = *m_red_lin.get(0, 0).unwrap_or(&0.0);
    let omega = (k_val / m_val).sqrt();

    let damping = Damping::StiffnessProportional {
        h: 0.05,
        omega,
        basis: StiffnessKind::Initial,
    };

    let dt = 0.001;
    let n_steps = 30;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let result2 = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[50.0],
        &[0.0],
        NonlinearThCfg::new(1, 1e-6),
    );

    let error = result2
        .expect_err("有限な未収束応答を確定してはいけない")
        .to_string();
    assert!(error.contains("step 1"), "{error}");
    assert!(error.contains("時刻 0.001 s"), "{error}");
    assert!(error.contains("反復上限 1 回"), "{error}");
    assert!(!error.contains("非有限"), "{error}");
}

/// 制振（マクスウェル）ダンパーが自由振動の応答を低減する（制振要素、Maxwell モデル）。
#[test]
fn test_maxwell_damper_reduces_free_vibration() {
    use sepika_core::model::{DamperAttr, DamperKind, DamperProps};

    let run = |with_damper: bool| -> f64 {
        let mut model = sdof_model();
        if with_damper {
            let did = ElemId(model.elements.len() as u32);
            model.load_cfg = Some(sepika_core::model::LoadCfg {
                dampers: vec![sepika_core::model::DamperSpec {
                    elem: did,
                    total_weight: 0.0,
                }],
                ..Default::default()
            });
            model.elements.push(ElementData {
                id: did,
                kind: ElementKind::Damper,
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: None,
                local_axis: LocalAxis {
                    ref_vector: [0.0, 0.0, 1.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            });
            model.damper_attrs.push(DamperAttr {
                elem: did,
                props: DamperProps {
                    kind: DamperKind::Maxwell,
                    kd: 1000.0,
                    c0: 30.0,
                    alpha: 1.0,
                    ..Default::default()
                },
            });
        }
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let omega = (1000.0_f64 / 1.0).sqrt();
        let damping = Damping::StiffnessProportional {
            h: 0.001,
            omega,
            basis: StiffnessKind::Initial,
        };
        let dt = 0.001;
        let n_steps = 1000;
        let wave = zero_wave(dt, n_steps);
        let newmark = NewmarkCfg {
            beta: 0.25,
            gamma: 0.5,
            dt,
        };
        let result = nonlinear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            DampingAccumulation::NonCumulative,
            &[10.0],
            &[0.0],
            NonlinearThCfg::new(30, 1e-8),
        )
        .expect("should converge");
        // 後半区間の応答振幅（自由振動の減衰を測る）。
        let nd = &result.history.node_disp;
        let n = nd.len();
        nd[n * 3 / 4..].iter().fold(0.0f64, |m, &v| m.max(v.abs()))
    };

    let no_damp = run(false);
    let with_damp = run(true);
    assert!(
        no_damp > 1.0,
        "undamped late amplitude should be sizeable: {no_damp}"
    );
    assert!(
        with_damp < no_damp * 0.8,
        "Maxwell damper should reduce late response: no_damp={no_damp}, with_damp={with_damp}"
    );
}

/// (a) 線形時刻歴で詳細記録（`ThRecording`）が生成され、フレーム数・節点数・階数が
/// 期待どおりであること。`n_steps=50` では自動決定される `record_every` が
/// `(50/1000).max(1)=1` となり、全ステップが記録される。
/// 明示した間引き係数5/7、端部フレーム・全断面包絡・全step層ピークも検証する。
#[test]
fn test_linear_recording_frames_and_stories() {
    let model = fiber_column_model(1e10);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: 10.0,
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 50;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let result = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[10.0],
        &[0.0],
        false,
        None,
    )
    .expect("linear recording should run");

    assert!(!result.history.record_dir_y);
    let recording = result.recording.expect("recording should be present");
    assert_eq!(recording.record_every, 1, "n_steps=50 なら間引きなし");
    assert_eq!(recording.frame_time.len(), n_steps + 1);
    assert_eq!(recording.frame_time.first(), Some(&0.0));
    assert_eq!(recording.frame_time.last(), Some(&(n_steps as f64 * dt)));
    assert_eq!(recording.node_disp.len(), n_steps + 1);
    assert_eq!(recording.node_disp[0].len(), model.nodes.len());
    assert_eq!(recording.member_forces.len(), n_steps + 1);
    assert_eq!(recording.peak_member_forces.len(), model.elements.len());
    assert_eq!(recording.story_x.stories.len(), model.layer_count());
    assert_eq!(recording.story_y.stories.len(), model.layer_count());
    assert_eq!(recording.story_x.story_shear.len(), n_steps + 1);
    assert_eq!(recording.story_x.story_weight[0], 10000.0);

    let peak_mf = recording.peak_member_forces[0]
        .as_ref()
        .expect("peak member forces for element 0");
    assert!(
        peak_mf.at.len() >= 3,
        "envelope should keep all evaluation sections, got {}",
        peak_mf.at.len()
    );

    for frame in &recording.member_forces {
        let mf = frame[0]
            .as_ref()
            .expect("frame member forces for element 0");
        assert_eq!(
            mf.at.len(),
            2,
            "frame member_forces should be trimmed to endpoints only"
        );
        assert!((mf.at[0].0 - 0.0).abs() < 1e-9, "min xi should be 0.0");
        assert!((mf.at[1].0 - 1.0).abs() < 1e-9, "max xi should be 1.0");
    }

    let last = recording.story_x.story_shear.len() - 1;
    let total_shear: f64 = recording.story_x.story_shear[last].iter().sum();
    let expected = *result.history.base_shear.last().unwrap();
    assert!(
        (total_shear - expected).abs() < expected.abs().max(1.0) * 1e-6,
        "story shear sum {total_shear} should match base shear {expected}"
    );

    let n_story = recording.story_x.stories.len();
    assert!(n_story > 0);

    let mut expected_peak_shear = vec![0.0f64; n_story];
    let mut expected_peak_accel = vec![0.0f64; n_story];
    let mut expected_peak_vel = vec![0.0f64; n_story];
    let mut expected_peak_disp = vec![0.0f64; n_story];
    for frame in &recording.story_x.story_shear {
        for (p, &v) in expected_peak_shear.iter_mut().zip(frame) {
            *p = p.max(v.abs());
        }
    }
    for frame in &recording.story_x.floor_accel {
        for (p, &v) in expected_peak_accel.iter_mut().zip(frame) {
            *p = p.max(v.abs());
        }
    }
    for frame in &recording.story_x.floor_vel {
        for (p, &v) in expected_peak_vel.iter_mut().zip(frame) {
            *p = p.max(v.abs());
        }
    }
    for frame in &recording.story_x.floor_disp {
        for (p, &v) in expected_peak_disp.iter_mut().zip(frame) {
            *p = p.max(v.abs());
        }
    }

    for (record_every, expected_frames) in [(5, 11), (7, 9)] {
        let result_thin = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            &[10.0],
            &[0.0],
            false,
            Some(record_every),
        )
        .expect("linear recording with explicit record_every should run");
        let recording_thin = result_thin.recording.expect("recording should be present");
        assert_eq!(recording_thin.record_every, record_every);
        assert_eq!(recording_thin.frame_time.len(), expected_frames);
        assert_eq!(recording_thin.story_x.story_shear.len(), expected_frames);
        assert_eq!(
            recording_thin.frame_time.first(),
            recording.frame_time.first()
        );
        assert_eq!(
            recording_thin.frame_time.last(),
            recording.frame_time.last()
        );
        for i in 0..n_story {
            assert!(
                (recording_thin.story_x.peak_story_shear[i] - expected_peak_shear[i]).abs()
                    < expected_peak_shear[i].abs().max(1.0) * 1e-9,
                "story{i} peak shear: thin={} full-derived={}",
                recording_thin.story_x.peak_story_shear[i],
                expected_peak_shear[i]
            );
            assert!(
                (recording_thin.story_x.peak_floor_accel[i] - expected_peak_accel[i]).abs()
                    < expected_peak_accel[i].abs().max(1.0) * 1e-9
            );
            assert!(
                (recording_thin.story_x.peak_floor_vel[i] - expected_peak_vel[i]).abs()
                    < expected_peak_vel[i].abs().max(1.0) * 1e-9
            );
            assert!(
                (recording_thin.story_x.peak_floor_disp[i] - expected_peak_disp[i]).abs()
                    < expected_peak_disp[i].abs().max(1.0) * 1e-9
            );
        }
        assert_eq!(
            recording_thin.story_x.peak_story_shear,
            recording.story_x.peak_story_shear
        );
        assert_eq!(
            recording_thin.story_x.peak_floor_accel,
            recording.story_x.peak_floor_accel
        );
    }
}

/// (c) 非線形時刻歴 + `apply_long_term` で、長期荷重（Dead）載荷後の初期変位が
/// 独立に求めた静的線形解と一致すること（fy=1e10 の弾性モデルなので線形解に近い）。
/// 動的加振・初期変位ともにゼロのため、応答は長期解のまま静止するはず。
#[test]
fn test_nonlinear_apply_long_term_matches_static_solution() {
    let mut model = fiber_column_model(1e10);
    model.load_cases.push(sepika_core::model::LoadCase {
        origin: Default::default(),
        id: sepika_core::ids::LoadCaseId(0),
        name: "DL".to_string(),
        nodal: vec![sepika_core::model::NodalLoad::manual(
            NodeId(1),
            [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
        member: vec![],
        kind: sepika_core::model::LoadCaseKind::Dead,
    });
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    // 独立に静的線形解を求める（弾性なので長期荷重の非線形解とほぼ一致するはず）。
    let k_free = assemble_global_k(&model, &dofmap);
    let k_red = reducer.reduce_k(&k_free);
    let f_free = crate::common::assemble::assemble_global_f(
        &model,
        &dofmap,
        sepika_core::ids::LoadCaseId(0),
    );
    let f_red = reducer.reduce_f(&f_free);
    let k_val = *k_red.get(0, 0).unwrap_or(&0.0);
    let expected_u = f_red[0] / k_val;

    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: 10.0,
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 20;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let mut cfg = NonlinearThCfg::new(20, 1e-9);
    cfg.apply_long_term = true;
    let result = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[0.0],
        &[0.0],
        cfg,
    )
    .expect("apply_long_term should converge");

    assert!(
        (result.peak_disp[1][0] - expected_u).abs() / expected_u.abs() < 0.05,
        "long-term peak {} should match static solution {}",
        result.peak_disp[1][0],
        expected_u
    );
    assert!(
        result.history.base_shear[0].abs() < 1e-6,
        "無地動の長期釣合いで初期慣性力が生じないこと: {}",
        result.history.base_shear[0]
    );
    // 動的加振も初期変位もゼロなので、時刻歴を通じて長期解のまま静止する。
    let recording = result.recording.expect("recording");
    for frame in &recording.node_disp {
        assert!(
            (frame[1][0] - expected_u).abs() / expected_u.abs() < 0.05,
            "should stay at long-term equilibrium: {} vs {}",
            frame[1][0],
            expected_u
        );
    }
}

/// 支点ばね（`Node::support_spring`）のみで水平剛性を与えられる SDOF。
fn support_spring_sdof_model(k: f64, m: f64) -> Model {
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [1000.0, 0.0, 0.0],
                restraint: FREE_UX,
                mass: Some([m, 0.0, 0.0, 0.0, 0.0, 0.0]),
                story: None,
                support_spring: Some([k, 0.0, 0.0, 0.0, 0.0, 0.0]),
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::NodalSpring,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Pinned, EndCondition::Pinned],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: Some([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        }],
        ..Default::default()
    }
}

/// 支点ばねのみで支持される SDOF で、非線形時刻歴の長期荷重初期化が収束し、静的解析解 `u=F/k` と一致すること。
#[test]
fn test_support_spring_long_term_static_matches_analytical() {
    let k = 500.0_f64;
    let m = 2.0_f64;
    let f_load = 300.0_f64;
    let mut model = support_spring_sdof_model(k, m);
    model.load_cases.push(sepika_core::model::LoadCase {
        origin: Default::default(),
        id: sepika_core::ids::LoadCaseId(0),
        name: "DL".to_string(),
        nodal: vec![sepika_core::model::NodalLoad::manual(
            NodeId(1),
            [f_load, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
        member: vec![],
        kind: sepika_core::model::LoadCaseKind::Dead,
    });
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: (k / m).sqrt(),
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 5;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let mut cfg = NonlinearThCfg::new(20, 1e-9);
    cfg.apply_long_term = true;
    let result = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[0.0],
        &[0.0],
        cfg,
    )
    .expect("支点ばねのみのモデルでも長期荷重の静的載荷が収束するはず（高-1）");

    let expected_u = f_load / k;
    assert!(
        (result.peak_disp[1][0] - expected_u).abs() / expected_u.abs() < 1e-6,
        "long-term disp {} should match analytical F/k={}",
        result.peak_disp[1][0],
        expected_u
    );
}

/// 支点ばねのみで支持される SDOF の自由振動応答が、同一モデルの線形時刻歴と一致すること。
#[test]
fn test_support_spring_free_vibration_matches_linear() {
    let k = 800.0_f64;
    let m = 1.0_f64;
    let model = support_spring_sdof_model(k, m);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let omega = (k / m).sqrt();
    let damping = Damping::StiffnessProportional {
        h: 0.03,
        omega,
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 30;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let mut cfg = NonlinearThCfg::new(20, 1e-10);
    cfg.apply_long_term = false;
    let result_nl = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[5.0],
        &[0.0],
        cfg,
    )
    .expect("支点ばねのみのモデルで自由振動が解けるはず（高-1）");
    assert!(result_nl.nonlinear);
    assert!(!result_nl.applied_long_term, "apply_long_term=false を反映");

    let result_lin = linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        &[5.0],
        &[0.0],
        false,
        None,
    )
    .expect("linear reference");

    assert_eq!(
        result_nl.history.node_disp.len(),
        result_lin.history.node_disp.len()
    );
    for (u_nl, u_lin) in result_nl
        .history
        .node_disp
        .iter()
        .zip(result_lin.history.node_disp.iter())
    {
        assert!(
            (u_nl - u_lin).abs() < 1e-4 * u_lin.abs().max(1.0),
            "nonlinear {u_nl} should match linear {u_lin}"
        );
    }
}

/// (d) `NonlinearThCfg::record_every` による明示的な間引きが機能すること。
/// `n_steps=50, record_every=5` なら、ステップ 0,5,...,50 の 11 フレームが記録される。
#[test]
fn test_nonlinear_record_every_thinning() {
    let model = fiber_column_model(1e10);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);

    let damping = Damping::StiffnessProportional {
        h: 0.02,
        omega: 10.0,
        basis: StiffnessKind::Initial,
    };
    let dt = 0.001;
    let n_steps = 50;
    let wave = zero_wave(dt, n_steps);
    let newmark = NewmarkCfg {
        beta: 0.25,
        gamma: 0.5,
        dt,
    };

    let mut cfg = NonlinearThCfg::new(20, 1e-6);
    cfg.record_every = Some(5);
    let result = nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &newmark,
        &damping,
        DampingAccumulation::NonCumulative,
        &[10.0],
        &[0.0],
        cfg,
    )
    .expect("should converge");

    assert!(result.nonlinear);
    assert!(
        result.applied_long_term,
        "長期系荷重ケースがなくても apply_long_term=true を反映（載荷有無ではなく設定値）"
    );

    let recording = result.recording.expect("recording");
    assert_eq!(recording.record_every, 5);
    assert_eq!(recording.frame_time.len(), 11, "0,5,..,50 の 11 フレーム");
    // peak 系は全ステップ更新（間引かない）。
    assert!(result.peak_disp[1][0] > 0.0);
    assert!(recording.peak_member_forces.iter().any(|f| f.is_some()));
}

#[test]
fn rc_reference_actual_entries_use_material_strength_for_pushover_and_nominal_for_th() {
    use sepika_core::model::{HysteresisModel, RcBeamReference};
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    let mut model = sdof_model();
    model.nodes[1].coord = [6000.0, 0.0, 0.0];
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![3],
            bottom: vec![3],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "RC基準".into());
    section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections[0] = section;
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].young = 20000.0;
    model.materials[0].fc = Some(24.0);
    model.materials[0].fy = None;
    model.materials[0].density = 2.4e-9;
    let mut rebar = model.materials[0].clone();
    rebar.id = MaterialId(1);
    rebar.name = "SD345".into();
    rebar.category = MaterialCategory::Rebar;
    rebar.young = 200000.0;
    rebar.fc = None;
    rebar.fy = Some(345.0);
    model.materials.push(rebar);
    model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    model.set_member_hysteresis(ElemId(1), HysteresisModel::Takeda);
    model.set_member_rc_beam_reference(ElemId(1), Some(RcBeamReference::AntisymmetricHalfMember));
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    for factor in [0.1, 0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        model.materials[1].strength_factor = Some(factor);
        let reason = crate::nonlinear::pushover::pushover_analysis(
            &model,
            &dofmap,
            &reducer,
            crate::statics::analysis::SeismicDir::X,
            2,
            0.0,
            false,
        )
        .unwrap_err();
        assert!(reason.contains("My"), "{reason}");
        let dt = 0.01;
        let response = nonlinear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &zero_wave(dt, 3),
            &NewmarkCfg {
                beta: 0.25,
                gamma: 0.5,
                dt,
            },
            &Damping::StiffnessProportional {
                h: 0.0,
                omega: 1.0,
                basis: StiffnessKind::Initial,
            },
            DampingAccumulation::NonCumulative,
            &[0.0],
            &[0.0],
            NonlinearThCfg::new(20, 1e-6),
        )
        .unwrap();
        assert!(!response.time.is_empty());
        assert_eq!(response.non_converged_steps, 0);
    }
}

mod loaded_recording;
