use super::*;
use approx::assert_relative_eq;
use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_material::{Bilinear, Concrete, UniaxialMaterial};
use sepika_section::fiber::rect_fiber_section;

fn make_section(w: f64, d: f64) -> Section {
    Section {
        frame_use: None,
        id: SectionId(0),
        name: "test".into(),
        area: w * d,
        iy: w * d.powi(3) / 12.0,
        iz: d * w.powi(3) / 12.0,
        j: w.powi(3) * d / 3.0,
        depth: d,
        width: w,
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
    }
}

#[test]
fn test_member_skeleton_generic_basic() {
    let sec = make_section(100.0, 200.0);
    let mat_data = Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: sepika_core::ids::MaterialId(0),
        name: "steel".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: None,
    };
    let fibers = rect_fiber_section(100.0, 200.0, 10, 20, 0);
    let reinforcement = Reinforcement {
        main_bars: vec![],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let mut member = MemberData {
        section: &sec,
        reinforcement: &reinforcement,
        material: &mat_data,
        fibers: &fibers,
        span: 4000.0,
        inflection_ratio: 0.5,
    };
    let template = Bilinear::new(205000.0, 235.0, 0.01);
    let mut mats: Vec<Box<dyn UniaxialMaterial>> = (0..fibers.fibers.len())
        .map(|_| template.clone_box())
        .collect();
    let skeleton = build_member_skeleton(&member, 0.0, &mut mats, 0.4).unwrap();
    assert!(!skeleton.points.is_empty());
    assert!(skeleton.points.last().unwrap().1 >= skeleton.points.first().unwrap().1);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(build_member_skeleton(&member, 0.0, &mut mats, value).is_err());
    }
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        member.span = value;
        assert!(build_member_skeleton(&member, 0.0, &mut mats, 0.4).is_err());
    }
}

#[test]
fn test_rc_skeleton_yield_matches_handcalc() {
    // 代表 RC 梁: b=300, D=500, 引張鉄筋 4-D19 (As≈4×283.5=1134 mm²), fy=345, E=200000
    // 手計算 My = at·σy·j, j=7d/8, d=D-cover-φ/2 = 500-50-9.5 = 440.5
    let b = 300.0;
    let d_total = 500.0;
    let cover = 50.0;
    let bar_dia: f64 = 19.0;
    let n_bars = 4;
    let as_bar: f64 = std::f64::consts::PI * (bar_dia / 2.0).powi(2);
    let at = n_bars as f64 * as_bar;
    let d = d_total - cover - bar_dia / 2.0;
    let j = 7.0 * d / 8.0;
    let fy = 345.0;
    let e_steel = 200000.0;
    let my_handcalc = at * fy * j; // [N·mm]

    let sec = make_section(b, d_total);
    // 引張鉄筋位置: 上端側 z = +(d - D/2) = +190.5（正曲率 ky>0 で上端が引張となる符号規約）
    let z_tension = d - d_total / 2.0;
    let rebar = Reinforcement {
        main_bars: (0..n_bars)
            .map(|i| {
                let y = (i as f64 - (n_bars as f64 - 1.0) / 2.0) * (b - 2.0 * cover)
                    / (n_bars as f64 - 1.0).max(1.0);
                (y, z_tension, as_bar)
            })
            .collect(),
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(e_steel, fy, 0.01);
    let opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let skeleton = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();

    // 降伏点のモーメント（points[2]）が手計算と概ね一致（離散化・j近似で 15% 以内）
    let my_fiber = skeleton.points.get(2).map(|p| p.1).unwrap_or(0.0);
    let ratio = my_fiber / my_handcalc;
    assert!(
        ratio > 0.85 && ratio < 1.15,
        "My fiber ({:.3} N·mm) vs handcalc ({:.3}): ratio={:.3}",
        my_fiber,
        my_handcalc,
        ratio
    );
}

#[test]
fn test_rc_skeleton_trilinear_shape() {
    let sec = make_section(300.0, 500.0);
    let rebar = Reinforcement {
        main_bars: vec![
            (0.0, 190.0, 283.5),
            (-90.0, 190.0, 283.5),
            (90.0, 190.0, 283.5),
        ],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, 345.0, 0.01);
    let opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let skeleton = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();

    // 4 点（原点+3折点）で昇順
    assert_eq!(skeleton.points.len(), 4);
    for w in skeleton.points.windows(2) {
        assert!(w[0].0 <= w[1].0, "theta must be ascending");
        assert!(w[0].1 <= w[1].1 + 1e-6, "M must be non-decreasing");
    }
    // ひび割れ < 降伏 < 終局
    assert!(skeleton.points[1].1 < skeleton.points[2].1);
    assert!(skeleton.points[2].1 < skeleton.points[3].1);
}

#[test]
fn test_rc_skeleton_axial_dependency() {
    let sec = make_section(300.0, 500.0);
    let rebar = Reinforcement {
        main_bars: vec![(0.0, 190.0, 283.5)],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, 345.0, 0.01);
    let opts0 = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let opts1 = SkeletonOptions {
        n_axial: -500_000.0, // 圧縮軸力
        ..opts0
    };
    let sk_n0 = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts0,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();
    let sk_n1 = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts1,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();
    // 軸力により降伏モーメントが変化する
    let my_n0 = sk_n0.points[2].1;
    let my_n1 = sk_n1.points[2].1;
    assert!(
        (my_n0 - my_n1).abs() / my_n0.max(1.0) > 1e-3,
        "axial force should change My: N0={}, N1={}",
        my_n0,
        my_n1
    );
}

#[test]
fn test_rc_skeleton_deformation_contributions_increase_rotation() {
    // せん断変形・鉄筋抜出しはいずれも降伏回転角 θy を増加させる（M は同一）。
    let sec = make_section(300.0, 500.0);
    let rebar = Reinforcement {
        main_bars: vec![
            (0.0, 190.0, 283.5),
            (-90.0, 190.0, 283.5),
            (90.0, 190.0, 283.5),
        ],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, 345.0, 0.01);
    let opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let sk_base = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();

    // せん断変形を加えると θy が増加する。
    let sk_with_shear = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::rc_rect(300.0, 500.0, &concrete).unwrap(),
        &PulloutContribution::none(),
    )
    .unwrap();
    assert!(
        sk_with_shear.points[2].0 > sk_base.points[2].0,
        "shear contribution must increase θy: base={}, with={}",
        sk_base.points[2].0,
        sk_with_shear.points[2].0
    );
    // M は同一（せん断は変形のみ加算）
    assert_relative_eq!(
        sk_base.points[2].1,
        sk_with_shear.points[2].1,
        epsilon = 1e-3
    );

    // 鉄筋抜出しを加えても θy が増加する。
    let pullout = PulloutContribution::explicit(
        pullout_point(0.2, 500.0),
        pullout_point(1.0, 500.0),
        pullout_point(2.0, 500.0),
    );
    let sk_with_pullout = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &pullout,
    )
    .unwrap();
    assert!(
        sk_with_pullout.points[2].0 > sk_base.points[2].0,
        "pullout must increase θy: base={}, with={}",
        sk_base.points[2].0,
        sk_with_pullout.points[2].0
    );
    let sk_combined = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::Stiffness { k_s: 1e9 },
        &pullout,
    )
    .unwrap();
    for (i, slip_angle) in [(1, 0.0004), (2, 0.002), (3, 0.004)] {
        assert_eq!(sk_combined.points[i].1, sk_base.points[i].1);
        let delta = sk_combined.points[i].0 - sk_base.points[i].0;
        assert_relative_eq!(
            delta,
            sk_base.points[i].1 / 2e12 + slip_angle,
            epsilon = 1e-15
        );
    }
}

#[test]
fn test_rc_skeleton_ultimate_matches_handcalc() {
    // 終局モーメント Mu が規準式 Mu ≈ a_t·σy·j（引張鉄筋降伏型、係数 0.9 系）と照合。
    // 降伏型破壊（a_t が少なめ）の RC 梁で Mu は My の 1.0〜1.2 倍程度。
    // 規準式: Mu = 0.9·a_t·σy·j （AIJ『非線形解析指針』等の簡易式）
    let b = 300.0;
    let d_total = 500.0;
    let cover = 50.0;
    let bar_dia: f64 = 19.0;
    let n_bars = 4;
    let as_bar: f64 = std::f64::consts::PI * (bar_dia / 2.0).powi(2);
    let at = n_bars as f64 * as_bar;
    let d = d_total - cover - bar_dia / 2.0;
    let j = 7.0 * d / 8.0;
    let fy = 345.0;
    let mu_handcalc = 0.9 * at * fy * j; // [N·mm]

    let sec = make_section(b, d_total);
    let z_tension = d - d_total / 2.0;
    let rebar = Reinforcement {
        main_bars: (0..n_bars)
            .map(|i| {
                let y = (i as f64 - (n_bars as f64 - 1.0) / 2.0) * (b - 2.0 * cover)
                    / (n_bars as f64 - 1.0).max(1.0);
                (y, z_tension, as_bar)
            })
            .collect(),
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, fy, 0.01);
    let opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let skeleton = build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &PulloutContribution::none(),
    )
    .unwrap();

    let mu_fiber = skeleton.points.get(3).map(|p| p.1).unwrap_or(0.0);
    let ratio = mu_fiber / mu_handcalc;
    // 0.9·a_t·σy·j は近似式。ファイバ積分は圧縮側コンクリートも寄与するため
    // Mu は My の 1.0〜1.3 倍程度。規準式との一致は 30% 以内を許容。
    assert!(
        ratio > 0.7 && ratio < 1.3,
        "Mu fiber ({:.3} N·mm) vs handcalc 0.9·at·σy·j ({:.3}): ratio={:.3}",
        mu_fiber,
        mu_handcalc,
        ratio
    );
}

fn pullout_point(slip_mm: f64, lever_arm_mm: f64) -> PulloutPoint {
    PulloutPoint {
        slip_mm,
        lever_arm_mm,
        source: "検証用の明示モデル".into(),
        lever_arm_definition: "引張鉄筋位置から採用回転中心までの正の距離".into(),
        rotation_center: "当該モデルの回転中心（中立軸と自動同一視しない）".into(),
    }
}

#[test]
fn shear_rotation_independent_units_length_and_sign() {
    let shear = ShearContribution::Stiffness { k_s: 1e9 };
    let q_n = 1e8 / 2000.0;
    assert_eq!(q_n, 50000.0);
    assert_relative_eq!(q_n * 2000.0 / 1e9, 0.1, epsilon = 1e-15);
    assert_relative_eq!(shear.rotation(1e8, 2000.0).unwrap(), 5e-5, epsilon = 1e-15);
    assert_relative_eq!(
        shear.rotation(1e8, 4000.0).unwrap(),
        2.5e-5,
        epsilon = 1e-15
    );
    assert_relative_eq!(shear.rotation(2e8, 4000.0).unwrap(), 5e-5, epsilon = 1e-15);
    assert_relative_eq!(
        shear.rotation(-1e8, 2000.0).unwrap(),
        -5e-5,
        epsilon = 1e-15
    );
    let converted =
        crate::deformation::mphi_to_mtheta(0.0, 1e8, None, 4000.0, 0.5, 250.0, shear, 0.0).unwrap();
    assert_eq!(converted.1, 1e8);
    assert_relative_eq!(converted.0, 5e-5, epsilon = 1e-15);
}

#[test]
fn shear_invalid_inputs_are_distinct_from_none() {
    let shear = ShearContribution::Stiffness { k_s: 1e9 };
    for moment in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            shear.rotation(moment, 2000.0),
            Err(DeformationError::InvalidInput("モーメントは有限値が必要"))
        );
        assert_eq!(
            crate::deformation::mphi_to_mtheta(0.0, moment, None, 4000.0, 0.5, 250.0, shear, 0.0,),
            Err(DeformationError::InvalidInput("モーメントは有限値が必要"))
        );
    }
    for ratio in [0.0, -1.0, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            crate::deformation::mphi_to_mtheta(0.0, 1e8, None, 4000.0, ratio, 250.0, shear, 0.0,),
            Err(DeformationError::InvalidInput(_))
        ));
    }
    let invalid = [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for value in invalid {
        assert!(matches!(
            ShearContribution::Stiffness { k_s: value }.rotation(1e8, 2000.0),
            Err(DeformationError::InvalidInput(_))
        ));
        assert!(ShearContribution::Stiffness { k_s: 1e9 }
            .rotation(1e8, value)
            .is_err());
        assert!(ShearContribution::none().rotation(0.0, value).is_err());
        let concrete = Concrete::new(30.0, 2.0);
        assert!(ShearContribution::rc_rect(value, 500.0, &concrete).is_err());
        assert!(ShearContribution::rc_rect(300.0, value, &concrete).is_err());
        assert!(ShearContribution::rc_rect(300.0, 500.0, &Concrete::new(value, 2.0)).is_err());
        let mut concrete = concrete;
        concrete.ec0 = if value == -1.0 { 1.0 } else { value };
        assert!(ShearContribution::rc_rect(300.0, 500.0, &concrete).is_err());
    }
    assert_eq!(
        ShearContribution::none().rotation(1e8, 2000.0).unwrap(),
        0.0
    );
    let concrete = Concrete::new(30.0, 2.0);
    // E0=30000、G=12500、As=125000。1.2をもう一度算入しない。
    assert_relative_eq!(
        ShearContribution::rc_rect(300.0, 500.0, &concrete)
            .unwrap()
            .rotation(1e8, 2000.0)
            .unwrap(),
        0.000032,
        epsilon = 1e-15
    );
}

#[test]
fn explicit_pullout_units_sign_zero_and_model_metadata() {
    assert_eq!(pullout_point(1.0, 500.0).rotation().unwrap(), 0.002);
    assert_eq!(pullout_point(-1.0, 500.0).rotation().unwrap(), -0.002);
    assert_eq!(pullout_point(0.0, 500.0).rotation().unwrap(), 0.0);
    for z in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(pullout_point(1.0, z).rotation().is_err());
        assert!(pullout_point(0.0, z).rotation().is_err());
    }
    for s in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(pullout_point(s, 500.0).rotation().is_err());
    }
    for missing in 0..3 {
        let mut point = pullout_point(1.0, 500.0);
        match missing {
            0 => point.source.clear(),
            1 => point.lever_arm_definition.clear(),
            _ => point.rotation_center.clear(),
        }
        assert!(matches!(
            point.rotation(),
            Err(DeformationError::InvalidInput(_))
        ));
    }
    assert!(matches!(
        PulloutContribution::AutomaticBond.rotations(),
        Err(DeformationError::Unsupported(_))
    ));
    assert_eq!(PulloutContribution::none().rotations().unwrap(), [0.0; 3]);
}

#[test]
fn anchorage_1997_sufficient_length_branch_independent_example() {
    // 1997 p.88 式(14)〜(17)の単一丸鋼・十分な定着長の枝だけを検証する。
    let lt_mm = 400.0 * 20.0 / (4.0 * 2.0);
    let slip_mm = (400.0 / 200000.0) * lt_mm / 2.0;
    assert_eq!(lt_mm, 1000.0);
    assert_eq!(slip_mm, 1.0);
    assert!(lt_mm <= 1000.0 + 600.0 / 4.0);
    assert_eq!(pullout_point(slip_mm, 500.0).rotation().unwrap(), 0.002);
}

#[test]
fn positive_reference_composition_has_independent_expected_rotations() {
    let angles = [0.0004, 0.002, 0.004];
    let input = [(3e-7, 2e7), (1e-6, 1e8), (2e-6, 1.2e8)];
    let expected = [0.00061, 0.0027166666666666667, 0.004976666666666667];
    for ((curvature, moment), (pullout, expected)) in
        input.into_iter().zip(angles.into_iter().zip(expected))
    {
        let point = crate::deformation::mphi_to_mtheta(
            curvature,
            moment,
            Some(1e-6),
            4000.0,
            0.5,
            250.0,
            ShearContribution::Stiffness { k_s: 1e9 },
            pullout,
        )
        .unwrap();
        assert_relative_eq!(point.0, expected, epsilon = 1e-15);
        assert_eq!(point.1, moment);
    }
}

#[test]
fn rc_builder_rejects_unsupported_pullout_and_invalid_length_before_generation() {
    let sec = make_section(300.0, 500.0);
    let rebar = Reinforcement {
        main_bars: vec![(0.0, 190.0, 283.5)],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, 345.0, 0.01);
    let mut opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    assert!(matches!(
        build_rc_member_skeleton(
            &sec,
            &rebar,
            &concrete,
            &steel,
            &opts,
            &ShearContribution::none(),
            &PulloutContribution::AutomaticBond
        ),
        Err(DeformationError::Unsupported(_))
    ));
    for length in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        opts.span = length;
        assert!(matches!(
            build_rc_member_skeleton(
                &sec,
                &rebar,
                &concrete,
                &steel,
                &opts,
                &ShearContribution::none(),
                &PulloutContribution::none()
            ),
            Err(DeformationError::InvalidInput(_))
        ));
    }
}

#[test]
fn rc_builder_rejects_reversed_composed_points_and_missing_pullout_model() {
    let sec = make_section(300.0, 500.0);
    let rebar = Reinforcement {
        main_bars: vec![(0.0, 190.0, 283.5)],
        hoop_pitch: 100.0,
        hoop_area: 0.0,
    };
    let concrete = Concrete::new(30.0, 2.0);
    let steel = Bilinear::new(200000.0, 345.0, 0.01);
    let opts = SkeletonOptions {
        span: 4000.0,
        inflection_ratio: 0.5,
        n_axial: 0.0,
        alpha: 0.4,
    };
    let reversed = PulloutContribution::explicit(
        pullout_point(500.0, 500.0),
        pullout_point(0.0, 500.0),
        pullout_point(0.0, 500.0),
    );
    assert!(matches!(
        build_rc_member_skeleton(
            &sec,
            &rebar,
            &concrete,
            &steel,
            &opts,
            &ShearContribution::none(),
            &reversed
        ),
        Err(DeformationError::InvalidInput(
            "合成後の参考骨格の折点が昇順でない"
        ))
    ));
    let missing = PulloutContribution::explicit(
        pullout_point(0.0, 500.0),
        pullout_point(1.0, 0.0),
        pullout_point(2.0, 500.0),
    );
    assert!(build_rc_member_skeleton(
        &sec,
        &rebar,
        &concrete,
        &steel,
        &opts,
        &ShearContribution::none(),
        &missing
    )
    .is_err());
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut bad_section = sec.clone();
        bad_section.width = invalid;
        assert!(build_rc_member_skeleton(
            &bad_section,
            &rebar,
            &concrete,
            &steel,
            &opts,
            &ShearContribution::none(),
            &PulloutContribution::none()
        )
        .is_err());
        bad_section = sec.clone();
        bad_section.depth = invalid;
        assert!(build_rc_member_skeleton(
            &bad_section,
            &rebar,
            &concrete,
            &steel,
            &opts,
            &ShearContribution::none(),
            &PulloutContribution::none()
        )
        .is_err());
        let bad_steel = Bilinear::new(invalid, 345.0, 0.01);
        assert!(build_rc_member_skeleton(
            &sec,
            &rebar,
            &concrete,
            &bad_steel,
            &opts,
            &ShearContribution::none(),
            &PulloutContribution::none()
        )
        .is_err());
    }
}
