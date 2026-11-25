use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Node, RigidZone, Section,
};
use sepika_core::section_shape::{
    BeamStirrup, CircleColumnHoop, RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar, RebarSize,
    RectColumnHoop, SectionShape, StandardRebar,
};
use smallvec::SmallVec;

/// テスト用の梁矩形 RC 断面（上下各 `main_count` 本、帯筋 D10@pitch）。
fn rc_beam_rect_section(
    id: u32,
    b: f64,
    d: f64,
    main_dia: f64,
    main_count: u32,
    pitch: f64,
) -> Section {
    let rebar = RcBeamRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
        top: vec![main_count],
        bottom: vec![main_count],
        cover: 40.0,
        stirrup: BeamStirrup {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch,
            legs: 2,
        },
    };
    Section {
        frame_use: Some(sepika_core::model::FrameSectionUse::Girder),
        id: SectionId(id),
        name: format!("RC{id}"),
        area: b * d,
        iy: b * d.powi(3) / 12.0,
        iz: d * b.powi(3) / 12.0,
        j: 1.0,
        depth: d,
        width: b,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: Some(SectionShape::RcBeamRect { b, d, rebar }),
        material: Some(MaterialId(0)),
        rebar_material: Some(MaterialId(1)),
        shear_rebar_material: Some(MaterialId(1)),
        steel_material: None,
        property_basis: Default::default(),
    }
}

/// テスト用の柱矩形 RC 断面（`x:[nx]`・`y:[ny]`、帯筋 D10@pitch）。
fn rc_column_rect_section(
    id: u32,
    b: f64,
    d: f64,
    main_dia: f64,
    nx: u32,
    ny: u32,
    pitch: f64,
) -> Section {
    let rebar = RcRectColumnRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
        x: vec![nx],
        y: vec![ny],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch,
            legs_x: 2,
            legs_y: 2,
        },
    };
    Section {
        frame_use: Some(sepika_core::model::FrameSectionUse::Column),
        id: SectionId(id),
        name: format!("RC{id}"),
        area: b * d,
        iy: b * d.powi(3) / 12.0,
        iz: d * b.powi(3) / 12.0,
        j: 1.0,
        depth: d,
        width: b,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: Some(SectionShape::RcColumnRect { b, d, rebar }),
        material: Some(MaterialId(0)),
        rebar_material: Some(MaterialId(1)),
        shear_rebar_material: Some(MaterialId(1)),
        steel_material: None,
        property_basis: Default::default(),
    }
}

fn material() -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(1),
        name: "SD345".to_string(),
        category: MaterialCategory::Rebar,
        young: 21000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: None,
        fy: Some(345.0),
    }
}

fn concrete_material() -> Material {
    Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        fc: Some(24.0),
        fy: None,
        ..material()
    }
}

fn node(id: u32, c: [f64; 3]) -> Node {
    Node {
        id: NodeId(id),
        coord: c,
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    }
}

fn frame_element(id: u32, sec: u32, n0: u32, n1: u32) -> ElementData {
    let mut nodes: SmallVec<[NodeId; 8]> = SmallVec::new();
    nodes.push(NodeId(n0));
    nodes.push(NodeId(n1));
    ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes,
        section: Some(SectionId(sec)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone {
            face_i: Some(0.0),
            face_j: Some(0.0),
            ..Default::default()
        },
        end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
        plastic_zone: None,
        spring: None,
    }
}

/// 1 柱（鉛直）+ 1 梁（水平）のモデル。
pub(super) fn column_and_beam_model() -> Model {
    let nodes = vec![
        node(0, [0.0, 0.0, 0.0]),
        node(1, [0.0, 0.0, 3000.0]),
        node(2, [6000.0, 0.0, 3000.0]),
    ];
    let sections = vec![
        rc_column_rect_section(0, 600.0, 600.0, 25.0, 4, 4, 100.0),
        rc_beam_rect_section(1, 400.0, 700.0, 25.0, 4, 100.0),
    ];
    let materials = vec![concrete_material(), material()];
    let elements = vec![frame_element(0, 0, 0, 1), frame_element(1, 1, 1, 2)];
    Model {
        load_cases: sepika_core::model::default_load_cases(),
        combinations: sepika_core::model::default_combinations(),
        ultimate_gravity_combination: Some(0),
        nodes,
        elements,
        sections,
        materials,
        ..Default::default()
    }
}

#[test]
fn test_collect_rc_ultimate_checks_column_and_beam() {
    let mut model = column_and_beam_model();
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[2].restraint = Dof6Mask::FIXED;
    let opts = UltimateShearOptions::default();
    let axial = vec![
        (
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(2_000_000.0)
            },
        ),
        (
            ElemId(1),
            MemberDemand {
                mz: 1.0e6,
                static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::IBottomJTop),
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        ),
    ];
    let checks = collect_rc_ultimate_checks(&model, &axial, &opts).unwrap();
    assert_eq!(checks.len(), 2, "柱・梁の 2 部材が検定される");

    let col = checks.iter().find(|c| c.elem == ElemId(0)).unwrap();
    let beam = checks.iter().find(|c| c.elem == ElemId(1)).unwrap();

    assert_eq!(col.kind, MemberKind::Column);
    assert_eq!(beam.kind, MemberKind::Girder);

    assert!(col.mu > 0.0 && col.qmu > 0.0 && col.qsu.is_nan() && col.bond_stress.is_none());
    assert!(beam.mu > 0.0 && beam.qmu.is_nan() && beam.qsu.is_nan());
    assert!(!beam.ok && beam.unchecked_reason.is_some());

    let ax = col.axial.expect("柱は軸終局耐力を持つ");
    assert!((ax.nuc - 600.0 * 600.0 * 24.0).abs() < 1e-3);
    assert!(ax.nut < 0.0);
    assert!(beam.axial.is_none());

    assert!(col.shear_margin.is_nan());
}

#[test]
fn test_ultimate_sr235_requires_fy() {
    let confirmed = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 1e6, 0.0, 1000.0, 0.0, 0.0)
    };
    let demand = [(ElemId(0), confirmed), (ElemId(1), confirmed)];
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let model_with_shear = |name: &str, fy: Option<f64>| -> Model {
        let mut model = column_and_beam_model();
        model.materials.push(Material {
            id: MaterialId(model.materials.len() as u32),
            name: name.to_string(),
            fy,
            ..material()
        });
        let shear_id = MaterialId(model.materials.len() as u32 - 1);
        model.sections[1].shear_rebar_material = Some(shear_id);
        model
    };

    let unavailable =
        collect_rc_ultimate_checks(&model_with_shear("SR235", None), &demand, &opts).unwrap();
    let column = unavailable.iter().find(|c| c.elem == ElemId(0)).unwrap();
    let beam = unavailable.iter().find(|c| c.elem == ElemId(1)).unwrap();
    let reason = beam.unchecked_reason.as_deref().unwrap();
    assert!(
        reason.contains("SR235") && reason.contains("fy"),
        "{reason}"
    );
    assert!(beam.qsu.is_nan() && beam.mu.is_finite() && column.qsu.is_finite());

    let checks =
        collect_rc_ultimate_checks(&model_with_shear("SR235", Some(235.0)), &demand, &opts)
            .unwrap();
    let beam = checks.iter().find(|c| c.elem == ElemId(1)).unwrap();
    assert!(beam.qsu.is_finite());
}

#[test]
fn test_ultimate_unknown_sd295_is_unsupported() {
    let opts = UltimateShearOptions::default();
    for name in ["SD295X", "SD295Z", "SD295-FOO"] {
        let mut model = column_and_beam_model();
        model.materials.push(Material {
            id: MaterialId(model.materials.len() as u32),
            name: name.to_string(),
            fy: Some(295.0),
            ..material()
        });
        let shear_id = MaterialId(model.materials.len() as u32 - 1);
        model.sections[1].shear_rebar_material = Some(shear_id);
        let checks = collect_rc_ultimate_checks(
            &model,
            &[
                (
                    ElemId(0),
                    MemberDemand {
                        q_long: Some(0.0),
                        ..MemberDemand::axial(0.0)
                    },
                ),
                (
                    ElemId(1),
                    MemberDemand {
                        mz: 1.0e6,
                        q_long: Some(0.0),
                        ..MemberDemand::axial(0.0)
                    },
                ),
            ],
            &opts,
        )
        .unwrap();
        let beam = checks.iter().find(|c| c.elem == ElemId(1)).unwrap();
        assert!(beam.qsu.is_nan());
        let err = beam.unchecked_reason.as_deref().unwrap();
        assert!(err.contains(name), "{name}: {err}");
        assert!(err.contains("未対応"), "{name}: {err}");
    }
}

/// 算定オプション（軽量コンクリートの低減・付着検定の省略）が機能する。
#[test]
fn test_ultimate_check_skips_non_rc() {
    let mut model = column_and_beam_model();
    model.sections[0].shape = None;
    let checks = collect_rc_ultimate_checks(
        &model,
        &[(
            ElemId(1),
            MemberDemand {
                mz: 1.0e6,
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &UltimateShearOptions::default(),
    )
    .unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].elem, ElemId(1));
}

#[test]
fn test_biaxial_margin_handcalc() {
    // rx=ry=0.5, α=2 → 1/√(0.25+0.25)=1/√0.5=√2。
    let m = biaxial_margin(0.5, 0.5, 2.0);
    assert!((m - 2.0_f64.sqrt()).abs() < 1e-9, "m={m}");
    assert!((biaxial_margin(0.5, 0.0, 2.0) - 2.0).abs() < 1e-9);
    assert!(biaxial_margin(0.0, 0.0, 2.0).is_infinite());
    assert!((biaxial_margin(0.6, 0.8, 2.0) - 1.0).abs() < 1e-9);
}

/// 柱の 2 軸せん断余裕度オプションが機能し、強軸単独より小さい（不利側）になる。
/// 柱の 2 軸曲げ余裕度オプションが機能する（設計用曲げ需要を与えたとき Some・正）。
#[test]
fn test_ultimate_check_biaxial_bending() {
    let model = column_and_beam_model();
    let demand = vec![
        (
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                n_axial: 1_500_000.0,
                mz: 2.0e8,
                my: 1.0e8,
                ..Default::default()
            },
        ),
        (
            ElemId(1),
            MemberDemand {
                mz: 1.0e6,
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        ),
    ];
    let uni =
        collect_rc_ultimate_checks(&model, &demand, &UltimateShearOptions::default()).unwrap();
    let bi = collect_rc_ultimate_checks(
        &model,
        &demand,
        &UltimateShearOptions {
            biaxial_bending: true,
            ..Default::default()
        },
    )
    .unwrap();
    let col_uni = uni.iter().find(|c| c.elem == ElemId(0)).unwrap();
    let col_bi = bi.iter().find(|c| c.elem == ElemId(0)).unwrap();
    assert!(col_uni.biaxial_bending_margin.is_none());
    let bm = col_bi.biaxial_bending_margin.expect("2軸曲げ指定で Some");
    assert!(bm > 0.0 && bm.is_finite(), "bm={bm}");
    // 手計算照合: 1/√((Mmx/Mux)²+(Mmy/Muy)²)。Mux=col.mu(強軸)、Muy は弱軸 Mu。
    // 強軸 Mux は col_bi.mu と一致（同一軸力）。弱軸は main_y=main_x なので b↔D 入替のみ。
    // rx=Mmx/Mux>0, ry>0 → bm < min(Mux/Mmx, Muy/Mmy)。
    let rx = 2.0e8 / col_bi.mu;
    assert!(rx > 0.0);
    let zero_demand = vec![
        (
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(1_500_000.0)
            },
        ),
        (
            ElemId(1),
            MemberDemand {
                mz: 1.0e6,
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        ),
    ];
    let z = collect_rc_ultimate_checks(
        &model,
        &zero_demand,
        &UltimateShearOptions {
            biaxial_bending: true,
            ..Default::default()
        },
    )
    .unwrap();
    let col_z = z.iter().find(|c| c.elem == ElemId(0)).unwrap();
    assert!(col_z.biaxial_bending_margin.unwrap().is_infinite());
    let beam_bi = bi.iter().find(|c| c.elem == ElemId(1)).unwrap();
    assert!(beam_bi.biaxial_bending_margin.is_none());
}

/// プッシュオーバー応答からの部材別 Rp・設計用せん断力（強軸・2 軸せん断の
/// 弱軸）の直接反映が機能する。
pub(super) fn cft_test_direct_lengths() -> Vec<sepika_core::model::SteelDesignAttr> {
    vec![sepika_core::model::SteelDesignAttr {
        elem: ElemId(0),
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        column_lateral_buckling_end_supports: None,
        lk_y_direct: Some(3000.0),
        lk_z_direct: Some(3000.0),
        c_direct: None,
    }]
}

/// CFT 角形柱 1 本のモデルで軸終局検定ドライバが Ncu/Ntu・軸余裕度を算定する。
#[test]
fn test_collect_cft_ultimate_checks() {
    let cft_shape = SectionShape::CftBox {
        corner_r: Some(0.0),
        height: 400.0,
        width: 400.0,
        thick: 12.0,
        horizontal_thick: 12.0,
    };
    let sec = Section {
        frame_use: Some(sepika_core::model::FrameSectionUse::Column),
        id: SectionId(0),
        name: "CFT400".into(),
        area: cft_shape.calc_area(),
        iy: cft_shape.calc_iy(),
        iz: cft_shape.calc_iz(),
        j: 1.0,
        depth: 400.0,
        width: 400.0,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: Some(cft_shape),
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: Some(MaterialId(1)),
        property_basis: Default::default(),
    };
    let mat = Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: "BCR295".to_string(),
        category: MaterialCategory::Concrete,
        young: 205000.0,
        poisson: 0.3,
        density: 2.4e-9,
        shear: None,
        fc: Some(30.0),
        fy: None,
    };
    let steel = Material {
        id: MaterialId(1),
        name: "BCR295".to_string(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    };
    let model = Model {
        load_cases: sepika_core::model::default_load_cases(),
        combinations: sepika_core::model::default_combinations(),
        ultimate_gravity_combination: Some(0),
        nodes: vec![node(0, [0.0, 0.0, 0.0]), node(1, [0.0, 0.0, 3000.0])],
        sections: vec![sec],
        materials: vec![mat, steel],
        elements: vec![frame_element(0, 0, 0, 1)],
        steel_design_attrs: cft_test_direct_lengths(),
        ..Default::default()
    };
    let axial = vec![(ElemId(0), 3_000_000.0)];
    let checks = collect_cft_ultimate_checks(&model, &axial).unwrap();
    assert_eq!(checks.len(), 1);
    let c = &checks[0];
    assert!(c.ncu > 0.0 && c.ntu > 0.0);
    assert!((c.axial_margin - c.ncu / 3_000_000.0).abs() < 1e-6);
    // lk=3000, D=400 → lk/D=7.5 → 中柱。
    assert_eq!(c.class, CftColumnClass::Medium);
    assert!(
        c.mu_nm.as_ref().is_ok_and(|mu| *mu > 0.0),
        "mu_nm={:?}",
        c.mu_nm
    );
    let mut rounded = model.clone();
    rounded.sections[0] = rounded.sections[0].with_surface_radius(Some(30.0)).unwrap();
    let checks = collect_cft_ultimate_checks(&rounded, &axial).unwrap();
    assert_eq!(checks.len(), 1);
    assert!(checks[0].ncu > 0.0 && checks[0].ntu > 0.0);
    assert_ne!(checks[0].ncu, c.ncu);
    assert!(checks[0].mu_nm.as_ref().unwrap() > &0.0);
    assert!(!checks[0].detail.contains("未算定"));
}

/// 実配筋モデルの断面から 1 部材のモデルを作る（部材軸は `horizontal` で切替）。
fn single_shape_model(shape: SectionShape, b: f64, d: f64, horizontal: bool) -> Model {
    let sec = Section {
        frame_use: Some(sepika_core::model::FrameSectionUse::Column),
        id: SectionId(0),
        name: "NEW0".to_string(),
        area: shape.calc_area(),
        iy: shape.calc_iy(),
        iz: shape.calc_iz(),
        j: 1.0,
        depth: d,
        width: b,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: Some(shape),
        material: Some(MaterialId(0)),
        rebar_material: Some(MaterialId(1)),
        shear_rebar_material: Some(MaterialId(1)),
        steel_material: None,
        property_basis: Default::default(),
    };
    let nodes = if horizontal {
        vec![node(0, [0.0, 0.0, 0.0]), node(1, [6000.0, 0.0, 0.0])]
    } else {
        vec![node(0, [0.0, 0.0, 0.0]), node(1, [0.0, 0.0, 3000.0])]
    };
    Model {
        load_cases: sepika_core::model::default_load_cases(),
        combinations: sepika_core::model::default_combinations(),
        ultimate_gravity_combination: Some(0),
        nodes,
        sections: vec![sec],
        materials: vec![concrete_material(), material()],
        elements: vec![frame_element(0, 0, 0, 1)],
        ..Default::default()
    }
}

/// 上 4+2 本・下 3+2 本の非対称 RC 梁断面。
fn beam_rect_shape() -> SectionShape {
    SectionShape::RcBeamRect {
        b: 400.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            top: vec![4, 2],
            bottom: vec![3, 2],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs: 2,
            },
        },
    }
}

/// x=[3], y=[3] の RC 矩形柱断面。
fn column_rect_shape() -> SectionShape {
    SectionShape::RcColumnRect {
        b: 600.0,
        d: 700.0,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 3,
            },
        },
    }
}

/// count=8 の RC 円形柱断面。
fn column_circle_shape() -> SectionShape {
    SectionShape::RcColumnCircle {
        d: 600.0,
        rebar: RcCircleColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            count: 8,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
            },
        },
    }
}

#[test]
fn static_beam_with_unresolved_bending_direction_still_rejects_invalid_reinforcement() {
    for (top, bottom) in [(vec![0], vec![2]), (vec![], vec![])] {
        let mut shape = beam_rect_shape();
        let SectionShape::RcBeamRect { rebar, .. } = &mut shape else {
            unreachable!();
        };
        rebar.top = top;
        rebar.bottom = bottom;
        let model = single_shape_model(shape, 400.0, 600.0, true);
        for (mz, static_beam_bending) in [
            (-20e6, StaticBeamBending::Mixed),
            (20e6, StaticBeamBending::Unverified),
            (0.0, StaticBeamBending::Signed),
        ] {
            let demand = MemberDemand {
                mz,
                static_beam_bending,
                q_long: Some(0.0),
                ..Default::default()
            };
            let reason = collect_rc_ultimate_checks(
                &model,
                &[(ElemId(0), demand)],
                &UltimateShearOptions::default(),
            )
            .unwrap_err();
            assert!(reason.contains("部材 ID 0 の配筋形状が不正"), "{reason}");
        }
    }
}

/// 実配筋モデルの新型バリアント（梁・矩形柱・円形柱）が終局検定の対象になる。
#[test]
fn test_ultimate_check_new_rc_shapes_supported() {
    let opts = UltimateShearOptions::default();

    let beam = single_shape_model(beam_rect_shape(), 400.0, 600.0, true);
    let checks = collect_rc_ultimate_checks(
        &beam,
        &[(
            ElemId(0),
            MemberDemand {
                min_axial_for_shear_n: Some(0.0),
                ..MemberDemand::from_pushover(0.0, -1.0e6, 0.0, 30_000.0, 0.0, 0.0)
            },
        )],
        &opts,
    )
    .unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].kind, MemberKind::Girder);
    assert!(checks[0].mu > 0.0 && checks[0].qsu > 0.0 && checks[0].qmu > 0.0);
    assert!(checks[0].axial.is_none());

    let column = single_shape_model(column_rect_shape(), 600.0, 700.0, false);
    let checks = collect_rc_ultimate_checks(
        &column,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &opts,
    )
    .unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].kind, MemberKind::Column);
    assert!(checks[0].mu > 0.0 && checks[0].qsu.is_nan() && checks[0].qmu > 0.0);
    assert!(checks[0].axial.is_some());

    let circle = single_shape_model(column_circle_shape(), 600.0, 600.0, false);
    let checks = collect_rc_ultimate_checks(
        &circle,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &opts,
    )
    .unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].kind, MemberKind::Column);
    assert!(checks[0].mu > 0.0 && checks[0].qsu.is_nan() && checks[0].qmu > 0.0);
    assert!(checks[0].axial.is_some());
}

/// 実配筋が幾何的に成立しない新モデル断面は、終局検定の入力不備として
/// 部材 ID 付きで停止する（無言スキップしない）。
#[test]
fn test_ultimate_inconsistent_rebar_errors() {
    let opts = UltimateShearOptions::default();
    let shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 700.0,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![4, 2],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 3,
            },
        },
    };
    let model = single_shape_model(shape, 600.0, 700.0, false);
    let err = collect_rc_ultimate_checks(
        &model,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &opts,
    )
    .unwrap_err();
    assert!(err.contains("部材 ID 0"), "{err}");
    assert!(err.contains("配筋形状"), "{err}");
}

/// 矩形柱・円形柱の Mu が軸力 0 の手計算（0.8·at·σy·D）に一致する。
#[test]
fn test_ultimate_new_columns_mu_matches_handcalc() {
    let opts = UltimateShearOptions::default();
    let a22 = std::f64::consts::PI * (22.0_f64 / 2.0).powi(2);

    let column = single_shape_model(column_rect_shape(), 600.0, 700.0, false);
    let checks = collect_rc_ultimate_checks(
        &column,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &opts,
    )
    .unwrap();
    let at = 3.0 * a22;
    let expected = 0.8 * at * 345.0 * 700.0;
    assert!(
        (checks[0].mu - expected).abs() / expected < 1e-9,
        "矩形柱 mu={} expected={expected}",
        checks[0].mu
    );

    let circle = single_shape_model(column_circle_shape(), 600.0, 600.0, false);
    let checks = collect_rc_ultimate_checks(
        &circle,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(0.0),
                ..MemberDemand::axial(0.0)
            },
        )],
        &opts,
    )
    .unwrap();
    let side = (std::f64::consts::PI * 600.0 * 600.0 / 4.0).sqrt();
    let at = 2.0 * a22;
    let expected = 0.8 * at * 345.0 * side;
    assert!(
        (checks[0].mu - expected).abs() / expected < 1e-9,
        "円形柱 mu={} expected={expected}",
        checks[0].mu
    );
}

/// 梁の上下非対称では曲げモーメントの符号で引張側が変わり Mu に反映される。
#[test]
fn test_ultimate_beam_rect_tension_side_affects_mu() {
    let opts = UltimateShearOptions::default();
    let model = single_shape_model(beam_rect_shape(), 400.0, 600.0, true);

    let top_demand = vec![(
        ElemId(0),
        MemberDemand {
            q_long: Some(0.0),
            mz: -1.0e8,
            ..Default::default()
        },
    )];
    let bottom_demand = vec![(
        ElemId(0),
        MemberDemand {
            q_long: Some(0.0),
            mz: 1.0e8,
            ..Default::default()
        },
    )];
    let top = collect_rc_ultimate_checks(&model, &top_demand, &opts).unwrap();
    let bottom = collect_rc_ultimate_checks(&model, &bottom_demand, &opts).unwrap();
    let mu_top = top[0].mu;
    let mu_bottom = bottom[0].mu;
    assert!(mu_top > 0.0 && mu_bottom > 0.0);
    assert!(
        (mu_top - mu_bottom).abs() > 1.0,
        "上端引張 {mu_top} と下端引張 {mu_bottom} は異なるはず"
    );
    assert!(mu_top > mu_bottom, "mu_top={mu_top} mu_bottom={mu_bottom}");
}

#[test]
fn rectangular_beam_arakawa_mean_uses_analysis_shear_span_and_withholds_unverified_response() {
    let mut shape = beam_rect_shape();
    let SectionShape::RcBeamRect { rebar, .. } = &mut shape else {
        unreachable!()
    };
    rebar.top = vec![4];
    rebar.bottom = vec![4];
    let model = single_shape_model(shape, 400.0, 600.0, true);
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let check = |demand| {
        collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts)
            .unwrap()
            .remove(0)
    };
    let demand = MemberDemand::from_pushover(0.0, 107_800_000.0, 0.0, 100_000.0, 0.0, 0.0);
    let result = check(MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..demand
    });
    // 式(付1.3-7)に d=539 mm と M/(Qd)=2 を代入。
    assert!((result.qsu - 421174.748973273).abs() < 1e-5, "{result:?}");
    assert!(result.ok && result.qmu == 100_000.0, "{result:?}");
    assert!(check(demand).qsu.is_nan());
    for axial in [Some(-1.0), Some(1.0)] {
        let result = check(MemberDemand {
            min_axial_for_shear_n: axial,
            ..demand
        });
        assert!(
            result.qsu.is_nan() && result.unchecked_reason.is_some(),
            "{result:?}"
        );
    }
    let result = check(MemberDemand {
        q_long: Some(0.0),
        mz: demand.mz,
        ..Default::default()
    });
    assert!(
        result.qsu.is_nan() && result.shear_margin.is_nan(),
        "{result:?}"
    );
}

#[test]
fn arakawa_mean_distinguishes_invalid_pitch_from_absent_reinforcement() {
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let demand = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 107_800_000.0, 0.0, 100_000.0, 0.0, 0.0)
    };
    for (base, width, depth, beam) in [
        (beam_rect_shape(), 400.0, 600.0, true),
        (column_rect_shape(), 600.0, 700.0, false),
    ] {
        for (pitch, legs) in [
            (100.0, 2),
            (0.0, 2),
            (-100.0, 2),
            (f64::NAN, 2),
            (f64::INFINITY, 2),
            (f64::NEG_INFINITY, 2),
            (f64::NAN, 0),
        ] {
            let mut shape = base.clone();
            match &mut shape {
                SectionShape::RcBeamRect { rebar, .. } => {
                    rebar.stirrup.pitch = pitch;
                    rebar.stirrup.legs = legs;
                }
                SectionShape::RcColumnRect { rebar, .. } => {
                    rebar.hoop.pitch = pitch;
                    rebar.hoop.legs_x = legs;
                    rebar.hoop.legs_y = legs;
                }
                _ => unreachable!(),
            }
            let model = single_shape_model(shape, width, depth, beam);
            let result = collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts)
                .unwrap()
                .remove(0);
            if pitch == 100.0 {
                assert!(
                    result.qsu.is_finite() && result.qsu > 0.0 && result.ok,
                    "{result:?}"
                );
            } else {
                assert!(
                    result.qsu.is_nan() && result.shear_margin.is_nan() && !result.ok,
                    "{result:?}"
                );
                let reason = result.unchecked_reason.as_deref().unwrap();
                assert!(
                    if legs == 0 {
                        reason.contains("せん断補強筋を確認できません")
                    } else {
                        reason.contains("間隔")
                    },
                    "{reason}"
                );
            }
        }
    }
}

#[test]
fn independent_axes_distinguish_zero_shear_from_unresolved_shear_span() {
    let model = single_shape_model(column_rect_shape(), 600.0, 700.0, false);
    let opts = UltimateShearOptions {
        include_bond: false,
        biaxial_shear: true,
        ..Default::default()
    };
    let check_demand = |demand| {
        collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts)
            .unwrap()
            .remove(0)
    };
    let check = |moment_nmm| {
        let demand = MemberDemand {
            min_axial_for_shear_n: Some(0.0),
            ..MemberDemand::from_pushover(0.0, moment_nmm, moment_nmm, 0.0, 0.0, 0.0)
        };
        check_demand(demand)
    };
    let pure_bending = check(10_000_000.0);
    assert!(pure_bending.qsu.is_finite() && pure_bending.qsu > 0.0);
    assert_eq!(pure_bending.weak_shear_margin, Some(f64::INFINITY));
    assert!(!pure_bending.ok && pure_bending.detail.contains("二軸同時作用"));

    let zero_response = check(0.0);
    assert!(zero_response.qsu.is_nan() && zero_response.shear_margin.is_nan());
    assert_eq!(zero_response.weak_shear_margin, None);
    assert!(
        !zero_response.ok
            && zero_response
                .unchecked_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("MとQがともにゼロ")),
        "{zero_response:?}"
    );
    let strong_axis_ng = check_demand(MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 100e6, 25e6, 5_000_000.0, 10_000.0, 0.0)
    });
    assert!(
        !strong_axis_ng.ok
            && strong_axis_ng.unchecked_reason.is_none()
            && strong_axis_ng.shear_margin < 1.0
            && strong_axis_ng.weak_shear_margin.unwrap() > 1.0
            && strong_axis_ng.detail.contains("二軸同時作用"),
        "{strong_axis_ng:?}"
    );
}

#[test]
fn rectangular_column_arakawa_mean_uses_the_verified_minimum_compression() {
    let model = single_shape_model(column_rect_shape(), 600.0, 700.0, false);
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let mut demand =
        MemberDemand::from_pushover(1_000_000.0, 127_800_000.0, 0.0, 100_000.0, 0.0, 0.0);
    let check = |demand| {
        collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts)
            .unwrap()
            .remove(0)
    };
    let unverified = check(demand);
    assert!(unverified.qsu.is_nan() && unverified.shear_margin.is_nan());
    assert!(unverified
        .unchecked_reason
        .unwrap()
        .contains("断面力の範囲"));
    demand.min_axial_for_shear_n = Some(840_000.0);
    let result = check(demand);
    // 式(付1.3-16)に d=639 mm、M/(Qd)=2 と圧縮応力度 2 N/mm² を代入。
    assert!((result.qsu - 680049.569603855).abs() < 1e-5, "{result:?}");
    assert!(result.ok, "{result:?}");
    demand.min_axial_for_shear_n = Some(-1.0);
    let result = check(demand);
    assert!(
        result.qsu.is_nan() && result.unchecked_reason.is_some(),
        "{result:?}"
    );
}

#[test]
fn bond_stress_can_fail_independently_of_unavailable_shear() {
    use super::{
        RcUltimateBondAxis, RcUltimateBondBar, RcUltimateBondConfinement, RcUltimateBondDemand,
        RcUltimateBondInput, RcUltimateBondPlacement, RcUltimateThroughBarYield,
    };

    let multiple_layers = beam_rect_shape();
    let mut single_layer = multiple_layers.clone();
    let SectionShape::RcBeamRect { rebar, .. } = &mut single_layer else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D22);
    rebar.top = vec![4];
    rebar.bottom = vec![4];
    let bond = RcUltimateBondInput {
        bar: RcUltimateBondBar::Through {
            clear_span_mm: 6000.0,
            cutoff_unnecessary_distance_mm: None,
            yielding: RcUltimateThroughBarYield::BothEnds,
        },
        axis: RcUltimateBondAxis::Strong,
        bar_designation_mm: 22.0,
        effective_depth_mm: 539.0,
        design_yield_stress_mpa: Some(345.0),
        placement: RcUltimateBondPlacement::TopCast,
        confinement: Some(RcUltimateBondConfinement {
            clear_spacing_mm: 13.0,
            minimum_cover_mm: 40.0,
            splitting_plane_bar_count: 4,
            transverse_rebar: None,
        }),
    };
    let demand = MemberDemand {
        q_long: Some(0.0),
        mz: 10e6,
        bond: Some(RcUltimateBondDemand::Single(bond)),
        ..Default::default()
    };
    let check = |shape, demand| {
        let model = single_shape_model(shape, 400.0, 600.0, true);
        collect_rc_ultimate_checks(
            &model,
            &[(ElemId(0), demand)],
            &UltimateShearOptions::default(),
        )
        .unwrap()
        .remove(0)
    };
    let result = check(single_layer.clone(), demand);
    let stress = result.bond_stress.unwrap();
    assert!(
        (stress.demand_mpa - 0.6949276689251053).abs() < 1e-12,
        "{result:?}"
    );
    assert!(
        result.bond_margin < 1.0 && result.qsu.is_nan(),
        "{result:?}"
    );
    assert!(
        !result.ok && result.unchecked_reason.is_none(),
        "{result:?}"
    );
    let mismatched = check(
        single_layer,
        MemberDemand {
            bond: Some(RcUltimateBondDemand::Single(RcUltimateBondInput {
                effective_depth_mm: 500.0,
                ..bond
            })),
            ..demand
        },
    );
    assert!(mismatched.bond_stress.is_none());
    assert!(mismatched
        .unchecked_reason
        .unwrap()
        .contains("有効せいが検討方向"));
    let multilayer = check(multiple_layers, demand);
    assert!(multilayer.bond_stress.is_none());
    assert!(multilayer.unchecked_reason.unwrap().contains("一段配筋"));

    let mut column_shape = column_rect_shape();
    let SectionShape::RcColumnRect { rebar, .. } = &mut column_shape else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D22);
    let column = single_shape_model(column_shape, 600.0, 700.0, false);
    let check_column = |bond| {
        collect_rc_ultimate_checks(
            &column,
            &[(
                ElemId(0),
                MemberDemand {
                    bond: Some(RcUltimateBondDemand::Single(RcUltimateBondInput {
                        bar: RcUltimateBondBar::Through {
                            clear_span_mm: 3000.0,
                            cutoff_unnecessary_distance_mm: None,
                            yielding: RcUltimateThroughBarYield::OneEndWithOtherElastic,
                        },
                        placement: RcUltimateBondPlacement::Other,
                        ..bond
                    })),
                    ..demand
                },
            )],
            &UltimateShearOptions::default(),
        )
        .unwrap()
        .remove(0)
    };
    for (axis, depth, expected) in [
        (RcUltimateBondAxis::Strong, 639.0, 0.803685),
        (RcUltimateBondAxis::Weak, 539.0, 0.771028),
    ] {
        let entry = RcUltimateBondInput {
            axis,
            effective_depth_mm: depth,
            ..bond
        };
        let result = check_column(entry);
        assert!(
            (result.bond_stress.unwrap().demand_mpa - expected).abs() < 1e-6,
            "{result:?}"
        );
        let mismatched = check_column(RcUltimateBondInput {
            effective_depth_mm: depth - 50.0,
            ..entry
        });
        assert!(mismatched.bond_stress.is_none());
        assert!(mismatched
            .unchecked_reason
            .unwrap()
            .contains("有効せいが検討方向"));
    }
}

#[test]
fn ultimate_bond_uses_yield_stress_and_keeps_an_independent_cutoff_length_failure() {
    use super::{
        RcUltimateBeamBondDemand, RcUltimateBeamBondDirectionDemand, RcUltimateBeamBondSection,
        RcUltimateBeamBondTensionSide, RcUltimateBondAxis, RcUltimateBondBar,
        RcUltimateBondConfinement, RcUltimateBondDemand, RcUltimateBondElasticStress,
        RcUltimateBondInput, RcUltimateBondMomentDistribution, RcUltimateBondPlacement,
        RcUltimateCutoffBarStress, RcUltimateThroughBarYield,
    };

    let mut shape = beam_rect_shape();
    let SectionShape::RcBeamRect { rebar, .. } = &mut shape else {
        unreachable!()
    };
    rebar.top = vec![4];
    rebar.bottom = vec![4];
    rebar.cover = 79.0;
    rebar.main_dia = RebarSize::Standard(StandardRebar::D22);
    let model = single_shape_model(shape, 400.0, 600.0, true);
    let bond_input = |bar, placement, design_yield_stress_mpa| RcUltimateBondInput {
        bar,
        axis: RcUltimateBondAxis::Strong,
        bar_designation_mm: 22.0,
        effective_depth_mm: 500.0,
        design_yield_stress_mpa,
        placement,
        confinement: Some(RcUltimateBondConfinement {
            clear_spacing_mm: 2.0,
            minimum_cover_mm: 79.0,
            splitting_plane_bar_count: 4,
            transverse_rebar: None,
        }),
    };
    let with_bond = |demand, bond: RcUltimateBondDemand| {
        let (bars, _) = bond.inputs();
        let elastic = bars
            .into_iter()
            .flatten()
            .find_map(|input| input.bar.elastic_stress());
        if let Some(stress) = elastic {
            let direction = |distance_from_i_face_mm| RcUltimateBeamBondDirectionDemand {
                section: RcUltimateBeamBondSection {
                    distance_from_i_face_mm,
                    long_term_mz_nmm: if stress.long_term_tensile_stress_mpa != 0.0 {
                        1e6
                    } else {
                        0.0
                    },
                    lateral_mz_nmm: if stress.lateral_tensile_stress_mpa != 0.0 {
                        1e6
                    } else {
                        0.0
                    },
                    tension_side: RcUltimateBeamBondTensionSide::Bottom,
                },
                represents_all_bond_checks: true,
                bars: bond,
            };
            MemberDemand {
                beam_bond: Some(RcUltimateBeamBondDemand {
                    seismic_axis: sepika_solver::statics::analysis::SeismicDir::X,
                    positive: Some(direction(0.0)),
                    negative: Some(direction(6000.0)),
                }),
                ..demand
            }
        } else {
            MemberDemand {
                bond: Some(bond),
                ..demand
            }
        }
    };
    let run_bond_with_strength =
        |model: &Model, bar, placement, demand, design_yield_stress_mpa| {
            let demand = with_bond(
                demand,
                RcUltimateBondDemand::Single(bond_input(bar, placement, design_yield_stress_mpa)),
            );
            collect_rc_ultimate_checks(
                model,
                &[(ElemId(0), demand)],
                &UltimateShearOptions::default(),
            )
        };
    let run_bond = |model: &Model, bar, placement, demand| {
        run_bond_with_strength(model, bar, placement, demand, Some(345.0))
    };
    let run_with_placement = |model: &Model, bar, bending, mz, placement| {
        run_bond(
            model,
            bar,
            placement,
            MemberDemand {
                q_long: Some(0.0),
                mz,
                static_beam_bending: bending,
                ..Default::default()
            },
        )
    };
    let run = |model: &Model, bar, bending| {
        run_with_placement(model, bar, bending, 10e6, RcUltimateBondPlacement::TopCast)
    };
    let check = |bar, bending| run(&model, bar, bending).unwrap().remove(0);
    let through = |yielding| RcUltimateBondBar::Through {
        clear_span_mm: 6000.0,
        cutoff_unnecessary_distance_mm: None,
        yielding,
    };
    let both = check(
        through(RcUltimateThroughBarYield::BothEnds),
        StaticBeamBending::Signed,
    );
    assert!((both.bond_stress.unwrap().demand_mpa - 0.69).abs() < 1e-12);
    assert!(
        both.bond_margin < 1.0 && !both.ok && both.unchecked_reason.is_none(),
        "{both:?}"
    );
    let demand = MemberDemand {
        q_long: Some(0.0),
        mz: 10e6,
        ..Default::default()
    };
    let pair_demand = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 10e6, 0.0, 1000.0, 0.0, 0.0)
    };
    let check_pair_with = |bond, include_bond| {
        collect_rc_ultimate_checks(
            &model,
            &[(ElemId(0), with_bond(pair_demand, bond))],
            &UltimateShearOptions {
                include_bond,
                ..Default::default()
            },
        )
        .unwrap()
        .remove(0)
    };
    let check_pair = |bond| check_pair_with(bond, true);
    for (distribution, l_prime, expected_stress) in [
        (
            RcUltimateBondMomentDistribution::NearAntisymmetric,
            1000.0,
            11.0 / 18.0,
        ),
        (
            RcUltimateBondMomentDistribution::NearPinnedEnd,
            2000.0,
            11.0 / 28.0,
        ),
    ] {
        let bar = RcUltimateBondBar::Through {
            clear_span_mm: 6000.0,
            cutoff_unnecessary_distance_mm: Some(l_prime),
            yielding: RcUltimateThroughBarYield::WithoutFlexuralYield {
                moment_distribution: distribution,
                stress: RcUltimateBondElasticStress {
                    long_term_tensile_stress_mpa: 100.0,
                    lateral_tensile_stress_mpa: 100.0,
                    lateral_factor: 1.5,
                },
            },
        };
        let through = bond_input(bar, RcUltimateBondPlacement::TopCast, Some(345.0));
        let cutoff = |development_length_mm, unnecessary_distance_mm, stress| {
            bond_input(
                RcUltimateBondBar::Cutoff {
                    development_length_mm,
                    clear_span_mm: 6000.0,
                    unnecessary_distance_mm,
                    stress,
                },
                RcUltimateBondPlacement::TopCast,
                Some(345.0),
            )
        };
        let combined =
            |cutoff| check_pair(RcUltimateBondDemand::ThroughAndCutoff { through, cutoff });
        let low_cutoff_stress =
            RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: 85.0,
                lateral_tensile_stress_mpa: 10.0,
                lateral_factor: 1.5,
            });
        let result = combined(cutoff(3500.0, l_prime, low_cutoff_stress));
        assert!((result.bond_stress.unwrap().demand_mpa - expected_stress).abs() < 1e-12);
        assert_eq!(result.bond_development_length_ok, Some(true));
        assert!(
            !result.ok && result.qsu.is_nan() && result.shear_margin.is_nan(),
            "{result:?}"
        );
        assert_eq!(
            result.unchecked_reason.is_some(),
            expected_stress < 0.5,
            "{result:?}"
        );
        assert!(result.detail.contains("一様な配筋を確認できません"));
        for bond in [
            RcUltimateBondDemand::Single(through),
            RcUltimateBondDemand::Single(cutoff(3500.0, l_prime, low_cutoff_stress)),
            RcUltimateBondDemand::ThroughAndCutoff {
                through,
                cutoff: cutoff(3500.0, l_prime, low_cutoff_stress),
            },
        ] {
            let result = check_pair_with(bond, false);
            assert!(
                !result.ok && result.qsu.is_nan() && result.shear_margin.is_nan(),
                "{result:?}"
            );
            assert!(result
                .unchecked_reason
                .unwrap()
                .contains("一様な配筋を確認できません"));
        }
        let cutoff_governs = combined(cutoff(
            3000.0,
            l_prime,
            RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: 285.0,
                lateral_tensile_stress_mpa: 10.0,
                lateral_factor: 1.5,
            }),
        ));
        assert!((cutoff_governs.bond_stress.unwrap().demand_mpa - 0.66).abs() < 1e-12);
        assert!(!cutoff_governs.ok && cutoff_governs.unchecked_reason.is_none());
        let missing = check_pair(RcUltimateBondDemand::Single(through));
        assert!(!missing.ok && missing.bond_development_length_ok.is_none());
        assert!(missing.detail.contains("併用するカットオフ筋の照査入力"));
        let mismatched = combined(cutoff(3500.0, l_prime + 100.0, low_cutoff_stress));
        assert!(!mismatched.ok && mismatched.bond_stress.is_none());
        assert!(mismatched
            .unchecked_reason
            .unwrap()
            .contains("距離が一致しません"));
        let conflicting = combined(cutoff(
            5000.0,
            l_prime,
            RcUltimateCutoffBarStress::FlexuralYield,
        ));
        assert!(!conflicting.ok && conflicting.bond_stress.is_none());
        assert!(conflicting
            .unchecked_reason
            .unwrap()
            .contains("曲げ降伏条件が一致しません"));
        let unknown = combined(cutoff(
            3500.0,
            l_prime,
            RcUltimateCutoffBarStress::Unverified,
        ));
        assert!(!unknown.ok && unknown.detail.contains("カットオフ筋の曲げ降伏状態"));
        assert_eq!(unknown.bond_development_length_ok, Some(true));
        if expected_stress < 0.5 {
            assert!(missing.unchecked_reason.is_some());
            assert!(unknown.unchecked_reason.is_some());
        } else {
            assert!(unknown.bond_margin < 1.0 && unknown.unchecked_reason.is_none());
        }
        let short = combined(cutoff(400.0, l_prime, low_cutoff_stress));
        assert_eq!(short.bond_development_length_ok, Some(false));
        assert!(!short.ok && short.unchecked_reason.is_none());
        let conflicting_and_short = combined(cutoff(
            400.0,
            l_prime,
            RcUltimateCutoffBarStress::FlexuralYield,
        ));
        assert!(conflicting_and_short.bond_stress.is_none());
        assert_eq!(
            conflicting_and_short.bond_development_length_ok,
            Some(false)
        );
        assert!(!conflicting_and_short.ok && conflicting_and_short.unchecked_reason.is_none());
        assert!(conflicting_and_short
            .detail
            .contains("曲げ降伏条件が一致しません"));
    }
    let measured_yield = run_bond_with_strength(
        &model,
        through(RcUltimateThroughBarYield::BothEnds),
        RcUltimateBondPlacement::TopCast,
        demand,
        Some(379.5),
    )
    .unwrap()
    .remove(0);
    assert!((measured_yield.bond_stress.unwrap().demand_mpa - 0.759).abs() < 1e-12);
    let unknown = check(
        through(RcUltimateThroughBarYield::Unverified),
        StaticBeamBending::Signed,
    );
    assert!(unknown.bond_stress.is_none() && unknown.bond_development_length_ok.is_none());
    assert!(unknown
        .unchecked_reason
        .unwrap()
        .contains("通し筋の曲げ降伏状態"));

    let cutoff = |development_length_mm, stress| RcUltimateBondBar::Cutoff {
        development_length_mm,
        clear_span_mm: 6000.0,
        unnecessary_distance_mm: 200.0,
        stress,
    };
    let low_stress = RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
        long_term_tensile_stress_mpa: 5.0,
        lateral_tensile_stress_mpa: 5.0,
        lateral_factor: 1.5,
    });
    for (design_strength, expected) in [
        (None, "実降伏強度または強度上昇"),
        (Some(344.0), "規格降伏強度を下回っています"),
    ] {
        let unverified = run_bond_with_strength(
            &model,
            cutoff(1000.0, low_stress),
            RcUltimateBondPlacement::TopCast,
            demand,
            design_strength,
        )
        .unwrap()
        .remove(0);
        assert!(
            unverified.bond_stress.is_none() && unverified.bond_development_length_ok == Some(true)
        );
        assert!(unverified.unchecked_reason.unwrap().contains(expected));
    }
    let check_cast = |model: &Model, placement| {
        run_with_placement(
            model,
            cutoff(1000.0, low_stress),
            StaticBeamBending::Signed,
            0.0,
            placement,
        )
        .unwrap()
        .remove(0)
    };
    let other = check_cast(&model, RcUltimateBondPlacement::Other);
    assert!((other.bond_stress.unwrap().capacity_mpa - 0.6409090909090909).abs() < 1e-12);
    assert!(other.bond_margin > 1.0 && !other.ok);
    assert_eq!(other.bond_development_length_ok, Some(true));
    assert!(other
        .unchecked_reason
        .as_deref()
        .unwrap()
        .contains("長期強軸曲げが0"));
    assert!(other.detail.contains("カットオフ筋の必要延長を満たします"));
    let top = check_cast(&model, RcUltimateBondPlacement::TopCast);
    assert!((top.bond_stress.unwrap().capacity_mpa - 0.5127272727272727).abs() < 1e-12);
    let mut lightweight = model.clone();
    lightweight.materials[0].concrete_class = sepika_core::units::ConcreteClass::Lightweight1;
    let light = check_cast(&lightweight, RcUltimateBondPlacement::TopCast);
    assert!((light.bond_stress.unwrap().capacity_mpa - 0.4101818181818182).abs() < 1e-12);
    let unverified_cast = check_cast(&model, RcUltimateBondPlacement::Unverified);
    assert!(unverified_cast.bond_stress.is_none());
    assert_eq!(unverified_cast.bond_development_length_ok, Some(true));
    assert!(unverified_cast
        .unchecked_reason
        .unwrap()
        .contains("上端筋区分"));
    let mut unknown_concrete_class = model.clone();
    unknown_concrete_class.materials[0].concrete_class =
        sepika_core::units::ConcreteClass::UserDefined;
    let unverified_concrete = check_cast(&unknown_concrete_class, RcUltimateBondPlacement::Other);
    assert!(unverified_concrete.bond_stress.is_none());
    assert_eq!(unverified_concrete.bond_development_length_ok, Some(true));
    assert!(unverified_concrete
        .unchecked_reason
        .unwrap()
        .contains("コンクリート区分"));
    for (class, upper_fc) in [
        (sepika_core::units::ConcreteClass::Normal, 60.0_f64),
        (sepika_core::units::ConcreteClass::Lightweight1, 36.0),
        (sepika_core::units::ConcreteClass::Lightweight2, 27.0),
    ] {
        let mut concrete = model.clone();
        concrete.materials[0].concrete_class = class;
        for fc in [18.0, upper_fc] {
            concrete.materials[0].fc = Some(fc);
            let within = check_cast(&concrete, RcUltimateBondPlacement::Other);
            assert!(
                within.bond_stress.is_some(),
                "{class:?} Fc={fc}: {within:?}"
            );
        }
        for fc in [18.0_f64.next_down(), upper_fc.next_up()] {
            concrete.materials[0].fc = Some(fc);
            let outside = check_cast(&concrete, RcUltimateBondPlacement::Other);
            assert!(
                outside.bond_stress.is_none() && outside.bond_development_length_ok == Some(true)
            );
            assert!(outside.unchecked_reason.unwrap().contains("Fcの適用範囲外"));
        }
    }
    let mut numeric_rebar = model.clone();
    let Some(SectionShape::RcBeamRect { rebar, .. }) = numeric_rebar.sections[0].shape.as_mut()
    else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::DiameterMm(22.0);
    let numeric = check_cast(&numeric_rebar, RcUltimateBondPlacement::Other);
    assert!(numeric.bond_stress.is_none() && numeric.bond_development_length_ok == Some(true));
    assert!(numeric.unchecked_reason.unwrap().contains("標準呼び名"));
    for (grade, fy) in [("USD685", 685.0), ("SD345", 346.0)] {
        let mut unverified_rebar = model.clone();
        unverified_rebar.materials[1].name = grade.into();
        unverified_rebar.materials[1].fy = Some(fy);
        let unsupported = check_cast(&unverified_rebar, RcUltimateBondPlacement::Other);
        assert!(unsupported.bond_stress.is_none());
        assert_eq!(unsupported.bond_development_length_ok, Some(true));
        assert!(unsupported
            .unchecked_reason
            .unwrap()
            .contains("D41以下のJIS異形鉄筋"));
    }
    let total = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 1e6, 0.0, 1000.0, 0.0, 0.0)
    };
    let mut nonconcrete = model.clone();
    nonconcrete.materials[0].category = MaterialCategory::Steel;
    let nonconcrete = run_bond(
        &nonconcrete,
        cutoff(400.0, low_stress),
        RcUltimateBondPlacement::Other,
        total,
    )
    .unwrap()
    .remove(0);
    assert!(
        nonconcrete.mu.is_nan() && nonconcrete.qsu.is_nan() && nonconcrete.bond_stress.is_none()
    );
    assert_eq!(nonconcrete.bond_development_length_ok, Some(false));
    assert!(!nonconcrete.ok && nonconcrete.unchecked_reason.is_none());
    assert!(nonconcrete
        .detail
        .contains("断面主材料に「コンクリート」区分"));
    let yielded = check(
        cutoff(1000.0, RcUltimateCutoffBarStress::FlexuralYield),
        StaticBeamBending::Signed,
    );
    assert!((yielded.bond_stress.unwrap().demand_mpa - 3.795).abs() < 1e-12);
    let elastic = |lateral_factor| {
        RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
            long_term_tensile_stress_mpa: 50.0,
            lateral_tensile_stress_mpa: 100.0,
            lateral_factor,
        })
    };
    let below_yield = check(cutoff(1000.0, elastic(1.5)), StaticBeamBending::Signed);
    assert!((below_yield.bond_stress.unwrap().demand_mpa - 2.2).abs() < 1e-12);
    let capped = check(cutoff(1000.0, elastic(3.0)), StaticBeamBending::Signed);
    assert!((capped.bond_stress.unwrap().demand_mpa - 3.795).abs() < 1e-12);
    let zero = check(
        cutoff(
            1000.0,
            RcUltimateCutoffBarStress::WithoutFlexuralYield(RcUltimateBondElasticStress {
                long_term_tensile_stress_mpa: 0.0,
                lateral_tensile_stress_mpa: 0.0,
                lateral_factor: 1.5,
            }),
        ),
        StaticBeamBending::Signed,
    );
    assert!(zero.bond_stress.is_none());
    assert!(zero.bond_margin.is_nan());
    assert_eq!(zero.bond_development_length_ok, Some(true));
    let unverified = check(cutoff(1000.0, elastic(1.49)), StaticBeamBending::Signed);
    assert!(
        unverified.bond_stress.is_none() && unverified.bond_development_length_ok == Some(true)
    );
    assert!(unverified
        .unchecked_reason
        .unwrap()
        .contains("鉄筋応力度または割増"));

    for (length, stress, bending) in [
        (
            400.0,
            RcUltimateCutoffBarStress::FlexuralYield,
            StaticBeamBending::Signed,
        ),
        (
            500.0,
            RcUltimateCutoffBarStress::Unverified,
            StaticBeamBending::Signed,
        ),
        (
            600.0,
            RcUltimateCutoffBarStress::Unverified,
            StaticBeamBending::Unverified,
        ),
    ] {
        let short = check(cutoff(length, stress), bending);
        assert!(
            short.bond_stress.is_none() && short.bond_margin.is_nan(),
            "{short:?}"
        );
        assert_eq!(short.bond_development_length_ok, Some(false), "{short:?}");
        assert!(!short.ok && short.unchecked_reason.is_none(), "{short:?}");
        assert!(
            short
                .detail
                .contains("カットオフ筋の付着長さが必要な延長を満たしません")
                && short.detail.contains("τD=未算定"),
            "{short:?}"
        );
    }

    let mut rebar_without_yield = model.clone();
    rebar_without_yield.materials.push(Material {
        id: MaterialId(2),
        fy: None,
        ..model.materials[1].clone()
    });
    rebar_without_yield.sections[0].rebar_material = Some(MaterialId(2));
    let mut unsupported_stirrup = model.clone();
    unsupported_stirrup.materials.push(Material {
        id: MaterialId(2),
        name: "USD685".into(),
        fy: Some(685.0),
        ..model.materials[1].clone()
    });
    unsupported_stirrup.sections[0].shear_rebar_material = Some(MaterialId(2));
    let mut stirrup_without_yield = unsupported_stirrup.clone();
    stirrup_without_yield.materials[2].name = "SD345".into();
    stirrup_without_yield.materials[2].fy = None;
    let mut mismatched_stirrup = unsupported_stirrup.clone();
    mismatched_stirrup.materials[2].name = "SR235".into();
    mismatched_stirrup.materials[2].fy = Some(235.0);
    let Some(SectionShape::RcBeamRect { rebar, .. }) =
        mismatched_stirrup.sections[0].shape.as_mut()
    else {
        unreachable!()
    };
    rebar.stirrup.dia = RebarSize::Standard(StandardRebar::D10);
    let incompatible = run_bond(
        &mismatched_stirrup,
        cutoff(1000.0, low_stress),
        RcUltimateBondPlacement::Other,
        total,
    )
    .unwrap()
    .remove(0);
    assert!(incompatible.qsu.is_nan() && incompatible.bond_stress.is_some());
    assert!(incompatible.bond_margin > 1.0 && !incompatible.ok);
    assert!(incompatible
        .unchecked_reason
        .unwrap()
        .contains("標準異形鉄筋 D10 に丸鋼材種 SR235"));

    for (invalid_model, diagnostic, hard_error) in [
        (&rebar_without_yield, "主筋の降伏強度を解決できません", true),
        (&unsupported_stirrup, "せん断補強筋グレード", false),
        (
            &stirrup_without_yield,
            "せん断補強筋材料「SD345」に降伏強度",
            false,
        ),
    ] {
        let short = run(
            invalid_model,
            cutoff(400.0, RcUltimateCutoffBarStress::FlexuralYield),
            StaticBeamBending::Signed,
        )
        .unwrap()
        .remove(0);
        assert_eq!(short.bond_development_length_ok, Some(false), "{short:?}");
        assert!(
            short.bond_stress.is_none() && short.bond_margin.is_nan(),
            "{short:?}"
        );
        assert!(!short.ok && short.unchecked_reason.is_none(), "{short:?}");
        assert!(
            short.detail.contains(diagnostic)
                && short.detail.contains("τD=未算定")
                && short
                    .detail
                    .contains("カットオフ筋の付着長さが必要な延長を満たしません"),
            "{short:?}"
        );

        let unverified = run(
            invalid_model,
            through(RcUltimateThroughBarYield::Unverified),
            StaticBeamBending::Signed,
        );
        if hard_error {
            let error = unverified.unwrap_err();
            assert!(error.contains(diagnostic), "{error}");
        } else {
            let result = unverified.unwrap().remove(0);
            assert!(result.bond_stress.is_none() && result.mu.is_finite() && !result.ok);
            assert!(result
                .unchecked_reason
                .as_deref()
                .unwrap()
                .contains(diagnostic));
        }
    }

    for (invalid_model, diagnostic) in [
        (&unsupported_stirrup, "せん断補強筋グレード"),
        (
            &stirrup_without_yield,
            "せん断補強筋材料「SD345」に降伏強度",
        ),
    ] {
        let stress_failure = run(
            invalid_model,
            cutoff(1000.0, RcUltimateCutoffBarStress::FlexuralYield),
            StaticBeamBending::Signed,
        )
        .unwrap()
        .remove(0);
        assert_eq!(stress_failure.bond_development_length_ok, Some(true));
        assert!((stress_failure.bond_stress.unwrap().demand_mpa - 3.795).abs() < 1e-12);
        assert!(stress_failure.bond_margin < 1.0);
        assert!(!stress_failure.ok && stress_failure.unchecked_reason.is_none());
        assert!(
            stress_failure.detail.contains(diagnostic),
            "{stress_failure:?}"
        );
        let bond_pass = run(
            invalid_model,
            cutoff(1000.0, low_stress),
            StaticBeamBending::Signed,
        )
        .unwrap()
        .remove(0);
        assert!(bond_pass.bond_stress.is_some() && bond_pass.bond_margin > 1.0);
        assert!(
            !bond_pass.ok
                && bond_pass
                    .unchecked_reason
                    .as_deref()
                    .unwrap()
                    .contains(diagnostic)
        );
    }
    for fc in [None, Some(f64::NAN)] {
        let mut invalid_concrete = model.clone();
        invalid_concrete.materials[0].fc = fc;
        let short = run(
            &invalid_concrete,
            cutoff(400.0, RcUltimateCutoffBarStress::FlexuralYield),
            StaticBeamBending::Signed,
        )
        .unwrap()
        .remove(0);
        assert_eq!(short.bond_development_length_ok, Some(false));
        assert!(short.bond_stress.is_none() && !short.ok && short.unchecked_reason.is_none());
        assert!(short.detail.contains("Fc") && short.detail.contains("必要な延長を満たしません"));
    }
}
