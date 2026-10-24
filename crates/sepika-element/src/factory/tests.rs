use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::AnalysisKind;
use sepika_core::model::{
    EndCondition, FrameSectionUse, LocalAxis, Material, MaterialCategory, Node, Section,
};

fn make_diaphragm_model() -> Model {
    let mut model = Model {
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
                coord: [5000.0, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        constraints: vec![sepika_core::model::Constraint::rigid_diaphragm(
            sepika_core::ids::StoryId(0),
            NodeId(2),
            vec![NodeId(1)],
        )],
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "sec".into(),
            area: 100.0,
            iy: 833.33,
            iz: 833.33,
            j: 100.0,
            depth: 10.0,
            width: 10.0,
            as_y: 83.33,
            as_z: 83.33,
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
            young: 20000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(1e20),
        }],
        ..Default::default()
    };
    let mut column_section = model.sections[0].clone();
    column_section.id = SectionId(1);
    column_section.frame_use = Some(FrameSectionUse::Column);
    model.sections.push(column_section);
    model
}

fn elastic_wall_girder_model() -> Model {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    let mut model = make_diaphragm_model();
    let mut node = model.nodes[2].clone();
    node.id = NodeId(3);
    node.coord = [5000.0, 0.0, 3000.0];
    model.nodes.push(node);
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].fc = Some(24.0);
    model.materials[0].fy = None;
    model.materials[0].density = 2.4e-9;
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 200.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "G1".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    model.sections[0] = section;
    let mut wall_section = SectionShape::RcWall {
        thickness: 150.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(2), "W1".into());
    wall_section.material = Some(MaterialId(0));
    model.sections.push(wall_section);
    let girder = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let mut top = girder.clone();
    top.id = ElemId(1);
    top.nodes = smallvec::smallvec![NodeId(2), NodeId(3)];
    let mut wall = girder.clone();
    wall.id = ElemId(2);
    wall.kind = ElementKind::Wall;
    wall.nodes = smallvec::smallvec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)];
    wall.section = Some(SectionId(2));
    model.elements = vec![girder, top, wall];
    model
}

#[test]
fn wall_girder_role_keeps_elastic_stiffness_force_mass_and_hinge_contract() {
    use crate::behavior::{Ctx, LocalVec, MassOption};
    use crate::frame::beam::{is_elastic_wall_girder, BeamElement};
    let mut model = elastic_wall_girder_model();
    let mut bare_model = model.clone();
    bare_model.elements.retain(|e| e.kind != ElementKind::Wall);
    let bare = BeamElement::new(&model.elements[0], &bare_model);
    for element_kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for regime in [
            ForceRegime::Auto,
            ForceRegime::UniaxialBendingShear,
            ForceRegime::AxialBendingInteract,
        ] {
            model.elements[0].kind = element_kind;
            model.elements[0].force_regime = regime;
            model.set_member_rc_beam_reference(
                ElemId(0),
                Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
            );
            let data = &model.elements[0];
            assert!(is_elastic_wall_girder(&model, data));
            assert!(input_check::member_strength_issue(data, &model).is_none());
            assert!(rc_beam_reference_input_issue(
                data,
                &model,
                AnalysisKind::Incremental,
                StrengthBasis::Nominal
            )
            .is_none());
            assert!(!nonlinear_input_issues(&model)
                .iter()
                .any(|issue| issue.contains("部材 ID 0")));
            let beam = BeamElement::new(data, &model);
            assert_eq!(beam.mass_properties, bare.mass_properties);
            let baseline_mass = [
                beam.mass_matrix(MassOption::Lumped),
                beam.mass_matrix(MassOption::Consistent),
            ];
            assert_eq!(beam.a, 100.0 * bare.a);
            assert_eq!(beam.iy, 100.0 * bare.iy);
            assert_eq!(beam.iz, 100.0 * bare.iz);
            for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
                let ctx = Ctx { model: &model };
                let mut behavior = build_nonlinear_behavior(
                    data,
                    &model,
                    StrengthBasis::MaterialStrength,
                    kind,
                    None,
                );
                assert_eq!(
                    behavior.tangent_stiffness(&ctx).data,
                    beam.tangent_stiffness(&ctx).data
                );
                for (option, expected) in [MassOption::Lumped, MassOption::Consistent]
                    .into_iter()
                    .zip(&baseline_mass)
                {
                    assert_eq!(behavior.mass_matrix(option).data, expected.data);
                }
                let mut increment = LocalVec {
                    data: smallvec::smallvec![0.0; 12],
                };
                increment.data[6] = 50.0;
                behavior.update_state(&increment, true, &ctx).unwrap();
                let expected_axial_force = 100.0 * 20000.0 * (300.0 * 600.0) / 5000.0 * 50.0;
                assert!(
                    (behavior.internal_force(&ctx).data[6] - expected_axial_force).abs() < 1e-6
                );
                assert_eq!(
                    behavior.tangent_stiffness(&ctx).data,
                    beam.tangent_stiffness(&ctx).data
                );
                assert!(behavior.fiber_section_states().is_none());
                assert!(behavior.ductility_probe().is_none());
                assert!(behavior.end_spring_rotations().is_none());
                let view = build_hinge_view(
                    data,
                    &model,
                    StrengthBasis::MaterialStrength,
                    kind,
                    0.0,
                    8,
                    24,
                )
                .unwrap();
                assert_eq!(view.model, AnalysisHingeModel::Other);
                assert!(view
                    .unavailability_reason
                    .unwrap()
                    .contains("材料降伏は評価しません"));
                assert!(
                    view.backbone.is_none()
                        && view.mn_surface.is_none()
                        && view.mn_linear.is_none()
                );
                assert!(!resolves_to_concentrated_spring(data, &model));
            }
        }
    }
}

#[test]
fn wall_girder_diagnostic_keeps_common_geometry_and_mass_requirements() {
    let mut model = elastic_wall_girder_model();
    assert!(input_check::member_strength_issue(&model.elements[0], &model).is_none());
    model.materials[0].fc = None;
    assert!(
        input_check::member_strength_issue(&model.elements[0], &model)
            .unwrap()
            .contains("Fc")
    );
    model.materials[0].fc = Some(24.0);
    if let Some(sepika_core::section_shape::SectionShape::RcBeamRect { rebar, .. }) =
        &mut model.sections[0].shape
    {
        rebar.cover = 400.0;
    }
    assert!(input_check::member_strength_issue(&model.elements[0], &model).is_some());
    let mut model = elastic_wall_girder_model();
    model.elements.retain(|e| e.kind != ElementKind::Wall);
    assert!(
        input_check::member_strength_issue(&model.elements[0], &model)
            .unwrap()
            .contains("主筋")
    );
}

fn assert_wall_girder_factory_rejection(model: &Model) -> String {
    let data = &model.elements[0];
    assert!(crate::frame::beam::is_elastic_wall_girder(model, data));
    let expected = input_check::member_strength_issue(data, model).expect("共通入力不備を診断する");
    assert!(expected.contains("部材 ID 0"));
    assert!(nonlinear_input_issues(model).contains(&expected));
    for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
        let panic = std::panic::catch_unwind(|| {
            build_nonlinear_behavior(data, model, StrengthBasis::Nominal, kind, None);
        })
        .expect_err("直接生成でも同じ入力不備を拒否する");
        let actual = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert_eq!(actual, expected);
    }
    expected
}

#[test]
fn elastic_wall_girder_requires_positive_finite_actual_young_for_every_section() {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
    let column_shape = SectionShape::RcColumnRect {
        b: 400.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    for shape in [
        None,
        Some(SectionShape::SteelFlatBar {
            width: 300.0,
            thick: 20.0,
        }),
        Some(column_shape),
    ] {
        let mut model = elastic_wall_girder_model();
        if let Some(shape) = shape {
            model.sections[0] = shape.to_section(SectionId(0), "G1".into());
        } else {
            model.sections[0].shape = None;
            model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
        }
        let mut material = model.materials[0].clone();
        material.id = MaterialId(1);
        if !model.sections[0]
            .shape
            .as_ref()
            .is_some_and(SectionShape::is_concrete_like)
        {
            material.category = MaterialCategory::Steel;
            material.fc = None;
        }
        model.materials.push(material);
        model.sections[0].material = Some(MaterialId(1));
        model.sections[0].frame_use = Some(FrameSectionUse::Girder);
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            model.elements[0].kind = kind;
            model.materials[1].young = 20000.0;
            assert!(
                input_check::member_strength_issue(&model.elements[0], &model).is_none(),
                "{:?}",
                input_check::member_strength_issue(&model.elements[0], &model)
            );
            build_nonlinear_behavior(
                &model.elements[0],
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::Incremental,
                None,
            );
            for young in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                model.materials[1].young = young;
                assert!(assert_wall_girder_factory_rejection(&model).contains("ヤング係数 E"));
            }
        }
    }
}

#[test]
fn elastic_wall_girders_reject_invalid_adopted_shear_modulus() {
    use crate::frame::beam::BeamElement;
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = elastic_wall_girder_model();
        for data in &mut model.elements[..2] {
            data.kind = kind;
        }
        for (shear, poisson) in [
            (Some(f64::NAN), 0.3),
            (Some(f64::INFINITY), 0.3),
            (Some(-1.0), 0.3),
            (Some(0.0), 0.3),
            (None, -1.0),
            (None, -2.0),
        ] {
            model.materials[0].shear = shear;
            model.materials[0].poisson = poisson;
            for data in &model.elements[..2] {
                let expected = match BeamElement::try_new(data, &model) {
                    Ok(_) => panic!("壁上下梁G={shear:?}/nu={poisson}が受理された"),
                    Err(error) => error,
                };
                assert!(expected.contains(&format!("部材 ID {}", data.id.0)));
                assert_eq!(
                    input_check::member_strength_issue(data, &model),
                    Some(expected.clone())
                );
                assert!(nonlinear_input_issues(&model).contains(&expected));
            }
            assert_wall_girder_factory_rejection(&model);
        }
        model.materials[0].shear = Some(80000.0);
        for data in &model.elements[..2] {
            assert!(input_check::member_strength_issue(data, &model).is_none());
            assert!(BeamElement::try_new(data, &model).is_ok());
            build_nonlinear_behavior(
                data,
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::Incremental,
                None,
            );
        }
    }
}

#[test]
fn elastic_wall_girder_factory_rejects_common_geometry_category_and_mass_errors() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for invalid in 0..3 {
            let mut model = elastic_wall_girder_model();
            model.elements[0].kind = kind;
            match invalid {
                0 => {
                    if let Some(sepika_core::section_shape::SectionShape::RcBeamRect {
                        rebar,
                        ..
                    }) = &mut model.sections[0].shape
                    {
                        rebar.cover = 400.0;
                    }
                }
                1 => model.materials[0].category = MaterialCategory::Rebar,
                _ => model.materials[0].density = f64::NAN,
            }
            assert_wall_girder_factory_rejection(&model);
        }
    }
}

/// フォースレジームの解決: 明示指定はそのまま、Auto はトポロジ
/// （剛床所属 × 鉛直材か）から判定する。
#[test]
fn test_resolve_force_regime_explicit_and_auto() {
    let model = make_diaphragm_model();
    let make = |id: u32, nodes: [NodeId; 2], force_regime: ForceRegime| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![nodes[0], nodes[1]],
        section: Some(if nodes[0] == NodeId(0) && nodes[1] == NodeId(2) {
            SectionId(1)
        } else {
            SectionId(0)
        }),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };

    let beam_explicit = make(0, [NodeId(0), NodeId(1)], ForceRegime::UniaxialBendingShear);
    assert!(matches!(
        resolve_force_regime(&beam_explicit, &model),
        ResolvedRegime::ConcentratedSpring
    ));
    let col_explicit = make(1, [NodeId(0), NodeId(2)], ForceRegime::AxialBendingInteract);
    assert!(matches!(
        resolve_force_regime(&col_explicit, &model),
        ResolvedRegime::Fiber
    ));
    // 柱トポロジ（Auto なら Fiber）でも明示指定が優先されること
    let col_explicit_spring = make(2, [NodeId(0), NodeId(2)], ForceRegime::UniaxialBendingShear);
    assert!(matches!(
        resolve_force_regime(&col_explicit_spring, &model),
        ResolvedRegime::ConcentratedSpring
    ));

    let beam_auto = make(0, [NodeId(0), NodeId(1)], ForceRegime::Auto);
    assert!(matches!(
        resolve_force_regime(&beam_auto, &model),
        ResolvedRegime::ConcentratedSpring
    ));
    let col_auto = make(1, [NodeId(0), NodeId(2)], ForceRegime::Auto);
    assert!(matches!(
        resolve_force_regime(&col_auto, &model),
        ResolvedRegime::Fiber
    ));
}

/// 線形弾性解析の要素生成は `ForceRegime` に依らず弾性 `BeamElement`。
///
/// 剛床に載る梁は `resolve_force_regime` では `ConcentratedSpring` に判定されるが、
/// これは非線形解析だけの振り分けである。線形側がこれに従うと
/// (1) 材端ばねが直列に入って弾性剛性が落ち、(2) 材端集中ばね梁は
/// `recover_forces` を持たないため部材内力が丸ごと欠落する。
#[test]
fn test_build_behavior_linear_is_always_elastic_beam() {
    let model = make_diaphragm_model();
    let make = |nodes: [NodeId; 2]| ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![nodes[0], nodes[1]],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let beam = make([NodeId(0), NodeId(1)]);
    let col = make([NodeId(0), NodeId(2)]);

    let ctx = crate::behavior::Ctx { model: &model };
    for (label, data) in [("集中ばね判定", &beam), ("ファイバー判定", &col)] {
        let behavior = build_behavior(data, &model);
        assert!(
            behavior.recover_forces(&[0.0; 12]).is_some(),
            "{label}: 線形解析の梁は内力を回収できる弾性 BeamElement であること"
        );
        let elastic = crate::frame::beam::BeamElement::new(data, &model);
        let k_ref = elastic.axis.to_global(&elastic.local_stiffness());
        let k = behavior.tangent_stiffness(&ctx);
        for i in 0..12 {
            for j in 0..12 {
                assert!(
                    (k.get(i, j) - k_ref.get(i, j)).abs() <= k_ref.get(i, j).abs() * 1e-12 + 1e-9,
                    "{label}: K[{i}][{j}] が弾性梁と一致しない: {} vs {}",
                    k.get(i, j),
                    k_ref.get(i, j)
                );
            }
        }
    }
}

#[test]
fn test_build_nonlinear_behavior_concentrated_spring_uses_spring_beam() {
    let model = make_diaphragm_model();
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let behavior = build_nonlinear_behavior(
        &beam,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        None,
    );
    let snap = behavior.snapshot_state();
    let is_spring = snap
        .downcast_ref::<(
            Vec<Box<dyn sepika_material::uniaxial::UniaxialMaterial>>,
            [f64; 4],
            [f64; 4],
            [f64; 12],
            [f64; 12],
        )>()
        .is_some();
    assert!(
        is_spring,
        "nonlinear ConcentratedSpring should be ConcentratedSpringBeam"
    );
}

#[test]
fn test_build_nonlinear_behavior_fiber_uses_fiber_beam() {
    let model = make_diaphragm_model();
    let col = ElementData {
        id: ElemId(1),
        kind: ElementKind::Fiber,
        nodes: smallvec::smallvec![NodeId(0), NodeId(2)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let behavior = build_nonlinear_behavior(
        &col,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        None,
    );
    let snap = behavior.snapshot_state();
    let is_fiber = snap
        .downcast_ref::<crate::frame::fiber::FiberBeamSnapshot>()
        .is_some();
    assert!(is_fiber, "nonlinear Fiber should be FiberBeam");
}

/// ブレース要素の生成モデル用（2 節点・軸方向 4000mm・断面積 2000mm2）。
fn make_brace_model(tension_only: bool) -> (Model, ElementData) {
    let model = Model {
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
                coord: [4000.0, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "brace".into(),
            area: 2000.0,
            iy: 0.0,
            iz: 0.0,
            j: 0.0,
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
            name: "steel".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
        }],
        ..Default::default()
    };
    let elem = ElementData {
        id: ElemId(0),
        kind: ElementKind::Brace { tension_only },
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Pinned, EndCondition::Pinned],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    (model, elem)
}

/// ブレース: 線形・非線形のどちらの生成でも要素側の特別扱いはせず、全剛性
/// E·A/L の TrussElement を生成する。引張専用の圧縮側無効化は線形応力解析の
/// active-set 反復で扱う。
#[test]
fn test_build_behavior_brace_uses_full_truss_stiffness() {
    let ea_l = 205000.0 * 2000.0 / 4000.0;
    for tension_only in [false, true] {
        let (model, elem) = make_brace_model(tension_only);
        let ctx = crate::behavior::Ctx { model: &model };
        let k = build_behavior(&elem, &model).tangent_stiffness(&ctx);
        assert!(
            (k.get(0, 0) - ea_l).abs() < 1e-6,
            "線形 tension_only={tension_only}: k00={}",
            k.get(0, 0)
        );
    }

    let (model, elem) = make_brace_model(true);
    let ctx = crate::behavior::Ctx { model: &model };
    let k = build_nonlinear_behavior(
        &elem,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        None,
    )
    .tangent_stiffness(&ctx);
    assert!(
        (k.get(0, 0) - ea_l).abs() < 1e-6,
        "非線形 tension_only: k00={}",
        k.get(0, 0)
    );
}

/// 壁要素の開口低減: wall_attrs の開口面積からせん断剛性が低減されること
/// （RC規準（耐震壁）の開口低減 r=1−1.25·√(開口面積/壁面積)）。
#[test]
fn test_build_behavior_wall_opening_reduces_shear_stiffness() {
    use sepika_core::model::WallAttr;

    let make_node = |id: u32, coord: [f64; 3]| Node {
        id: NodeId(id),
        coord,
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    };
    let mut model = Model {
        nodes: vec![
            make_node(0, [0.0, 0.0, 0.0]),
            make_node(1, [4000.0, 0.0, 0.0]),
            make_node(2, [4000.0, 0.0, 3000.0]),
            make_node(3, [0.0, 0.0, 3000.0]),
        ],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "wall".into(),
            area: 150.0 * 1000.0,
            iy: 1.0e9,
            iz: 1.0e9,
            j: 1.0e9,
            depth: 1000.0,
            width: 150.0,
            as_y: 125_000.0,
            as_z: 125_000.0,
            floor: None,
            panel_thickness: None,
            thickness: Some(150.0),
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
            name: "FC24".into(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 0.0,
            shear: None,
            fc: Some(24.0),
            fy: None,
        }],
        ..Default::default()
    };
    let wall = ElementData {
        id: ElemId(0),
        kind: ElementKind::Wall,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };

    let shear_pattern = |k: &crate::behavior::LocalMat| -> f64 {
        let mut u = [0.0; 24];
        u[2 * 6] = 1.0;
        u[3 * 6] = 1.0;
        let mut s = 0.0;
        for i in 0..24 {
            for j in 0..24 {
                s += u[i] * k.get(i, j) * u[j];
            }
        }
        s
    };
    let axial_pattern = |k: &crate::behavior::LocalMat| -> f64 {
        let mut u = [0.0; 24];
        u[2 * 6 + 2] = 1.0;
        u[3 * 6 + 2] = 1.0;
        let mut s = 0.0;
        for i in 0..24 {
            for j in 0..24 {
                s += u[i] * k.get(i, j) * u[j];
            }
        }
        s
    };

    model.elements.push(wall.clone());
    let b_no = build_behavior(&wall, &model);
    let ctx = crate::behavior::Ctx { model: &model };
    let k_no = b_no.tangent_stiffness(&ctx);

    model.wall_attrs.push(WallAttr {
        elem: ElemId(0),
        opening_area: 1.2e6,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: vec![],
        finish_intensity: 0.0,
    });
    let b_open = build_behavior(&wall, &model);
    let ctx2 = crate::behavior::Ctx { model: &model };
    let k_open = b_open.tangent_stiffness(&ctx2);

    model.wall_attrs[0] = WallAttr {
        elem: ElemId(0),
        opening_area: 1.0,
        opening_weight: 0.0,
        slit: Default::default(),
        finish_intensity: 0.0,
        openings: vec![
            sepika_core::model::WallOpening {
                width: 1000.0,
                height: 800.0,
                offset: Some([0.0, 500.0]),
            },
            sepika_core::model::WallOpening {
                width: 500.0,
                height: 800.0,
                offset: Some([1800.0, 500.0]),
            },
        ],
    };
    let b_dims = build_behavior(&wall, &model);
    let ctx3 = crate::behavior::Ctx { model: &model };
    let k_dims = b_dims.tangent_stiffness(&ctx3);
    assert!(
        (shear_pattern(&k_dims) - shear_pattern(&k_open)).abs() < 1e-6,
        "個別開口(Σ1.2e6)と面積のみ(1.2e6)の低減が一致しない: {} vs {}",
        shear_pattern(&k_dims),
        shear_pattern(&k_open)
    );

    model.multi_opening_mode = sepika_core::model::MultiOpeningMode::Envelope;
    let b_env = build_behavior(&wall, &model);
    let ctx4 = crate::behavior::Ctx { model: &model };
    let k_env = b_env.tangent_stiffness(&ctx4);
    assert!(
        shear_pattern(&k_env) < shear_pattern(&k_dims) * 0.999,
        "包絡モードで低減が強まらない: env={} eq={}",
        shear_pattern(&k_env),
        shear_pattern(&k_dims)
    );
    model.multi_opening_mode = sepika_core::model::MultiOpeningMode::Equivalent;

    assert!(
        shear_pattern(&k_open) < shear_pattern(&k_no) * 0.999,
        "shear open={} no={}",
        shear_pattern(&k_open),
        shear_pattern(&k_no)
    );
    assert!((axial_pattern(&k_open) - axial_pattern(&k_no)).abs() < 1e-6);
}

#[test]
fn test_resolve_member_hysteresis_and_flexural_springs() {
    use sepika_core::model::HysteresisModel;
    use sepika_core::section_shape::{RcBeamRebar, SectionShape};

    fn rebar() -> RcBeamRebar {
        use sepika_core::section_shape::BeamStirrup;
        RcBeamRebar {
            main_dia: 22.0,
            top: vec![2],
            bottom: vec![2],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        }
    }

    let mut model = make_diaphragm_model();
    let mut rebar_material = model.materials[0].clone();
    rebar_material.id = MaterialId(1);
    rebar_material.category = MaterialCategory::Rebar;
    rebar_material.young = 205000.0;
    rebar_material.fc = None;
    rebar_material.fy = Some(345.0);
    model.materials.push(rebar_material);
    model.sections[0].rebar_material = Some(MaterialId(1));
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());

    assert!(!is_rc_like_section(&beam, &model));
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental),
        HysteresisModel::Standard
    );
    let (_i, _j, backbone) = build_flexural_springs(
        &beam,
        &model,
        HysteresisModel::Standard,
        StrengthBasis::Nominal,
    );
    assert!(backbone.use_mn);

    model.sections[0].shape = Some(SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: rebar(),
    });
    model.sections[0].depth = 700.0;
    model.sections[0].width = 400.0;
    model.sections[0].iy = 400.0 * 700.0f64.powi(3) / 12.0;
    model.sections[0].iz = 700.0 * 400.0f64.powi(3) / 12.0;
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    model.materials[0].fc = Some(24.0);
    model.materials[0].fy = Some(345.0);
    assert!(is_rc_like_section(&beam, &model));
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental),
        HysteresisModel::Takeda
    );
    let (_i, _j, backbone) = build_flexural_springs(
        &beam,
        &model,
        HysteresisModel::Takeda,
        StrengthBasis::Nominal,
    );
    assert!(
        !backbone.use_mn,
        "武田型(履歴材料)は N-M 相関(set_yield)対象外"
    );

    model.sections[0].shape = Some(SectionShape::SteelH {
        root_r: Some(0.0),
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
    });
    assert!(!is_rc_like_section(&beam, &model));
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental),
        HysteresisModel::Standard
    );

    model.set_member_hysteresis(ElemId(0), HysteresisModel::MaxPointOriented);
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental),
        HysteresisModel::MaxPointOriented
    );
    let (_i, _j, backbone) = build_flexural_springs(
        &beam,
        &model,
        HysteresisModel::MaxPointOriented,
        StrengthBasis::Nominal,
    );
    assert!(!backbone.use_mn);
}

/// 履歴則の 2 スロット（増分用／時刻歴用）の解決を検証する。
/// - 時刻歴用スロット未指定は増分用の指定に従う
/// - 時刻歴用スロット指定は時刻歴解決のみに効く
#[test]
fn test_resolve_member_hysteresis_two_slots() {
    use sepika_core::model::HysteresisModel;

    let mut model = make_diaphragm_model();
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());

    for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
        assert_eq!(
            resolve_member_hysteresis(&beam, &model, kind),
            HysteresisModel::Standard
        );
    }

    model.set_member_hysteresis(ElemId(0), HysteresisModel::OriginOriented);
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::TimeHistory),
        HysteresisModel::OriginOriented
    );

    model.set_member_hysteresis_th(ElemId(0), Some(HysteresisModel::MaxPointOriented));
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental),
        HysteresisModel::OriginOriented
    );
    assert_eq!(
        resolve_member_hysteresis(&beam, &model, AnalysisKind::TimeHistory),
        HysteresisModel::MaxPointOriented
    );
}

/// ファイバー・MS のコンクリート除荷則の解決を検証する。
/// - 既定: 増分=逆行型、時刻歴=Karsan–Jirsa 型（壁は増分=原点指向型）
/// - コンクリート履歴として解釈できない指定（武田型等）は既定へフォールバック
#[test]
fn test_resolve_fiber_concrete_hysteresis_defaults_and_overrides() {
    use sepika_core::model::HysteresisModel;

    let mut model = make_diaphragm_model();
    let col = ElementData {
        id: ElemId(0),
        kind: ElementKind::Fiber,
        nodes: smallvec::smallvec![NodeId(0), NodeId(2)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(col.clone());

    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::Incremental),
        HysteresisModel::Retrograde
    );
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::TimeHistory),
        HysteresisModel::KarsanJirsa
    );
    assert_eq!(
        resolve_wall_concrete_hysteresis(&col, &model, AnalysisKind::Incremental),
        HysteresisModel::OriginOriented
    );
    assert_eq!(
        resolve_wall_concrete_hysteresis(&col, &model, AnalysisKind::TimeHistory),
        HysteresisModel::KarsanJirsa
    );

    model.set_member_hysteresis(ElemId(0), HysteresisModel::Takeda);
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::Incremental),
        HysteresisModel::Retrograde
    );

    model.set_member_hysteresis(ElemId(0), HysteresisModel::OriginOriented);
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::Incremental),
        HysteresisModel::OriginOriented
    );
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::TimeHistory),
        HysteresisModel::OriginOriented
    );

    model.set_member_hysteresis_th(ElemId(0), Some(HysteresisModel::KarsanJirsa));
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::Incremental),
        HysteresisModel::OriginOriented
    );
    assert_eq!(
        resolve_fiber_concrete_hysteresis(&col, &model, AnalysisKind::TimeHistory),
        HysteresisModel::KarsanJirsa
    );
}

/// 耐震壁の面内せん断ばねの履歴則解決を検証する。
/// - 既定（Auto・コンクリート用指定）: 最大点指向型（増分・時刻歴共通）
/// - Q–δ 系の個別指定（標準型・武田型等）は尊重される
#[test]
fn test_resolve_wall_shear_hysteresis_defaults_and_overrides() {
    use sepika_core::model::HysteresisModel;

    let mut model = make_diaphragm_model();
    let wall = ElementData {
        id: ElemId(0),
        kind: ElementKind::Wall,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(wall.clone());

    for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
        assert_eq!(
            resolve_wall_shear_hysteresis(&wall, &model, kind),
            HysteresisModel::MaxPointOriented
        );
    }

    model.set_member_hysteresis(ElemId(0), HysteresisModel::KarsanJirsa);
    assert_eq!(
        resolve_wall_shear_hysteresis(&wall, &model, AnalysisKind::Incremental),
        HysteresisModel::MaxPointOriented
    );
    assert_eq!(
        resolve_wall_concrete_hysteresis(&wall, &model, AnalysisKind::Incremental),
        HysteresisModel::KarsanJirsa
    );

    model.set_member_hysteresis(ElemId(0), HysteresisModel::Standard);
    assert_eq!(
        resolve_wall_shear_hysteresis(&wall, &model, AnalysisKind::Incremental),
        HysteresisModel::Standard
    );
}

/// 材端曲げバネの降伏時剛性低下率 αy。
/// - 形状未設定・非 RC 矩形・鉛直材（柱）は既定 0.3。
/// - RC 矩形の梁は菅野式。b=400・D=700・4-D22（1 段）・かぶり 50・帯筋 D10・
///   可撓長 5000・Ec=20000 のときの独立値 αy≈0.33475
///   （pt=0.002715、a/D=3.571、d/D=629/700、n=10.25）。
#[test]
fn test_flexural_alpha_y_sugano_for_rc_beam() {
    use sepika_core::section_shape::{RcBeamRebar, SectionShape};

    let mut model = make_diaphragm_model();
    let mut rebar_material = model.materials[0].clone();
    rebar_material.id = MaterialId(1);
    rebar_material.category = MaterialCategory::Rebar;
    rebar_material.young = 205000.0;
    rebar_material.fc = None;
    rebar_material.fy = Some(345.0);
    model.materials.push(rebar_material);
    model.sections[0].rebar_material = Some(MaterialId(1));
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());

    assert!((flexural_alpha_y(&beam, &model) - 0.3).abs() < 1e-12);

    let rebar = RcBeamRebar {
        main_dia: 22.0,
        top: vec![2],
        bottom: vec![2],
        cover: 50.0,
        stirrup: sepika_core::section_shape::BeamStirrup {
            dia: 10.0,
            pitch: 100.0,
            legs: 2,
        },
    };
    model.sections[0].shape = Some(SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar,
    });
    let got = flexural_alpha_y(&beam, &model);
    assert!(
        (got - 0.334_749_510_190_037_85).abs() < 1e-12,
        "菅野式の αy: got={got}"
    );
    assert!(got > 0.0 && got < 1.0);
    assert!(
        (got - 0.3).abs() > 1e-3,
        "菅野式の値が既定 0.3 と区別できること（got={got}）"
    );

    let mut column = beam.clone();
    column.nodes = smallvec::smallvec![NodeId(0), NodeId(2)];
    column.section = Some(SectionId(1));
    assert!((flexural_alpha_y(&column, &model) - 0.3).abs() < 1e-12);
}

#[test]
fn test_src_beam_alpha_y_falls_back_to_default() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};

    let mut model = make_diaphragm_model();
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());
    model.sections[0].shape = Some(SectionShape::SrcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![2],
            bottom: vec![2],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 12.0,
    });
    assert!((flexural_alpha_y(&beam, &model) - 0.3).abs() < 1e-12);
}

#[test]
fn test_rc_beam_flexural_spring_exhibits_takeda_degradation() {
    use sepika_core::model::HysteresisModel;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};

    let mut model = make_diaphragm_model();
    let mut rebar_material = model.materials[0].clone();
    rebar_material.id = MaterialId(1);
    rebar_material.category = MaterialCategory::Rebar;
    rebar_material.young = 205000.0;
    rebar_material.fc = None;
    rebar_material.fy = Some(345.0);
    model.materials.push(rebar_material);
    model.sections[0].rebar_material = Some(MaterialId(1));
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());
    model.sections[0].shape = Some(SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![2],
            bottom: vec![2],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    });
    model.sections[0].depth = 700.0;
    model.sections[0].width = 400.0;
    model.sections[0].iy = 400.0 * 700.0f64.powi(3) / 12.0;
    model.sections[0].iz = 700.0 * 400.0f64.powi(3) / 12.0;
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    model.materials[0].young = 25_000.0;
    model.materials[0].fc = Some(24.0);
    model.materials[0].fy = Some(345.0);

    let rule = resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental);
    assert_eq!(rule, HysteresisModel::Takeda);
    let (mut si, _sj, backbone) =
        build_flexural_springs(&beam, &model, rule, StrengthBasis::Nominal);
    assert!(!backbone.use_mn);

    let (_m0, k0) = si.trial(1e-8);
    si.commit();
    assert!(k0 > 0.0);

    let big = 0.02_f64;
    let (m_peak, _) = si.trial(big);
    si.commit();
    assert!(m_peak > 0.0, "should carry positive moment at peak");

    let (m1, _) = si.trial(big * 0.95);
    let (m2, _) = si.trial(big * 0.90);
    let ku = (m1 - m2) / (big * 0.05);
    assert!(
        ku < k0 * 0.999,
        "Takeda unloading stiffness ({ku}) must be below initial ({k0})"
    );
    assert!(ku > 0.0, "unloading stiffness must stay positive");
}

#[test]
fn test_steel_beam_flexural_spring_buckling_degrades() {
    use sepika_core::model::HysteresisModel;
    use sepika_core::section_shape::SectionShape;

    let mut model = make_diaphragm_model();
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.elements.push(beam.clone());
    model.sections[0].shape = Some(SectionShape::SteelH {
        root_r: Some(0.0),
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
    });
    model.sections[0].depth = 400.0;
    model.sections[0].width = 200.0;
    model.sections[0].iz = 200.0 * 400.0f64.powi(3) / 12.0;
    model.materials[0].fy = Some(325.0);
    model.set_member_hysteresis(ElemId(0), HysteresisModel::SteelBuckling);

    let rule = resolve_member_hysteresis(&beam, &model, AnalysisKind::Incremental);
    assert_eq!(rule, HysteresisModel::SteelBuckling);
    let (mut si, _sj, backbone) =
        build_flexural_springs(&beam, &model, rule, StrengthBasis::Nominal);
    assert!(
        backbone.use_mn,
        "座屈考慮型は set_yield 対応で N-M 相関適用可"
    );

    let (_m0, k0) = si.trial(1e-9);
    si.commit();
    assert!(k0 > 0.0);
    let theta_y = { 1e-3 };
    let mut m_max = 0.0_f64;
    let mut m_last = 0.0_f64;
    for i in 1..=200 {
        let th = theta_y * i as f64 * 0.5;
        let (m, _) = si.trial(th);
        si.commit();
        m_max = m_max.max(m);
        m_last = m;
    }
    assert!(m_max > 0.0);
    assert!(
        m_last < m_max * 0.999,
        "buckling degradation: last M ({m_last}) must fall below peak ({m_max})"
    );
}

#[test]
fn axial_factor_preserves_nonaxial_stiffness_and_mass() {
    let mut model = make_diaphragm_model();
    model.materials[0].density = 7.85e-9;
    let ctx = crate::behavior::Ctx { model: &model };
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let data = ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        };
        let normal = build_behavior(&data, &model);
        let reduced = build_behavior_with_axial_factor(&data, &model, 1e-6);
        let kn = normal.tangent_stiffness(&ctx);
        let kr = reduced.tangent_stiffness(&ctx);
        for i in 0..12 {
            for j in 0..12 {
                let expected = kn.get(i, j)
                    * if [0, 6].contains(&i) && [0, 6].contains(&j) {
                        1e-6
                    } else {
                        1.0
                    };
                assert!((kr.get(i, j) - expected).abs() <= expected.abs() * 1e-12);
            }
        }
        for option in [
            crate::behavior::MassOption::Lumped,
            crate::behavior::MassOption::Consistent,
        ] {
            let mn = normal.mass_matrix(option);
            let mr = reduced.mass_matrix(option);
            for i in 0..12 {
                for j in 0..12 {
                    assert_eq!(mn.get(i, j), mr.get(i, j));
                }
            }
        }
    }
}

#[test]
fn zero_increment_concentrated_factory_keeps_equilibrium_tangent_force_and_history() {
    use sepika_material::UniaxialMaterial;
    let mut model = make_diaphragm_model();
    let beam = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    model.set_member_hysteresis(ElemId(0), sepika_core::model::HysteresisModel::Takeda);
    let mut behavior = build_nonlinear_behavior(
        &beam,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        None,
    );
    let ctx = crate::behavior::Ctx { model: &model };
    type Snapshot = (
        Vec<Box<dyn sepika_material::UniaxialMaterial>>,
        [f64; 4],
        [f64; 4],
        [f64; 12],
        [f64; 12],
    );
    let initial = behavior.snapshot_state();
    let initial = initial.downcast_ref::<Snapshot>().unwrap();
    let k0 = 6.0 * 20000.0 * 833.33 / 5000.0;
    for spring in &initial.0 {
        assert!((spring.probe(0.0).1 - k0).abs() < 1e-9);
        let mut material =
            sepika_material::HysteresisMaterial::new(sepika_material::HysteresisRule::Takeda {
                crack: (10.0, 0.01),
                yield_point: (20.0, 0.02),
                ultimate: (30.0, 0.04),
                alpha: 0.0,
            });
        material
            .deserialize_state(&spring.serialize_state())
            .unwrap();
    }
    let du = crate::behavior::LocalVec {
        data: smallvec::smallvec![0.0, 0.0, 0.0, 0.0, 0.0, -0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001],
    };
    behavior.update_state(&du, true, &ctx).unwrap();
    let before = behavior.snapshot_state();
    let state = before.downcast_ref::<Snapshot>().unwrap();
    let k = behavior.tangent_stiffness(&ctx);
    let f = behavior.internal_force(&ctx);
    for i in 0..12 {
        let expected: f64 = (0..12).map(|j| k.get(i, j) * state.3[j]).sum();
        assert!((f.data[i] - expected).abs() < 1e-7);
    }
    for end in 0..2 {
        assert!((state.0[end].probe(state.1[end]).0 - f.data[5 + 6 * end]).abs() < 1e-7);
        assert!((state.1[end] + state.2[end] - state.3[5 + 6 * end]).abs() < 1e-12);
    }
    let zero = crate::behavior::LocalVec {
        data: smallvec::smallvec![0.0;12],
    };
    behavior.update_state(&zero, false, &ctx).unwrap();
    behavior.commit_state();
    let after = behavior.snapshot_state();
    let after = after.downcast_ref::<Snapshot>().unwrap();
    assert_eq!(state.1, after.1);
    assert_eq!(state.2, after.2);
    for end in 0..2 {
        assert_eq!(
            state.0[end].serialize_state(),
            after.0[end].serialize_state()
        );
    }
    assert_eq!(k.data, behavior.tangent_stiffness(&ctx).data);
    assert_eq!(f.data, behavior.internal_force(&ctx).data);
    behavior.update_state(&du, false, &ctx).unwrap();
    behavior.revert_state();
    assert_eq!(f.data, behavior.internal_force(&ctx).data);
}

pub(super) fn wall_side_column_factory_model() -> Model {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
    let mut model = elastic_wall_girder_model();
    let mut rebar = model.materials[0].clone();
    rebar.id = MaterialId(1);
    rebar.category = MaterialCategory::Rebar;
    rebar.name = "SD345".into();
    rebar.young = 200_000.0;
    rebar.fy = Some(345.0);
    rebar.fc = None;
    model.materials.push(rebar);
    let mut section = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    }
    .to_section(SectionId(1), "C1".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections[1] = section;
    let mut column = model.elements[0].clone();
    column.id = ElemId(3);
    column.section = Some(SectionId(1));
    column.nodes = smallvec::smallvec![NodeId(0), NodeId(2)];
    column.local_axis.ref_vector = [1.0, 0.0, 0.0];
    model.elements.push(column.clone());
    column.id = ElemId(4);
    column.nodes = smallvec::smallvec![NodeId(1), NodeId(3)];
    model.elements.push(column);
    model
}

#[test]
fn direct_side_column_factories_reject_conflicting_endpoint_and_section_ownership() {
    let model = wall_side_column_factory_model();
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for conflict in 0..4 {
            let mut data = model.elements[3].clone();
            data.kind = representation;
            match conflict {
                0 => data.nodes = smallvec::smallvec![NodeId(1), NodeId(3)],
                1 => data.section = Some(SectionId(0)),
                2 => data.section = None,
                _ => data.nodes = model.elements[0].nodes.clone(),
            }
            for analysis in [
                None,
                Some(AnalysisKind::Incremental),
                Some(AnalysisKind::TimeHistory),
            ] {
                let reason =
                    wall_side_column_input_issue(&data, &model, analysis).expect("物理所有の矛盾");
                assert!(reason.contains("部材 ID 3") && reason.contains("壁 ID [2]"));
                assert!(reason.contains(if conflict == 0 || conflict == 3 {
                    "端点"
                } else {
                    "断面参照"
                }));
                let panic = std::panic::catch_unwind(|| match analysis {
                    None => build_behavior(&data, &model),
                    Some(kind) => {
                        build_nonlinear_behavior(&data, &model, StrengthBasis::Nominal, kind, None)
                    }
                })
                .err()
                .expect("直接生成も同じ理由で拒否");
                assert_eq!(panic.downcast_ref::<String>(), Some(&reason));
                if let Some(kind) = analysis {
                    let view =
                        build_hinge_view(&data, &model, StrengthBasis::Nominal, kind, 0.0, 4, 8)
                            .unwrap();
                    assert_eq!(view.model, AnalysisHingeModel::Other);
                    assert_eq!(view.unavailability_reason, Some(reason));
                }
            }
        }
    }
}

#[test]
fn steel_plate_boundary_clones_preserve_registered_section_ownership_without_rc_release() {
    use sepika_core::section_shape::SectionShape;
    let mut model = wall_side_column_factory_model();
    model.materials[1].category = MaterialCategory::Steel;
    for (index, usage) in [(0, FrameSectionUse::Girder), (1, FrameSectionUse::Column)] {
        let mut section = SectionShape::SteelBox {
            height: 600.0,
            width: 600.0,
            thick: 12.0,
            corner_r: Some(0.0),
        }
        .to_section(SectionId(index), "S".into());
        section.frame_use = Some(usage);
        section.material = Some(MaterialId(1));
        model.sections[index as usize] = section;
    }
    model.sections[2].shape = None;
    model.sections[2].material = Some(MaterialId(1));
    model.sections[2].thickness = Some(9.0);
    let mut alternative = model.sections[1].clone();
    alternative.id = SectionId(3);
    alternative.material = Some(MaterialId(0));
    model.sections.push(alternative);
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut data = model.elements[3].clone();
        data.kind = representation;
        for kind in [
            None,
            Some(AnalysisKind::Incremental),
            Some(AnalysisKind::TimeHistory),
        ] {
            assert!(wall_side_column_input_issue(&data, &model, kind).is_none());
            assert!(crate::wall::side_column::wall_side_column_release(&data, &model).is_none());
            let valid = match kind {
                None => build_behavior(&data, &model),
                Some(analysis) => {
                    build_nonlinear_behavior(&data, &model, StrengthBasis::Nominal, analysis, None)
                }
            };
            assert_eq!(valid.n_dof(), 12);
            assert!(!valid.owns_frame_member_loads());
            let mut conflicting = data.clone();
            conflicting.section = Some(SectionId(3));
            let reason = wall_side_column_input_issue(&conflicting, &model, kind).unwrap();
            assert!(reason.contains("断面参照"));
            let panic = std::panic::catch_unwind(|| match kind {
                None => build_behavior(&conflicting, &model),
                Some(analysis) => build_nonlinear_behavior(
                    &conflicting,
                    &model,
                    StrengthBasis::Nominal,
                    analysis,
                    None,
                ),
            })
            .err()
            .unwrap();
            assert_eq!(panic.downcast_ref::<String>(), Some(&reason));
        }
    }
}

#[test]
fn steel_plate_wall_factory_rejects_boundary_column_with_girder_section_use() {
    let mut model = wall_side_column_factory_model();
    model.materials[1].category = MaterialCategory::Steel;
    for index in [0, 1] {
        let mut section = sepika_core::section_shape::SectionShape::SteelBox {
            height: 600.0,
            width: 600.0,
            thick: 12.0,
            corner_r: Some(0.0),
        }
        .to_section(SectionId(index), "S".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        section.material = Some(MaterialId(1));
        model.sections[index as usize] = section;
    }
    model.sections[2].shape = None;
    model.sections[2].material = Some(MaterialId(1));
    model.sections[2].thickness = Some(9.0);
    let wall = model
        .elements
        .iter()
        .find(|elem| elem.kind == ElementKind::Wall)
        .unwrap();
    for kind in [
        None,
        Some(AnalysisKind::Incremental),
        Some(AnalysisKind::TimeHistory),
    ] {
        let reason = wall_side_column_input_issue(wall, &model, kind).unwrap();
        assert!(reason.contains("側柱には柱用断面が必要"), "{reason}");
        let panic = std::panic::catch_unwind(|| match kind {
            None => build_behavior(wall, &model),
            Some(kind) => {
                build_nonlinear_behavior(wall, &model, StrengthBasis::Nominal, kind, None)
            }
        })
        .err()
        .unwrap();
        assert_eq!(panic.downcast_ref::<String>(), Some(&reason));
    }
}

#[test]
fn side_column_construction_clones_allow_representation_reverse_ends_and_original_end_conditions() {
    use crate::behavior::Ctx;
    let model = wall_side_column_factory_model();
    let ctx = Ctx { model: &model };
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for reverse in [false, true] {
            for condition in [
                EndCondition::Fixed,
                EndCondition::Pinned,
                EndCondition::SemiRigid { k_theta: 1e8 },
            ] {
                for regime in [ForceRegime::Auto, ForceRegime::AxialBendingInteract] {
                    let mut data = model.elements[3].clone();
                    data.kind = representation;
                    if reverse {
                        data.nodes.reverse();
                    }
                    data.end_cond[0] = condition;
                    data.force_regime = regime;
                    for analysis in [None, Some(AnalysisKind::Incremental)] {
                        assert!(wall_side_column_input_issue(&data, &model, analysis).is_none());
                        let member = match analysis {
                            None => build_behavior(&data, &model),
                            Some(kind) => build_nonlinear_behavior(
                                &data,
                                &model,
                                StrengthBasis::Nominal,
                                kind,
                                None,
                            ),
                        };
                        let matrix = member.tangent_stiffness(&ctx);
                        assert_eq!(member.n_dof(), 12);
                        for i in 0..12 {
                            for j in 0..12 {
                                assert!(matrix.get(i, j).is_finite());
                            }
                        }
                        if analysis.is_some() {
                            assert!(member.fiber_section_states().is_some());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn wall_side_column_factories_preserve_initial_stiffness_release_and_axial_history() {
    use crate::behavior::{Ctx, LocalVec};
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_side_column_factory_model();
        model.elements[3].kind = kind;
        let data = &model.elements[3];
        let ctx = Ctx { model: &model };
        assert!(
            wall_side_column_input_issue(data, &model, Some(AnalysisKind::Incremental)).is_none()
        );
        let mut linear = build_behavior(data, &model);
        let mut nonlinear = build_nonlinear_behavior(
            data,
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::Incremental,
            None,
        );
        let k = linear.tangent_stiffness(&ctx);
        let kn = nonlinear.tangent_stiffness(&ctx);
        for i in 0..12 {
            for j in 0..12 {
                approx::assert_relative_eq!(
                    k.get(i, j),
                    kn.get(i, j),
                    epsilon = 1e-4,
                    max_relative = 1e-8
                );
            }
        }
        let mut u = vec![0.0; 12];
        u[6] = 1.0;
        linear
            .update_state(
                &LocalVec {
                    data: u.clone().into(),
                },
                false,
                &ctx,
            )
            .unwrap();
        nonlinear
            .update_state(&LocalVec { data: u.into() }, false, &ctx)
            .unwrap();
        assert!(linear
            .internal_force(&ctx)
            .data
            .iter()
            .all(|f| f.abs() < 1e-3));
        assert!(nonlinear
            .internal_force(&ctx)
            .data
            .iter()
            .all(|f| f.abs() < 1e-3));
        let view = build_hinge_view(
            data,
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::Incremental,
            0.0,
            4,
            8,
        )
        .unwrap();
        assert_eq!(
            view.model,
            if kind == ElementKind::MultiSpring {
                AnalysisHingeModel::MultiSpring
            } else {
                AnalysisHingeModel::Fiber
            }
        );
        for sign in [-1.0, 1.0] {
            let mut member = build_nonlinear_behavior(
                data,
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::Incremental,
                None,
            );
            let mut previous = 0.0;
            let mut peak = 0.0;
            for delta in [
                sign * 0.05,
                sign * 2.0,
                sign * 10.0,
                sign * 8.0,
                sign * 10.0,
            ] {
                let mut du = vec![0.0; 12];
                du[8] = delta - previous;
                member
                    .update_state(&LocalVec { data: du.into() }, true, &ctx)
                    .unwrap();
                let force = member.internal_force(&ctx).data[8];
                assert!(force.is_finite());
                if delta.abs() == 10.0 {
                    peak = force;
                }
                previous = delta;
            }
            assert!(peak.abs() > 1.0);
            assert!(!member.fiber_section_states().expect("材料履歴").is_empty());
            let elastic = k.get(8, 8) * previous;
            assert!(
                (peak - elastic).abs() > elastic.abs() * 0.1,
                "弾性置換ではない {kind:?} {sign}: {peak} vs {elastic}"
            );
        }
    }
}

#[test]
fn wall_side_column_rejections_agree_on_analysis_ids_and_reasons() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_side_column_factory_model();
        model.elements[3].kind = kind;
        for analysis in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
            model.elements[3].force_regime = ForceRegime::UniaxialBendingShear;
            if kind != ElementKind::Beam && analysis == AnalysisKind::Incremental {
                continue;
            }
            let data = &model.elements[3];
            let issue =
                wall_side_column_input_issue(data, &model, Some(analysis)).expect("未定式化");
            assert!(issue.contains("部材 ID 3") && issue.contains("壁 ID [2]"));
            assert!(nonlinear_input_issues_for_kind(&model, analysis).contains(&issue));
            let view = build_hinge_view(data, &model, StrengthBasis::Nominal, analysis, 0.0, 4, 8)
                .unwrap();
            assert_eq!(view.model, AnalysisHingeModel::Other);
            assert_eq!(view.unavailability_reason, Some(issue.clone()));
            let panic = std::panic::catch_unwind(|| {
                build_nonlinear_behavior(data, &model, StrengthBasis::Nominal, analysis, None)
            })
            .err()
            .expect("直接factory拒否");
            assert_eq!(panic.downcast_ref::<String>(), Some(&issue));
        }
        model.elements[3].force_regime = ForceRegime::Auto;
        model.sections[1].material = None;
        let data = &model.elements[3];
        let issue = wall_side_column_input_issue(data, &model, None).unwrap();
        assert!(issue.contains("材料が設定されていません"));
        let panic = std::panic::catch_unwind(|| build_behavior(data, &model))
            .err()
            .unwrap();
        assert_eq!(panic.downcast_ref::<String>(), Some(&issue));
    }
}

#[test]
fn wall_side_columns_do_not_connect_out_of_scope_rc_panel_for_three_representations() {
    use crate::behavior::Ctx;
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_side_column_factory_model();
        model.elements[3].kind = kind;
        let mut panel = model.elements[3].clone();
        panel.id = ElemId(5);
        panel.kind = ElementKind::PanelZone;
        panel.nodes = smallvec::smallvec![NodeId(0)];
        model.elements.push(panel);
        let data = &model.elements[3];
        let ctx = Ctx { model: &model };
        let no_panel = {
            let mut bare = model.clone();
            bare.elements.pop();
            build_behavior(data, &bare).tangent_stiffness(&Ctx { model: &bare })
        };
        for nonlinear in [false, true] {
            let member = if nonlinear {
                build_nonlinear_behavior(
                    data,
                    &model,
                    StrengthBasis::Nominal,
                    AnalysisKind::Incremental,
                    None,
                )
            } else {
                build_behavior(data, &model)
            };
            assert_eq!(member.n_dof(), 12);
            let k = member.tangent_stiffness(&ctx);
            for i in 0..12 {
                for j in 0..12 {
                    approx::assert_relative_eq!(
                        k.get(i, j),
                        no_panel.get(i, j),
                        epsilon = 1e-4,
                        max_relative = 1e-8
                    );
                }
            }
        }
    }
}

#[test]
fn panel_757_valid_steel_joint_couples_each_factory_representation_once() {
    use crate::behavior::Ctx;
    use sepika_core::section_shape::SectionShape;

    let mut model = make_diaphragm_model();
    model.nodes[1].coord = [6_000.0, 0.0, 3_000.0];
    model.materials[0].name = "SN400B".into();
    model.materials[0].young = 205_000.0;
    model.materials[0].fy = Some(235.0);
    for (index, usage, height, width, web_thick, flange_thick) in [
        (0, FrameSectionUse::Girder, 600.0, 200.0, 11.0, 17.0),
        (1, FrameSectionUse::Column, 400.0, 300.0, 13.0, 21.0),
    ] {
        let mut section = SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            root_r: Some(0.0),
        }
        .to_section(SectionId(index), "S".into());
        section.frame_use = Some(usage);
        section.material = Some(MaterialId(0));
        model.sections[index as usize] = section;
    }
    let column = ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(2)],
        section: Some(SectionId(1)),
        local_axis: LocalAxis {
            ref_vector: [0.6, 0.8, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let mut girder = column.clone();
    girder.id = ElemId(1);
    girder.nodes = smallvec::smallvec![NodeId(2), NodeId(1)];
    girder.section = Some(SectionId(0));
    girder.local_axis.ref_vector = [0.0, 0.0, 1.0];
    let mut panel = column.clone();
    panel.id = ElemId(2);
    panel.kind = ElementKind::PanelZone;
    panel.nodes = smallvec::smallvec![NodeId(2)];
    panel.section = None;
    model.elements = vec![column, girder, panel];
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        model.elements[0].kind = representation;
        let mut bare = model.clone();
        bare.elements.pop();
        for nonlinear in [false, true] {
            let build = |model: &Model| {
                let data = &model.elements[0];
                if nonlinear {
                    build_nonlinear_behavior(
                        data,
                        model,
                        StrengthBasis::Nominal,
                        AnalysisKind::Incremental,
                        None,
                    )
                } else {
                    build_behavior(data, model)
                }
            };
            let inner = build(&bare);
            let wrapped = build(&model);
            assert_eq!(inner.n_dof(), 12);
            assert_eq!(wrapped.n_dof(), 14);
            let expected = inner.tangent_stiffness(&Ctx { model: &bare });
            let actual = wrapped.tangent_stiffness(&Ctx { model: &model });
            for i in 0..12 {
                for j in 0..12 {
                    approx::assert_relative_eq!(
                        actual.get(i, j),
                        expected.get(i, j),
                        epsilon = 1e-4,
                        max_relative = 1e-8
                    );
                }
                for axis in 0..2 {
                    approx::assert_relative_eq!(
                        actual.get(i, 12 + axis),
                        0.5 * expected.get(i, 9 + axis),
                        epsilon = 1e-4,
                        max_relative = 1e-8
                    );
                    approx::assert_relative_eq!(
                        actual.get(12 + axis, i),
                        0.5 * expected.get(9 + axis, i),
                        epsilon = 1e-4,
                        max_relative = 1e-8
                    );
                }
            }
            for i in 0..2 {
                for j in 0..2 {
                    approx::assert_relative_eq!(
                        actual.get(12 + i, 12 + j),
                        0.25 * expected.get(9 + i, 9 + j),
                        epsilon = 1e-4,
                        max_relative = 1e-8
                    );
                }
            }
        }
    }
}
