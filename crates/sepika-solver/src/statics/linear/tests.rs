use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, JointKind, LoadCase,
    LocalAxis, Material, MaterialCategory, MemberDetailAttr, MemberJoint, MemberLoad,
    MemberLoadKind, Model, NodalLoad, Node, Section,
};

#[test]
fn no_long_axial_column_targets_fiber_and_multispring_by_section_use() {
    let mut model = Model::default();
    let mut section = Section::zero(SectionId(0), "C1".into());
    section.frame_use = Some(FrameSectionUse::Column);
    model.sections.push(section);
    model.stress_cfg.no_long_axial_column = true;
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let elem = ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![],
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
        assert!(is_axial_disabled_target(&elem, &model, &model.stress_cfg));
    }
}

/// 単純梁（i:ピン, j:ローラ）に等分布荷重 → 中央曲げ wL²/8、端部 0 を検証。
/// 曲げは静定なので EI に依らず厳密。組立（等価節点力）＋回復（重ね合わせ）の総合検証。
#[test]
fn simply_supported_udl_midspan_moment() {
    let l = 1000.0_f64;
    let w = 2.0_f64;
    let model = Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                // Ux,Uy,Uz,Rx 拘束（並進ピン＋ねじり剛体モード除去）
                restraint: Dof6Mask(0b001111),
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [l, 0.0, 0.0],
                // Uy,Uz 拘束（ローラ。Ux 自由）
                restraint: Dof6Mask(0b000110),
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
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
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "s".into(),
            area: 1000.0,
            iy: 1.0e7,
            iz: 1.0e7,
            j: 1.0e6,
            depth: 200.0,
            width: 100.0,
            as_y: 800.0,
            as_z: 800.0,
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
            name: "m".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "udl".into(),
            nodal: vec![],
            member: vec![MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: l,
                    w1: w,
                    w2: w,
                },
            )],
        }],
        ..Default::default()
    };

    let res = linear_static_once(&model, LoadCaseId(1)).expect("solve");
    let (_, mf) = res
        .member_forces
        .iter()
        .find(|(id, _)| *id == ElemId(0))
        .expect("member forces for elem 0");

    let expected_mid = w * l * l / 8.0;
    let mut mid_mz = None;
    let mut end_mz_max = 0.0_f64;
    for (xi, vals) in &mf.at {
        let mz = vals[5];
        if (xi - 0.5).abs() < 1e-9 {
            mid_mz = Some(mz);
        }
        if (*xi < 1e-9) || ((xi - 1.0).abs() < 1e-9) {
            end_mz_max = end_mz_max.max(mz.abs());
        }
    }
    let mid = mid_mz.expect("midspan section present");
    assert!(
        (mid.abs() - expected_mid).abs() / expected_mid < 1e-3,
        "midspan Mz={} expected {}",
        mid,
        expected_mid
    );
    assert!(
        end_mz_max < expected_mid * 1e-3,
        "end Mz should be ~0, got {}",
        end_mz_max
    );
}

/// 単純梁モデル（長さ l、i:ピン+ねじり拘束, j:ローラ）を指定の部材荷重で作る。
fn ss_beam(l: f64, member: Vec<MemberLoad>) -> Model {
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask(0b001111),
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [l, 0.0, 0.0],
                restraint: Dof6Mask(0b000110),
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
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
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "s".into(),
            area: 1000.0,
            iy: 1.0e7,
            iz: 1.0e7,
            j: 1.0e6,
            depth: 200.0,
            width: 100.0,
            as_y: 800.0,
            as_z: 800.0,
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
            name: "m".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "lc".into(),
            nodal: vec![],
            member,
        }],
        ..Default::default()
    }
}

fn owned_udl_model() -> Model {
    let mut model = ss_beam(
        1000.0,
        vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 1.0, 0.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 2.0,
                w2: 2.0,
            },
        )],
    );
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FIXED;
    }
    model
}

#[test]
fn short_brace_distribution_public_static_paths_preserve_force_and_first_moment() {
    use crate::statics::analysis::Analysis;
    for (wi, wj, resultant, first_moment) in [
        (1e16, 3e16, 2000.0, 7.0 / 6.0 * 1e-10),
        (3e16, 1e16, 2000.0, 5.0 / 6.0 * 1e-10),
        (-1e16, -3e16, -2000.0, -7.0 / 6.0 * 1e-10),
        (1e16, -1e16, 0.0, -1.0 / 6.0 * 1e-10),
    ] {
        let mut model = ss_beam(
            1.0,
            vec![MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 1e-13,
                    w1: wi,
                    w2: wj,
                },
            )],
        );
        model.elements[0].kind = ElementKind::Brace {
            tension_only: false,
        };
        model.sections[0].frame_use = Some(FrameSectionUse::Brace);
        for node in &mut model.nodes {
            node.restraint = Dof6Mask(0b111101);
            node.support_spring = Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        }
        let analysis = Analysis::prepare(&model).unwrap();
        for result in [
            linear_static_once(&model, LoadCaseId(1)).unwrap(),
            analysis.linear_static(LoadCaseId(1)).unwrap(),
        ] {
            let [ri, rj] = [result.disp[0][1], result.disp[1][1]];
            assert!(
                (ri + rj - resultant).abs() < 1e-10,
                "{ri:e} + {rj:e} != {resultant:e}"
            );
            assert!(
                (rj / first_moment - 1.0).abs() < 2e-14,
                "{rj:e} != {first_moment:e}"
            );
            for (_, forces) in result.member_forces {
                for (_, force) in forces.at {
                    assert_eq!(force, [0.0; 6]);
                }
            }
        }
    }
}

#[test]
fn linear_rhs_rejects_missing_or_reordered_behaviors() {
    let mut model = owned_udl_model();
    let mut second = model.elements[0].clone();
    second.id = ElemId(1);
    model.elements.push(second);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    let mut stiffness =
        StaticStiffness::build(&model, &map, &reducer, model.load_cases[0].kind).unwrap();
    let entries: Vec<_> = stiffness
        .behaviors
        .iter()
        .map(|(id, b, g)| (*id, b.as_ref(), g.as_slice()))
        .collect();
    let ledger = PhysicalLoadLedger::new(&model, &model.load_cases[0]).unwrap();
    assert!(assemble_linear_rhs(&model, &map, &ledger, &entries[..1]).is_err());
    assert!(assemble_linear_rhs(&model, &map, &ledger, &[entries[1], entries[0]]).is_err());
    stiffness.behaviors.swap(0, 1);
    assert!(solve_static_case(
        &model,
        &map,
        &reducer,
        &model.load_cases[0],
        Some((stiffness.solver.as_ref(), &stiffness.behaviors)),
        TENSION_ONLY_MAX_ITER
    )
    .is_err());
}

#[test]
fn wall_normal_release_retains_loaded_support_balance_without_end_moments() {
    let model = owned_udl_model();
    let map = DofMap::build(&model);
    let elem = &model.elements[0];
    let beam = sepika_element::frame::beam::BeamElement::new(elem, &model);
    let behavior = sepika_element::wall::side_column::InPlaneReleasedColumn::new(
        beam,
        sepika_element::wall::side_column::ReleaseAxis::LocalY,
    );
    let gdofs = behavior.global_dofs(&map);
    let ledger = PhysicalLoadLedger::new(&model, &model.load_cases[0]).unwrap();
    let entries = [(elem.id, &behavior as &dyn ElementBehavior, gdofs.as_slice())];
    assert!(assemble_linear_rhs(&model, &map, &ledger, &entries)
        .unwrap()
        .is_empty());
    let (result, reactions) = recover_static(&model, &map, &[], &ledger, &entries).unwrap();
    for reaction in &reactions {
        assert!((reaction[1] + 1000.0).abs() < 1e-8);
        assert!(reaction[5].abs() < 1e-7);
    }
    assert!((value_at(&result.member_forces[0].1, 0.5, 4).abs() - 250000.0).abs() < 1e-6);
}

#[test]
fn ordinary_fixed_udl_reactions_balance_original_force_and_moment() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = owned_udl_model();
        model.nodes[1].coord = [6000.0, 0.0, 0.0];
        model.elements[0].kind = kind;
        model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.load_cases[0].member[0] = sepika_core::model::MemberLoad::manual(
            model.elements[0].id,
            [0.0, 1.0, 0.0],
            sepika_core::model::MemberLoadKind::Distributed {
                a: 0.0,
                b: 6000.0,
                w1: 1.0,
                w2: 1.0,
            },
        );
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        let stiffness =
            StaticStiffness::build(&model, &map, &reducer, model.load_cases[0].kind).unwrap();
        let entries: Vec<_> = stiffness
            .behaviors
            .iter()
            .map(|(id, b, g)| (*id, b.as_ref(), g.as_slice()))
            .collect();
        let ledger = PhysicalLoadLedger::new(&model, &model.load_cases[0]).unwrap();
        let (_, reactions) = recover_static(&model, &map, &[], &ledger, &entries).unwrap();
        for reaction in &reactions {
            assert!((reaction[1] + 3000.0).abs() < 1e-8);
        }
        assert!((reactions[0][5] + 3e6).abs() < 1e-7);
        assert!((reactions[1][5] - 3e6).abs() < 1e-7);
        assert!((reactions[0][1] + reactions[1][1] + 6000.0).abs() < 1e-8);
        assert!((reactions[0][5] + reactions[1][5] + 6000.0 * reactions[1][1] + 18e6).abs() < 1e-7);
    }
}

#[test]
fn wall_physical_balance_preserves_normalized_node_order() {
    let mut model = owned_udl_model();
    model.load_cases[0].member.clear();
    let first = model.nodes[0].clone();
    model.nodes = [
        [0.0, 0.0, 0.0],
        [4000.0, 0.0, 0.0],
        [4000.0, 0.0, 3000.0],
        [0.0, 0.0, 3000.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(i, coord)| Node {
        id: NodeId(i as u32),
        coord,
        restraint: Dof6Mask::FREE,
        ..first.clone()
    })
    .collect();
    model.elements[0].kind = ElementKind::Wall;
    model.sections[0].frame_use = None;
    model.sections[0].thickness = Some(10.0);
    model.materials[0].fy = Some(345.0);
    let map = DofMap::build(&Model {
        elements: vec![ElementData {
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            ..model.elements[0].clone()
        }],
        ..model.clone()
    });
    let mut u = vec![0.0; map.n_active()];
    u[map.active(2 * 6).unwrap() as usize] = 0.3;
    u[map.active(3 * 6 + 2).unwrap() as usize] = -0.2;
    let mut expected: Option<Vec<f64>> = None;
    for nodes in [[0, 1, 2, 3], [1, 2, 3, 0], [3, 2, 1, 0]] {
        model.elements[0].nodes = nodes.into_iter().map(NodeId).collect();
        let behavior =
            sepika_element::wall::wall_element::WallElement::try_new(&model.elements[0], &model)
                .unwrap();
        let gdofs = behavior.global_dofs(&map);
        let ledger = PhysicalLoadLedger::new(&model, &model.load_cases[0]).unwrap();
        let (_, reactions) = recover_static(
            &model,
            &map,
            &u,
            &ledger,
            &[(ElemId(0), &behavior, gdofs.as_slice())],
        )
        .unwrap();
        if let Some(expected) = &expected {
            for (actual, expected) in reactions.iter().flatten().zip(expected) {
                assert!((actual - expected).abs() < 1e-6);
            }
        } else {
            expected = Some(reactions.into_iter().flatten().collect::<Vec<_>>());
        }
    }
}

fn value_at(mf: &sepika_element::frame::beam::MemberForces, xi: f64, comp: usize) -> f64 {
    mf.at
        .iter()
        .find(|(x, _)| (x - xi).abs() < 1e-9)
        .map(|(_, v)| v[comp])
        .expect("section present")
}

fn mid_value(mf: &sepika_element::frame::beam::MemberForces, comp: usize) -> f64 {
    value_at(mf, 0.5, comp)
}

/// 単純梁・材長 1/4 点の集中荷重 P の内力回復を、荷重より先の断面 x=3L/8 で
/// 検証する。x>a なので `fixed_internal_local` の Point 回復項（合力・
/// モーメント）が非ゼロで効く。
///
/// 静定梁の手計算: 支点反力 R_i = P·b/L（b = L−a）、M(x) = R_i·x − P·(x−a)、
/// Qy(x) = R_i − P。評価断面 x=3L/8 は既定の評価断面（0, L/2, L）に含まれない
/// ため、部材付帯情報の継手位置で追加する（剛性・応力解析には影響しない）。
#[test]
fn simply_supported_off_center_point_load_internal_force() {
    let l = 1000.0_f64;
    let p = 500.0_f64;
    let a = l / 4.0;
    let b = l - a;
    let r_i = p * b / l;
    let x = 3.0 * l / 8.0;
    let mut model = ss_beam(
        l,
        vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Point { a, p },
        )],
    );
    model.member_detail_attrs.push(MemberDetailAttr {
        elem: ElemId(0),
        haunch_i: None,
        haunch_j: None,
        joints: vec![MemberJoint {
            distance: x,
            kind: JointKind::Site,
        }],
    });
    let res = linear_static_once(&model, LoadCaseId(1)).expect("solve");
    let (_, mf) = res
        .member_forces
        .iter()
        .find(|(id, _)| *id == ElemId(0))
        .unwrap();
    let xi = x / l;
    let m = value_at(mf, xi, 5);
    let expected_m = r_i * x - p * (x - a);
    assert!(
        (m - expected_m).abs() / expected_m < 1e-3,
        "point Mz={} expected {}",
        m,
        expected_m
    );
    let shear = value_at(mf, xi, 1);
    let expected_shear = r_i - p;
    assert!(
        (shear - expected_shear).abs() / expected_shear.abs() < 1e-3,
        "point Qy={} expected {}",
        shear,
        expected_shear
    );
}

/// 単純梁・全体 Y 方向 UDL（ローカル z 面）→ 中央 My = wL²/8。z 面の符号検証。
#[test]
fn simply_supported_udl_zplane_moment() {
    let l = 1000.0_f64;
    let w = 1.5_f64;
    let model = ss_beam(
        l,
        vec![MemberLoad::manual(
            ElemId(0),
            [0.0, -1.0, 0.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: l,
                w1: w,
                w2: w,
            },
        )],
    );
    let res = linear_static_once(&model, LoadCaseId(1)).expect("solve");
    let (_, mf) = res
        .member_forces
        .iter()
        .find(|(id, _)| *id == ElemId(0))
        .unwrap();
    let expected = w * l * l / 8.0;
    let mid = mid_value(mf, 4).abs();
    assert!(
        (mid - expected).abs() / expected < 1e-3,
        "zplane mid My={} expected {}",
        mid,
        expected
    );
    // ねじり・Mz は概ね 0
    assert!(mid_value(mf, 5).abs() < expected * 1e-3, "Mz leak");
}

fn make_axial_cantilever() -> Model {
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
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
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
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "sec".to_string(),
            area: 100.0,
            iy: 1000.0,
            iz: 1000.0,
            j: 100.0,
            depth: 100.0,
            width: 100.0,
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
            name: "mat".to_string(),
            category: MaterialCategory::Steel,
            young: 1000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "axial".to_string(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    }
}

#[test]
fn test_linear_static_axial_cantilever() {
    let model = make_axial_cantilever();
    let result = linear_static_once(&model, LoadCaseId(1)).unwrap();
    assert!(
        (result.disp[1][0] - 10.0).abs() < 1e-6,
        "ux={}",
        result.disp[1][0]
    );
    assert!(result.member_forces.len() == 1);
    let forces = &result.member_forces[0].1;
    // 軸力 N は部材内力（引張正）。先端 +1000N の引張で全断面 N=+1000。
    for (xi, vals) in &forces.at {
        assert!((vals[0] - 1000.0).abs() < 1e-6, "N(ξ={})={}", xi, vals[0]);
    }
}

#[test]
fn test_linear_static_once_unknown_load_case() {
    let model = make_axial_cantilever();
    let err = linear_static_once(&model, LoadCaseId(99)).unwrap_err();
    assert!(
        matches!(err, SolveError::InvalidInput(ref msg) if msg.contains("99")),
        "{err:?}"
    );
}

#[test]
fn test_linear_static_once_zero_independent_dofs_load_case_validation() {
    for all_fixed in [true, false] {
        let mut model = make_axial_cantilever();
        if all_fixed {
            model.nodes[1].restraint = Dof6Mask::FIXED;
        } else {
            model
                .constraints
                .push(sepika_core::model::Constraint::RigidLink {
                    master: NodeId(0),
                    slaves: vec![NodeId(1)],
                    dofs: Dof6Mask::FIXED,
                });
        }
        let dofmap = DofMap::build(&model);
        assert_eq!(dofmap.n_active() == 0, all_fixed);
        assert_eq!(Reducer::build(&model, &dofmap).n_indep, 0);
        let result = linear_static_once(&model, LoadCaseId(1)).unwrap();
        assert_eq!(result.disp, vec![[0.0; 6]; model.nodes.len()]);
        assert_eq!(result.member_forces.len(), 1);
        assert!(result.member_forces[0]
            .1
            .at
            .iter()
            .all(|(_, f)| f.iter().all(|v| *v == 0.0)));
        assert!(result.panel_moments.is_empty());
        let err = linear_static_once(&model, LoadCaseId(99)).unwrap_err();
        assert!(
            matches!(err, SolveError::InvalidInput(ref msg) if msg.contains("99")),
            "{err:?}"
        );
    }
}

#[test]
fn test_linear_static_once_tension_only_unknown_load_case_rejected_at_entry() {
    let mut model = tension_only_portal(1.0e5, true);
    model.stress_cfg.tension_only_iteration = true;
    assert!(has_tension_only_brace(&model));
    assert!(linear_static_once(&model, LoadCaseId(1)).is_ok());
    let err = linear_static_once(&model, LoadCaseId(99)).unwrap_err();
    assert!(
        matches!(err, SolveError::InvalidInput(ref msg) if msg.contains("99")),
        "{err:?}"
    );

    // 剛性が特異でも、反復の求解エラーより先に不存在ケースを拒否する。
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FREE;
    }
    let err = linear_static_once(&model, LoadCaseId(99)).unwrap_err();
    assert!(
        matches!(err, SolveError::InvalidInput(ref msg) if msg.contains("99")),
        "{err:?}"
    );
}

#[test]
fn axial_distributed_gravity_recovers_full_base_compression() {
    let mut model = make_axial_cantilever();
    model.nodes[1].coord = [0.0, 0.0, 1000.0];
    model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
    model.load_cases[0].nodal.clear();
    model.load_cases[0].member = vec![MemberLoad::manual(
        ElemId(0),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: 2.0,
            w2: 2.0,
        },
    )];
    let result = linear_static_once(&model, LoadCaseId(1)).unwrap();
    // 上端自由の柱は、各断面より上の全重量を圧縮軸力として負担する。
    for (xi, values) in &result.member_forces[0].1.at {
        let expected = -2000.0 * (1.0 - xi);
        assert!(
            (values[0] - expected).abs() < 1e-6,
            "xi={xi}: N={}, expected={expected}",
            values[0]
        );
    }
}

/// X 軸上の片持ち梁に「グローバル Y 方向」の先端荷重をかける。
/// 参照ベクトル [0,0,1] では local y = global Z（鉛直上）、local z = global −Y と
/// なるので、水平（Y 方向）たわみは弱軸＝断面 **iz** で決まる（iy=強軸ではない）。
/// クロス変換（construct.rs）または to_global を欠くと iy を使ってしまい誤る。
/// よって iy≠iz の断面で、δ = PL³/(3E·iz) に一致することを確認する。
#[test]
fn test_beam_to_global_transverse_uses_correct_inertia() {
    // 現実的な鋼材大断面（iz=1e9 級）を用いる：to_global 修正の検証に加え、
    // 端ばね静縮約のペナルティが大断面でも非正定値化しないこと（堅牢性）も同時に確認。
    let e = 205000.0_f64;
    let l = 1000.0_f64;
    let iy = 2.0e9_f64;
    let iz = 1.0e9_f64;
    let p = 10000.0_f64;
    let mut model = make_axial_cantilever();
    model.materials[0].young = e;
    model.sections[0].iy = iy;
    model.sections[0].iz = iz;
    model.sections[0].as_y = 1.0e9;
    model.sections[0].as_z = 1.0e9;
    model.load_cases[0].nodal[0].values = [0.0, p, 0.0, 0.0, 0.0, 0.0];

    let result = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let uy = result.disp[1][1];
    let expected = p * l.powi(3) / (3.0 * e * iz);
    let buggy = p * l.powi(3) / (3.0 * e * iy);
    // iz ベースの値に一致し、iy ベース(1/2)を明確に排除する。
    assert!(
        (uy - expected).abs() / expected < 1e-3,
        "uy={} expected(iz)={} buggy(iy)={}",
        uy,
        expected,
        buggy
    );
}

/// 剛域がモデル→解析へ接続され、結果に効くことのエンドツーエンド確認。
/// 同一片持ち梁で、基部に大きな剛域（可とう長を短縮）を入れると、
/// 先端たわみが明確に小さく（剛く）なる。
#[test]
fn test_rigid_zone_affects_analysis() {
    let mut base = make_axial_cantilever();
    base.sections[0].iy = 1.0e7;
    base.sections[0].iz = 1.0e7;
    base.sections[0].as_y = 1.0e8;
    base.sections[0].as_z = 1.0e8;
    base.load_cases[0].nodal[0].values = [0.0, 0.0, 1000.0, 0.0, 0.0, 0.0];

    // 剛域なし
    let r0 = linear_static_once(&base, LoadCaseId(1)).unwrap();
    let uz0 = r0.disp[1][2];

    // 基部に剛域 λ_i=800（可とう長 200）
    let mut rigid = base.clone();
    rigid.elements[0].rigid_zone.length_i = 800.0;
    let r1 = linear_static_once(&rigid, LoadCaseId(1)).unwrap();
    let uz1 = r1.disp[1][2];

    assert!(
        uz0.abs() > 0.0 && uz1.abs() > 0.0,
        "uz0={} uz1={}",
        uz0,
        uz1
    );
    assert!(
        uz1.abs() < 0.5 * uz0.abs(),
        "剛域で剛くなるはず: uz_norigid={} uz_rigid={}",
        uz0,
        uz1
    );
}

#[test]
fn test_linear_static_vertical_cantilever_bending() {
    // 鉛直柱: (0,0,0)固定 → (0,0,1000)自由。頂部に水平荷重 P=1000 (global X)。
    // 座標変換が正しく適用されれば曲げ片持ち応答 δx ≈ PL³/3E·Iz + せん断 ≈ 333,364。
    // 回転変換が欠落していると軸剛性を誤用して δx≈10 になる（回帰防止）。
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
                coord: [0.0, 0.0, 1000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
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
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "sec".to_string(),
            area: 100.0,
            iy: 1000.0,
            iz: 1000.0,
            j: 100.0,
            depth: 100.0,
            width: 100.0,
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
            name: "mat".to_string(),
            category: MaterialCategory::Steel,
            young: 1000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "h".to_string(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    };
    let result = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let ux = result.disp[1][0];
    // 曲げ主成分 333,333 + せん断 ~31。軸剛性誤用(=10)を確実に弾く帯域で判定。
    assert!(
            (333_000.0..=334_000.0).contains(&ux),
            "vertical cantilever tip ux={ux} (expected ~333,364 bending; got axial ~10 means rotation missing)"
        );
}

#[test]
fn test_linear_static_deterministic() {
    let model = make_axial_cantilever();
    let first = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let second = linear_static_once(&model, LoadCaseId(1)).unwrap();
    assert_eq!(first.disp, second.disp);
    assert_eq!(first.member_forces.len(), second.member_forces.len());
    for (a, b) in first.member_forces.iter().zip(second.member_forces.iter()) {
        assert_eq!(a.0, b.0);
        assert_eq!(a.1.at, b.1.at);
    }
}

#[test]
fn test_shell_rigid_floor_membrane_off() {
    use sepika_core::model::{Constraint, Story};

    let model = Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(StoryId(0)),
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [100.0, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(StoryId(0)),
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [100.0, 100.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(StoryId(0)),
                support_spring: None,
            },
            Node {
                id: NodeId(3),
                coord: [0.0, 100.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(StoryId(0)),
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Shell,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
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
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "shell".to_string(),
            area: 0.0,
            iy: 0.0,
            iz: 0.0,
            j: 0.0,
            depth: 0.0,
            width: 0.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: Some(10.0),
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
            name: "mat".to_string(),
            category: MaterialCategory::Steel,
            young: 1000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        stories: vec![Story {
            wall_weights: Vec::new(),
            level_kind: Default::default(),
            structure: Default::default(),
            id: StoryId(0),
            name: "floor".to_string(),
            elevation: Some(0.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            seismic_weight: None,
            weight_override: None,
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        }],
        constraints: vec![Constraint::rigid_diaphragm(
            StoryId(0),
            NodeId(0),
            vec![NodeId(1), NodeId(2), NodeId(3)],
        )],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "load".to_string(),
            nodal: vec![NodalLoad::manual(NodeId(2), [0.0, 0.0, 1.0, 0.0, 0.0, 0.0])],
            member: vec![],
        }],
        ..Default::default()
    };

    let res = linear_static_once(&model, LoadCaseId(1)).unwrap();
    assert!(
        res.disp[1][0].abs() < 1e-12 && res.disp[1][1].abs() < 1e-12,
        "slave should not move in-plane: {:?}",
        [res.disp[1][0], res.disp[1][1]]
    );
    assert!(
        res.disp[2][2].abs() > 1e-12,
        "shell should deflect vertically: {}",
        res.disp[2][2]
    );
}

/// 単純支持正方形板（等分布荷重）の N×N メッシュモデルを作る。
/// 周辺=単純支持（Uz=0, 縁回転自由）。面内は全節点で固定（平板曲げ＝面内変位0）。
fn make_ss_plate(n: usize, a: f64, t: f64, e: f64, nu: f64, q: f64, clamped: bool) -> Model {
    let h = a / n as f64;
    let nn = n + 1;
    let idx = |ix: usize, iy: usize| (iy * nn + ix) as u32;
    let mut nodes = Vec::new();
    for iy in 0..nn {
        for ix in 0..nn {
            let on_boundary = ix == 0 || ix == n || iy == 0 || iy == n;
            // 常に Ux,Uy,Rz を固定（面内＋ドリリング）。周辺は Uz も固定。
            let mut mask = 0b100011u8;
            if on_boundary {
                mask |= 1 << 2;
                if clamped {
                    mask |= 1 << 3;
                    mask |= 1 << 4;
                }
            }
            nodes.push(Node {
                id: NodeId(idx(ix, iy)),
                coord: [ix as f64 * h, iy as f64 * h, 0.0],
                restraint: Dof6Mask(mask),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
    }
    let mut elements = Vec::new();
    let mut eid = 0u32;
    for iy in 0..n {
        for ix in 0..n {
            elements.push(ElementData {
                id: ElemId(eid),
                kind: ElementKind::Shell,
                nodes: smallvec::smallvec![
                    NodeId(idx(ix, iy)),
                    NodeId(idx(ix + 1, iy)),
                    NodeId(idx(ix + 1, iy + 1)),
                    NodeId(idx(ix, iy + 1)),
                ],
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
            });
            eid += 1;
        }
    }
    // 等分布荷重 q を負担面積で節点 Fz へ（周辺節点の荷重は支点が負担）。
    let mut nodal = Vec::new();
    for iy in 0..nn {
        for ix in 0..nn {
            let wx = if ix == 0 || ix == n { 0.5 } else { 1.0 };
            let wy = if iy == 0 || iy == n { 0.5 } else { 1.0 };
            let fz = q * (wx * h) * (wy * h);
            nodal.push(NodalLoad::manual(
                NodeId(idx(ix, iy)),
                [0.0, 0.0, fz, 0.0, 0.0, 0.0],
            ));
        }
    }
    Model {
        nodes,
        elements,
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "plate".into(),
            area: 0.0,
            iy: 0.0,
            iz: 0.0,
            j: 0.0,
            depth: 0.0,
            width: 0.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: Some(t),
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
            name: "m".into(),
            category: MaterialCategory::Steel,
            young: e,
            poisson: nu,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "q".into(),
            nodal,
            member: vec![],
        }],
        ..Default::default()
    }
}

/// 正方形板（等分布荷重）の中央たわみが参照解 α·q·a⁴/D に収束すること。
///
/// ケース表で境界条件と α を切り替える。単純支持 α=0.00406、四辺固定（クランプ）
/// α=0.00126。各ケースで 4・8・16 分割の誤差が単調減少し、16×16 で参照解の
/// ±2% 以内に入る。
#[test]
fn test_plate_convergence() {
    let (a, t, e, nu, q) = (1000.0_f64, 10.0_f64, 200000.0_f64, 0.3_f64, 0.01_f64);
    let d = e * t.powi(3) / (12.0 * (1.0 - nu * nu));

    for (clamped, alpha, label) in [
        (false, 0.00406_f64, "単純支持"),
        (true, 0.00126_f64, "四辺固定"),
    ] {
        let ref_w = alpha * q * a.powi(4) / d;
        let center_w = |n: usize| -> f64 {
            let model = make_ss_plate(n, a, t, e, nu, q, clamped);
            let res = linear_static_once(&model, LoadCaseId(1)).unwrap();
            let nn = n + 1;
            let c = (n / 2) * nn + (n / 2);
            res.disp[c][2].abs()
        };

        let w4 = center_w(4);
        let w8 = center_w(8);
        let w16 = center_w(16);
        let e4 = (w4 - ref_w).abs();
        let e8 = (w8 - ref_w).abs();
        let e16 = (w16 - ref_w).abs();

        // 細分化で誤差が単調減少して参照解へ近づく
        assert!(
            e8 < e4 && e16 < e8,
            "{label}: 誤差が単調減少しない: e4={e4} e8={e8} e16={e16} (w4={w4} w8={w8} w16={w16} ref={ref_w})"
        );
        // 16×16 で参照解の ±2% 以内
        assert!(
            e16 / ref_w < 0.02,
            "{label}: 16x16 誤差 {:.2}% > 2% (w16={} ref={})",
            e16 / ref_w * 100.0,
            w16,
            ref_w
        );
    }
}

// 1 スパン・2 柱・頂部大梁・対角ブレース 1 本のモデル（ブレース付きラーメン）。
// 柱・大梁は Fixed-Fixed（曲げ骨組）、ブレースは Pinned-Pinned のトラス要素。
// 荷重ケースを 2 本（Dead=長期, Seismic=短期）用意し、同一の鉛直荷重を与える。
fn braced_frame(kind: sepika_core::model::LoadCaseKind) -> Model {
    use sepika_core::model::StressAnalysisCfg;

    let l = 4000.0_f64;
    let h = 3000.0_f64;
    let node = |id: u32, coord: [f64; 3]| Node {
        id: NodeId(id),
        coord,
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    };
    let sec = Section {
        frame_use: Some(FrameSectionUse::Column),
        id: SectionId(0),
        name: "steel".into(),
        area: 6000.0,
        iy: 8.0e7,
        iz: 8.0e7,
        j: 1.0e6,
        depth: 300.0,
        width: 300.0,
        as_y: 5000.0,
        as_z: 5000.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: None,
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    };
    let mat = Material {
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
    };
    Model {
        nodes: vec![
            {
                let mut n = node(0, [0.0, 0.0, 0.0]);
                n.restraint = Dof6Mask::FIXED;
                n
            },
            {
                let mut n = node(1, [l, 0.0, 0.0]);
                n.restraint = Dof6Mask::FIXED;
                n
            },
            node(2, [0.0, 0.0, h]),
            node(3, [l, 0.0, h]),
        ],
        elements: vec![
            // e0: 柱A（鉛直）
            ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(0), NodeId(2)],
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
            },
            // e1: 柱B（鉛直）
            ElementData {
                id: ElemId(1),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(1), NodeId(3)],
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
            },
            // e2: 頂部大梁（水平）
            ElementData {
                id: ElemId(2),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(2), NodeId(3)],
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
            },
            // e3: 対角ブレース（柱Aの脚部 → 柱Bの頂部）
            ElementData {
                id: ElemId(3),
                kind: ElementKind::Brace {
                    tension_only: false,
                },
                nodes: smallvec::smallvec![NodeId(0), NodeId(3)],
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
            },
        ],
        sections: vec![sec],
        materials: vec![mat],
        load_cases: vec![LoadCase {
            id: LoadCaseId(1),
            name: "gravity".into(),
            nodal: vec![NodalLoad::manual(
                NodeId(2),
                [0.0, 0.0, -1.0e5, 0.0, 0.0, 0.0],
            )],
            member: vec![],
            kind,
        }],
        stress_cfg: StressAnalysisCfg::default(),
        ..Default::default()
    }
}

// 柱の長期軸力無効化テスト用モデル。同一の2節点間に「柱」（`ElementKind::Beam`、
// 鉛直、Fixed-Fixed）と「鉛直ブレース」（`ElementKind::Brace`、同じ2節点）を
// 並列に配置する。荷重方向が部材軸と厳密に一致するため、柱の軸剛性が
// 支配的な baseline では柱・ブレースへほぼ等分に軸力を負担させつつ、
// 柱側だけ曲げ・せん断剛性（iy/iz/j/as_y/as_z）はそのまま健全に保つ。
// ブレース側は常に軸剛性のみを保つため、柱の軸力を無効化しても
// 機構化（特異行列）を起こさず、負担していた軸力がそのままブレースへ
// 移ることを明快に検証できる。
fn column_with_parallel_vertical_brace() -> Model {
    use sepika_core::model::StressAnalysisCfg;

    let h = 3000.0_f64;
    let sec = Section {
        frame_use: Some(FrameSectionUse::Column),
        id: SectionId(0),
        name: "steel".into(),
        area: 6000.0,
        iy: 8.0e7,
        iz: 8.0e7,
        j: 1.0e6,
        depth: 300.0,
        width: 300.0,
        as_y: 5000.0,
        as_z: 5000.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: None,
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    };
    let mat = Material {
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
    };
    Model {
        nodes: vec![
            {
                let mut n = Node {
                    id: NodeId(0),
                    coord: [0.0, 0.0, 0.0],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                };
                n.restraint = Dof6Mask::FIXED;
                n
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, h],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![
            // e0: 柱（鉛直）。no_long_axial_column の対象。
            ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
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
            },
            // e1: 同じ2節点間の鉛直ブレース（並列）。常に軸剛性を保ち、
            // 柱が負担しなくなった軸力の受け皿になる。
            ElementData {
                id: ElemId(1),
                kind: ElementKind::Brace {
                    tension_only: false,
                },
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [1.0, 0.0, 0.0],
                },
                end_cond: [EndCondition::Pinned, EndCondition::Pinned],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            },
        ],
        sections: vec![sec],
        materials: vec![mat],
        load_cases: vec![LoadCase {
            id: LoadCaseId(1),
            name: "gravity".into(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -1.0e5, 0.0, 0.0, 0.0],
            )],
            member: vec![],
            kind: sepika_core::model::LoadCaseKind::Dead,
        }],
        stress_cfg: StressAnalysisCfg::default(),
        ..Default::default()
    }
}

fn axial_force(res: &StaticOnce, elem: ElemId) -> f64 {
    let (_, mf) = res
        .member_forces
        .iter()
        .find(|(id, _)| *id == elem)
        .unwrap_or_else(|| panic!("member forces for elem {:?} not found", elem));
    mf.at[0].1[0]
}

/// 検証1: `no_long_axial_brace=true` の長期ケースでは、ブレース軸力がほぼ0
/// （元の1e-3倍以下）になり、周囲の柱（柱A。ブレースが基部で直接取り付く側の
/// 柱で、荷重も直接負担する主経路）の軸力の絶対値が増えること
/// （一貫構造計算プログラムの実務慣行）。
#[test]
fn test_no_long_axial_brace_zeros_brace_force_and_increases_column() {
    let mut model = braced_frame(sepika_core::model::LoadCaseKind::Dead);
    let base = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let base_brace = axial_force(&base, ElemId(3)).abs();
    let base_col_a = axial_force(&base, ElemId(0)).abs();
    assert!(
        base_brace > 1.0,
        "baseline brace force should be non-trivial: {base_brace}"
    );

    model.stress_cfg.no_long_axial_brace = true;
    let cut = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let cut_brace = axial_force(&cut, ElemId(3)).abs();
    let cut_col_a = axial_force(&cut, ElemId(0)).abs();

    assert!(
        cut_brace <= base_brace * 1e-3,
        "brace force should collapse to ~0: base={base_brace} cut={cut_brace}"
    );
    assert!(
            cut_col_a > base_col_a,
            "column A axial force should increase when brace unloaded: base={base_col_a} cut={cut_col_a}"
        );
}

/// 検証2: `no_long_axial_column=true` の長期ケースでは、柱の長期軸力が
/// ほぼ0（元の1e-3倍以下）になること。同一2節点間で柱と並列に鉛直
/// ブレースを置いたモデル（`column_with_parallel_vertical_brace`）を使い、
/// 柱が負担しなくなった軸力がブレース側で健全に負担される
/// （機構化せず数値的に安定に解ける）ことも合わせて確認する。
#[test]
fn test_no_long_axial_column_zeros_column_force() {
    let mut model = column_with_parallel_vertical_brace();
    let base = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let base_col = axial_force(&base, ElemId(0)).abs();
    let base_brace = axial_force(&base, ElemId(1)).abs();
    assert!(
        base_col > 1.0,
        "baseline column force should be non-trivial: {base_col}"
    );

    model.stress_cfg.no_long_axial_column = true;
    let cut = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let cut_col = axial_force(&cut, ElemId(0)).abs();
    let cut_brace = axial_force(&cut, ElemId(1)).abs();

    assert!(
        cut_col <= base_col * 1e-3,
        "column force should collapse to ~0: base={base_col} cut={cut_col}"
    );
    // 柱が負担しなくなった軸力は並列ブレースへ移り、荷重全体はブレース単体で
    // 健全に負担される（機構化していないことの確認）。
    assert!(
            cut_brace > base_brace,
            "brace should pick up the load the column no longer carries: base={base_brace} cut={cut_brace}"
        );
    assert!(
        (cut_brace - 1.0e5).abs() / 1.0e5 < 1e-3,
        "brace should carry ~all of the applied load once column axial is disabled: {cut_brace}"
    );
}

/// 検証3: 短期（Seismic）荷重ケースでは、`no_long_axial_brace=true` でも
/// 適用されない（ブレース軸力が長期無効化なしの基準値と同程度に残ること）。
#[test]
fn test_axial_cut_not_applied_to_short_term_case() {
    let mut model = braced_frame(sepika_core::model::LoadCaseKind::Dead);
    // 同一の鉛直荷重を持つ短期（地震）荷重ケースを追加する。
    model.load_cases.push(LoadCase {
        id: LoadCaseId(2),
        name: "seismic_gravity_dummy".into(),
        nodal: vec![NodalLoad::manual(
            NodeId(2),
            [0.0, 0.0, -1.0e5, 0.0, 0.0, 0.0],
        )],
        member: vec![],
        kind: sepika_core::model::LoadCaseKind::Seismic,
    });
    model.stress_cfg.no_long_axial_brace = true;

    let long_term = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let short_term = linear_static_once(&model, LoadCaseId(2)).unwrap();

    let brace_long = axial_force(&long_term, ElemId(3)).abs();
    let brace_short = axial_force(&short_term, ElemId(3)).abs();

    assert!(
        brace_long < 1.0,
        "長期ケースはブレース軸力が無効化されるはず: {brace_long}"
    );
    assert!(
        brace_short > 1.0,
        "短期ケースは無効化されず通常どおりブレース軸力を負担するはず: {brace_short}"
    );
}

/// SRC柱の軸剛性を低減しても、曲げ・せん断・ねじり剛性と質量は保持する。
#[test]
fn test_axial_cut_applies_to_composite_src_column() {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};

    let mut model = column_with_parallel_vertical_brace();
    // 柱断面を SRC（shape あり・コンクリート材料 fc あり）へ差し替える。
    model.sections[0].shape = Some(SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![8],
            y: vec![8],
            cover: 50.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 12.0,
    });
    model.materials[0].fc = Some(24.0);
    model.materials[0].density = 2.4e-9;
    model.materials[0].young = 2.27e4;

    let elem = &model.elements[0];
    let normal = build_behavior_with_axial_factor(elem, &model, 1.0);
    let reduced = build_behavior_with_axial_factor(elem, &model, AXIAL_DISABLE_FACTOR);
    let ctx = Ctx { model: &model };
    let normal_k = normal.tangent_stiffness(&ctx);
    let reduced_k = reduced.tangent_stiffness(&ctx);
    for row in 0..12 {
        for col in 0..12 {
            let factor = if [2, 8].contains(&row) && [2, 8].contains(&col) {
                AXIAL_DISABLE_FACTOR
            } else {
                1.0
            };
            let expected = normal_k.get(row, col) * factor;
            assert!(
                (reduced_k.get(row, col) - expected).abs() <= expected.abs().max(1.0) * 1e-12,
                "剛性成分 ({row}, {col})"
            );
        }
    }
    for mass in [
        sepika_element::behavior::MassOption::Lumped,
        sepika_element::behavior::MassOption::Consistent,
    ] {
        assert_eq!(
            normal.mass_matrix(mass).data,
            reduced.mass_matrix(mass).data
        );
    }

    let base = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let base_col = axial_force(&base, ElemId(0)).abs();
    assert!(
        base_col > 1.0,
        "SRC柱の基準軸力が有意であること: {base_col}"
    );

    model.stress_cfg.no_long_axial_column = true;
    let cut = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let cut_col = axial_force(&cut, ElemId(0)).abs();
    let cut_brace = axial_force(&cut, ElemId(1)).abs();

    assert!(
        cut_col <= base_col * 1e-3,
        "SRC柱でも軸力が無効化されるはず: base={base_col} cut={cut_col}"
    );
    assert!(
        (cut_brace - 1.0e5).abs() / 1.0e5 < 1e-3,
        "荷重はブレースが全量負担するはず: {cut_brace}"
    );
}

// 引張専用ブレースの active-set 反復（真の引張専用解析）

/// 引張専用ブレース検証用の1スパン門型フレーム。
///
/// - N0[0,0,0]・N1[L,0,0]: 基部固定
/// - N2[0,0,H]・N3[L,0,H]: 頂部自由
/// - e0/e1: 柱（鉛直 Beam, Fixed-Fixed）… ブレース無効化時の水平抵抗（曲げ）経路
/// - e2: 頂部大梁（Beam）
/// - e3: 対角ブレース N0→N3（`tension_only` は引数指定）
///
/// 頂部2節点に水平力 `fx`（+x）を与える。ブレース軸 t=(L,0,H)/len に対し、
/// +x のスウェイで N3 が N0 から離れる → 軸伸び δ>0（引張）。-x なら δ<0（圧縮）。
fn tension_only_portal(fx: f64, tension_only: bool) -> Model {
    let l = 4000.0_f64;
    let h = 3000.0_f64;
    let node = |id: u32, coord: [f64; 3], fixed: bool| Node {
        id: NodeId(id),
        coord,
        restraint: if fixed {
            Dof6Mask::FIXED
        } else {
            Dof6Mask::FREE
        },
        mass: None,
        story: None,
        support_spring: None,
    };
    let sec = Section {
        frame_use: Some(FrameSectionUse::Girder),
        id: SectionId(0),
        name: "steel".into(),
        area: 6000.0,
        iy: 8.0e7,
        iz: 8.0e7,
        j: 1.0e6,
        depth: 300.0,
        width: 300.0,
        as_y: 5000.0,
        as_z: 5000.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: None,
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    };
    let mat = Material {
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
    };
    let column = |id: u32, n0: u32, n1: u32| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
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
    };
    Model {
        nodes: vec![
            node(0, [0.0, 0.0, 0.0], true),
            node(1, [l, 0.0, 0.0], true),
            node(2, [0.0, 0.0, h], false),
            node(3, [l, 0.0, h], false),
        ],
        elements: vec![
            column(0, 0, 2),
            column(1, 1, 3),
            // e2: 頂部大梁
            ElementData {
                id: ElemId(2),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(2), NodeId(3)],
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
            },
            // e3: 対角ブレース N0→N3
            ElementData {
                id: ElemId(3),
                kind: ElementKind::Brace { tension_only },
                nodes: smallvec::smallvec![NodeId(0), NodeId(3)],
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
            },
        ],
        sections: vec![sec],
        materials: vec![mat],
        load_cases: vec![LoadCase {
            id: LoadCaseId(1),
            name: "wind".into(),
            nodal: vec![
                NodalLoad::manual(NodeId(2), [fx, 0.0, 0.0, 0.0, 0.0, 0.0]),
                NodalLoad::manual(NodeId(3), [fx, 0.0, 0.0, 0.0, 0.0, 0.0]),
            ],
            member: vec![],
            kind: sepika_core::model::LoadCaseKind::Seismic,
        }],
        stress_cfg: sepika_core::model::StressAnalysisCfg::default(),
        ..Default::default()
    }
}

/// 引張側の荷重（+x スウェイ）では、反復 ON の引張専用ブレースが全剛性の
/// 一般ブレース（`tension_only: false`）と厳密に一致する軸力を負担すること。
/// active なブレースは E·A/L の一般ブレースそのものになる、という等価性の検証。
#[test]
fn test_tension_only_iteration_tension_side_matches_full_brace() {
    // 参照: 一般（全剛性）ブレース
    let full = tension_only_portal(1.0e4, false);
    let ref_res = linear_static_once(&full, LoadCaseId(1)).unwrap();
    let ref_brace = axial_force(&ref_res, ElemId(3));
    assert!(
        ref_brace.abs() > 1.0,
        "参照ブレース軸力が有意であること: {ref_brace}"
    );

    // 引張専用 + 反復 ON
    let mut model = tension_only_portal(1.0e4, true);
    model.stress_cfg.tension_only_iteration = true;
    let res = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let brace = axial_force(&res, ElemId(3));

    assert!(
        (brace - ref_brace).abs() / ref_brace.abs() < 1e-6,
        "引張側では全剛性ブレースと一致するはず: full={ref_brace} to={brace}"
    );
}

/// 圧縮側の荷重（−x スウェイ）では、反復 ON の引張専用ブレースが無効化され
/// 軸力がほぼ0になること。一方、反復 OFF（一括解析）では全剛性 E·A/L のまま
/// 圧縮の軸力を負担すること。
#[test]
fn test_tension_only_iteration_compression_side_is_slack() {
    // 反復 OFF（既定の一括解析）: 全剛性 E·A/L で圧縮を負担する
    let base = tension_only_portal(-1.0e4, true);
    assert!(!base.stress_cfg.tension_only_iteration);
    let base_res = linear_static_once(&base, LoadCaseId(1)).unwrap();
    let base_brace = axial_force(&base_res, ElemId(3));
    assert!(
        base_brace.abs() > 1.0,
        "反復 OFF では圧縮軸力が有意であること: {base_brace}"
    );
    assert!(base_brace < 0.0, "圧縮（負）であること: {base_brace}");

    // 反復 ON: 圧縮ブレースは無効化され軸力ほぼ0
    let mut model = tension_only_portal(-1.0e4, true);
    model.stress_cfg.tension_only_iteration = true;
    let res = linear_static_once(&model, LoadCaseId(1)).unwrap();
    let brace = axial_force(&res, ElemId(3));

    assert!(
        brace.abs() <= base_brace.abs() * 1e-3,
        "圧縮側では軸力が0へ落ちるはず: base={base_brace} to={brace}"
    );
}

/// 剛床に載る梁を持つ 1 層門型ラーメン（柱 ElemId 0,1／梁 ElemId 2）。
/// 梁の全長に等分布荷重 w=10 N/mm（鉛直下向き）をかける。
///
/// `with_rigid_floor` が true のとき、階（`Story`）に剛床を定義する。
/// 自由度の拘束（`constraints`）は付けないため解は変わらないはずで、
/// 剛床の有無で応力解析の結果が変わってはいけないことの検証に使う
/// （`ForceRegime::Auto` の剛床判定が線形解析の要素種別へ漏れない）。
fn rigid_floor_portal(with_rigid_floor: bool) -> Model {
    use sepika_core::model::Story;
    let l = 6000.0_f64;
    let h = 3500.0_f64;
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
                coord: [l, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0, 0.0, h],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(3),
                coord: [l, 0.0, h],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![],
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: "s".into(),
            area: 1.0e4,
            iy: 1.0e8,
            iz: 1.0e8,
            j: 1.0e6,
            depth: 400.0,
            width: 200.0,
            as_y: 4.0e3,
            as_z: 4.0e3,
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
            name: "SN400B".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "udl".into(),
            nodal: vec![],
            member: vec![MemberLoad::manual(
                ElemId(2),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: l,
                    w1: 10.0,
                    w2: 10.0,
                },
            )],
        }],
        ..Default::default()
    };
    let mut push = |id: u32, i: u32, j: u32, ref_vector: [f64; 3]| {
        model.elements.push(ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(i), NodeId(j)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis { ref_vector },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
    };
    push(0, 0, 2, [1.0, 0.0, 0.0]);
    push(1, 1, 3, [1.0, 0.0, 0.0]);
    push(2, 2, 3, [0.0, 0.0, 1.0]);

    if with_rigid_floor {
        // 梁の材端節点が剛床上にあることだけを表す剛床（スレーブなし）。
        // 自由度は何も拘束しないので、線形解析の結果は剛床の有無で変わらないはず
        // であり、要素種別が剛床判定に引きずられていないかを切り分けられる。
        model
            .constraints
            .push(sepika_core::model::Constraint::rigid_diaphragm(
                StoryId(0),
                NodeId(2),
                Vec::new(),
            ));
        model.stories.push(Story {
            wall_weights: Vec::new(),
            level_kind: Default::default(),
            structure: Default::default(),
            id: StoryId(0),
            name: "2F".into(),
            elevation: Some(h),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(2), NodeId(3)],
            seismic_weight: None,
            weight_override: None,
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        });
    }
    model
}

///
/// `ForceRegime::Auto` は「剛床に載る水平材」を材端集中ばね（非線形要素）へ
/// 振り分けるが、これは非線形解析だけの規則である。線形解析の要素生成が
/// この振り分けに従うと、材端集中ばね梁が `recover_forces` を持たないため
/// 当該梁が `member_forces` から丸ごと欠落し、応力図・検定比図に
/// 何も表示されなくなる（RC 基礎梁など剛床に載らない梁だけが残る）。
#[test]
fn test_rigid_floor_beam_member_forces_are_recovered() {
    let model = rigid_floor_portal(true);
    let res = linear_static_once(&model, LoadCaseId(1)).unwrap();

    for id in [ElemId(0), ElemId(1), ElemId(2)] {
        assert!(
            res.member_forces.iter().any(|(e, _)| *e == id),
            "部材 {:?} の内力が回収されていない: 回収済み={:?}",
            id,
            res.member_forces
                .iter()
                .map(|(e, _)| e.0)
                .collect::<Vec<_>>()
        );
    }

    // 梁の曲げは両端固定に近い分布（中央 sagging・端部 hogging）で、
    // 単純梁の中央曲げ wL²/8 を超えない有意な値になる。
    let beam = res
        .member_forces
        .iter()
        .find(|(e, _)| *e == ElemId(2))
        .map(|(_, mf)| mf)
        .expect("梁の内力");
    let m_mid = beam
        .at
        .iter()
        .find(|(xi, _)| (xi - 0.5).abs() < 1e-9)
        .map(|(_, f)| f[5])
        .expect("中央の内力");
    let w = 10.0_f64;
    let l = 6000.0_f64;
    let m_simple = w * l * l / 8.0;
    assert!(
        m_mid.abs() > m_simple * 0.1 && m_mid.abs() < m_simple,
        "梁中央の曲げが妥当な範囲にない: M={m_mid} (wL²/8={m_simple})"
    );
}

/// 自由度を拘束しない剛床（スレーブなしの `Constraint::RigidDiaphragm`）を加えても、
/// 線形解析の結果が一切変わらないこと。要素種別が `ForceRegime` の剛床判定に依存すると、
/// 材端ばねが直列に入って梁の曲げ剛性が落ち、応力・変形が変わってしまう。
#[test]
fn test_rigid_floor_definition_does_not_change_linear_result() {
    let without = linear_static_once(&rigid_floor_portal(false), LoadCaseId(1)).unwrap();
    let with = linear_static_once(&rigid_floor_portal(true), LoadCaseId(1)).unwrap();

    assert_eq!(without.member_forces.len(), with.member_forces.len());
    for ((id_a, mf_a), (id_b, mf_b)) in without.member_forces.iter().zip(&with.member_forces) {
        assert_eq!(id_a, id_b);
        assert_eq!(mf_a.at.len(), mf_b.at.len());
        for ((xi_a, fa), (xi_b, fb)) in mf_a.at.iter().zip(&mf_b.at) {
            assert!((xi_a - xi_b).abs() < 1e-12);
            for k in 0..6 {
                assert!(
                    (fa[k] - fb[k]).abs() <= fa[k].abs() * 1e-12 + 1e-6,
                    "部材 {:?} xi={xi_a} 成分{k} が剛床定義の有無で変化: {} vs {}",
                    id_a,
                    fa[k],
                    fb[k]
                );
            }
        }
    }
    for (ua, ub) in without.disp.iter().zip(&with.disp) {
        for k in 0..6 {
            assert!(
                (ua[k] - ub[k]).abs() <= ua[k].abs() * 1e-12 + 1e-12,
                "節点変位が剛床定義の有無で変化: {} vs {}",
                ua[k],
                ub[k]
            );
        }
    }
}

/// 線材の部材内力が回収できないモデルは、黙って結果から欠落させず解析エラーにする
/// （[`ensure_line_member_forces`]）。要素実装が `recover_forces` を持たないと、
/// 応力図・断面検定・接合部検定の入力から当該部材が無言で消えるため。
#[test]
fn test_ensure_line_member_forces_detects_missing() {
    let model = rigid_floor_portal(true);
    let res = linear_static_once(&model, LoadCaseId(1)).unwrap();

    // 全線材が揃っていれば OK。
    assert!(ensure_line_member_forces(&model, &res.member_forces).is_ok());

    // 梁 ElemId(2) の内力が欠落した状態を模擬するとエラーになり、
    // メッセージに欠落した部材 ID が含まれる。
    let missing: Vec<_> = res
        .member_forces
        .iter()
        .filter(|(id, _)| *id != ElemId(2))
        .cloned()
        .collect();
    let err = ensure_line_member_forces(&model, &missing)
        .expect_err("欠落を検出できていない")
        .to_string();
    assert!(err.contains('2'), "欠落した部材 ID が示されていない: {err}");
}

fn scalar_axial_model(kind: ElementKind, column: bool) -> Model {
    let mut model = column_with_parallel_vertical_brace();
    model.elements.truncate(1);
    model.elements[0].kind = kind;
    model.nodes[1].coord[2] = 1000.0;
    model.nodes[1].restraint = Dof6Mask(0b111011);
    model.nodes[1].support_spring = Some([0.0, 0.0, 200000.0, 0.0, 0.0, 0.0]);
    model.nodes[1].mass = Some([1.0; 6]);
    model.sections[0].area = 1000.0;
    model.sections[0].frame_use = Some(if column {
        FrameSectionUse::Column
    } else {
        FrameSectionUse::Girder
    });
    model.materials[0].young = 200000.0;
    model.load_cases[0].nodal[0].values[2] = -1000.0;
    if matches!(kind, ElementKind::Brace { .. }) {
        model.sections[0].frame_use = Some(FrameSectionUse::Brace);
    } else if !column {
        model.nodes[1].coord = [1000.0, 0.0, 0.0];
        model.nodes[1].restraint = Dof6Mask(0b111110);
        model.nodes[1].support_spring = Some([200000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.load_cases[0].nodal[0].values = [-1000.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }
    model
}

fn assert_scalar_response(result: &StaticOnce, force: f64, factor: f64) {
    let expected_u = force / (200000.0 + factor * 200000.0);
    let expected_n = factor * 200000.0 * expected_u;
    assert!((result.disp[1][..3].iter().sum::<f64>() / expected_u - 1.0).abs() < 1e-10);
    assert!(
        (axial_force(result, ElemId(0)) / expected_n - 1.0).abs() < 1e-10,
        "N={} expected {}",
        axial_force(result, ElemId(0)),
        expected_n
    );
}

#[test]
fn static_public_paths_adopt_all_column_kinds_and_load_classifications() {
    use crate::statics::analysis::Analysis;
    use sepika_core::model::LoadCaseKind::*;
    for elem_kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for column in [true, false] {
            for enabled in [false, true] {
                let mut model = scalar_axial_model(elem_kind, column);
                model.stress_cfg.no_long_axial_column = enabled;
                for (i, kind) in [Dead, Live, Snow, Other, LiveSeismic, Wind, Seismic]
                    .into_iter()
                    .enumerate()
                {
                    let mut case = model.load_cases[0].clone();
                    case.id = LoadCaseId(i as u32 + 1);
                    case.kind = kind;
                    case.name = format!("{kind:?}");
                    if i == 0 {
                        model.load_cases.clear();
                    }
                    model.load_cases.push(case);
                }
                let analysis = Analysis::prepare(&model).unwrap();
                let order = [
                    LoadCaseId(1),
                    LoadCaseId(7),
                    LoadCaseId(1),
                    LoadCaseId(2),
                    LoadCaseId(3),
                    LoadCaseId(4),
                    LoadCaseId(5),
                    LoadCaseId(6),
                ];
                let batch = analysis.linear_static_batch(&order);
                for (id, batched) in order.iter().zip(batch) {
                    let kind = model
                        .load_cases
                        .iter()
                        .find(|case| case.id == *id)
                        .unwrap()
                        .kind;
                    let factor = if enabled && column && matches!(kind, Dead | Live | Snow | Other)
                    {
                        1e-6
                    } else {
                        1.0
                    };
                    for result in [
                        linear_static_once(&model, *id).unwrap(),
                        analysis.linear_static(*id).unwrap(),
                        batched.unwrap(),
                    ] {
                        assert_scalar_response(&result, -1000.0, factor);
                    }
                }
            }
        }
    }
}

#[test]
fn brace_settings_product_order_and_case_response_superposition() {
    use crate::statics::analysis::Analysis;
    use sepika_core::model::{LoadCaseKind, LoadCombination};
    for long_disabled in [false, true] {
        for tension_iteration in [false, true] {
            for tension_only in [false, true] {
                let mut model = scalar_axial_model(ElementKind::Brace { tension_only }, false);
                model.stress_cfg.no_long_axial_brace = long_disabled;
                model.stress_cfg.tension_only_iteration = tension_iteration;
                for (id, kind, force) in [
                    (1, LoadCaseKind::Dead, -1000.0),
                    (2, LoadCaseKind::Dead, 1000.0),
                    (3, LoadCaseKind::Seismic, -1000.0),
                ] {
                    let mut case = model.load_cases[0].clone();
                    case.id = LoadCaseId(id);
                    case.kind = kind;
                    case.nodal[0].values[2] = force;
                    if id == 1 {
                        model.load_cases.clear();
                    }
                    model.load_cases.push(case);
                }
                let analysis = Analysis::prepare(&model).unwrap();
                for id in [1, 2, 3, 1, 2, 1] {
                    let force = if id == 2 { 1000.0 } else { -1000.0 };
                    let beta = if long_disabled && id != 3 { 1e-6 } else { 1.0 };
                    let active = if tension_iteration && tension_only && force < 0.0 {
                        1e-6
                    } else {
                        1.0
                    };
                    for result in [
                        linear_static_once(&model, LoadCaseId(id)).unwrap(),
                        analysis.linear_static(LoadCaseId(id)).unwrap(),
                    ] {
                        assert_scalar_response(&result, force, beta * active);
                    }
                }
                let combo = LoadCombination {
                    name: "compression+tension".into(),
                    terms: vec![(LoadCaseId(1), 1.0), (LoadCaseId(2), 1.0)],
                };
                let result = analysis.linear_combination(&combo).unwrap();
                let beta = if long_disabled { 1e-6 } else { 1.0 };
                let compression = if tension_iteration && tension_only {
                    1e-6
                } else {
                    1.0
                };
                let expected_u = -1000.0 / (200000.0 + beta * compression * 200000.0)
                    + 1000.0 / (200000.0 + beta * 200000.0);
                let expected_n = beta
                    * compression
                    * 200000.0
                    * (-1000.0 / (200000.0 + beta * compression * 200000.0))
                    + beta * 200000.0 * (1000.0 / (200000.0 + beta * 200000.0));
                if tension_iteration && tension_only {
                    assert!((result.disp[1][2] / expected_u - 1.0).abs() < 1e-9);
                    assert!((axial_force(&result, ElemId(0)) / expected_n - 1.0).abs() < 1e-9);
                } else {
                    assert_eq!(result.disp[1][2], 0.0);
                    assert_eq!(axial_force(&result, ElemId(0)), 0.0);
                }
            }
        }
    }
}

#[test]
fn active_set_requires_unchanged_set_and_reports_case_and_limit() {
    for (force, limit, success) in [
        (-1000.0, 1, false),
        (-1000.0, 2, true),
        (1000.0, 1, true),
        (0.0, 1, true),
    ] {
        let mut model = scalar_axial_model(ElementKind::Brace { tension_only: true }, false);
        model.stress_cfg.tension_only_iteration = true;
        model.stress_cfg.no_long_axial_brace = true;
        model.load_cases[0].name = "scalar-limit".into();
        model.load_cases[0].nodal[0].values[2] = force;
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let result =
            solve_static_case(&model, &dofmap, &reducer, &model.load_cases[0], None, limit);
        if success {
            let result = result.unwrap();
            if force != 0.0 {
                assert_scalar_response(&result, force, if force < 0.0 { 1e-12 } else { 1e-6 });
            } else {
                assert_eq!(result.disp[1][2], 0.0);
            }
        } else {
            assert!(
                matches!(result, Err(SolveError::NonConvergence(message)) if message.contains("scalar-limit") && message.contains("ID 1") && message.contains("上限 1"))
            );
        }
    }
}

#[test]
fn static_adoption_does_not_change_baseline_eigenvalues() {
    use crate::statics::analysis::Analysis;
    let mut model = scalar_axial_model(ElementKind::Brace { tension_only: true }, false);
    let expected = Analysis::prepare(&model).unwrap().eigen(1).unwrap().omega2;
    model.stress_cfg.no_long_axial_brace = true;
    model.stress_cfg.tension_only_iteration = true;
    let analysis = Analysis::prepare(&model).unwrap();
    analysis.linear_static(LoadCaseId(1)).unwrap();
    assert_eq!(analysis.eigen(1).unwrap().omega2, expected);
}

#[test]
fn lip_channel_geometry_public_linear_result_rejects_beam_fiber_ms_without_panic() {
    use sepika_core::section_shape::SectionShape;
    let mut failures = vec![];
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for lip in [75.0, 80.0, 100.0] {
            for fully_fixed in [false, true] {
                let mut model = ss_beam(3000.0, vec![]);
                model.elements[0].kind = kind;
                model.sections[0].name = "LC544".into();
                model.sections[0].shape = Some(SectionShape::SteelLipChannel {
                    height: 150.0,
                    width: 75.0,
                    lip,
                    thick: 2.3,
                });
                if fully_fixed {
                    for node in &mut model.nodes {
                        node.restraint = Dof6Mask::FIXED;
                    }
                }
                let before = format!("{model:?}");
                let reason = model.validate_lip_channels().unwrap_err().to_string();
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    linear_static_once(&model, LoadCaseId(1))
                })) {
                    Ok(Err(SolveError::InvalidInput(error))) => {
                        assert!(error.contains(&reason), "{error}")
                    }
                    Ok(Err(error)) => {
                        failures.push(format!("{kind:?}/C{lip}/fixed{fully_fixed}: {error}"))
                    }
                    Ok(Ok(_)) => failures.push(format!(
                        "{kind:?}/C{lip}/fixed{fully_fixed}: 不成立形状で成功"
                    )),
                    Err(_) => failures.push(format!(
                        "{kind:?}/C{lip}/fixed{fully_fixed}: Result入口でpanic"
                    )),
                }
                assert_eq!(format!("{model:?}"), before);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn invalid_young_modulus_reaches_both_public_static_entries_as_same_reason() {
    let mut model = ss_beam(1000.0, vec![]);
    model.materials[0].young = 0.0;
    let direct = linear_static_once(&model, model.load_cases[0].id)
        .unwrap_err()
        .to_string();
    let cached = match crate::statics::analysis::Analysis::prepare(&model) {
        Ok(_) => panic!("不正剛性は解析前に拒否する"),
        Err(error) => error.to_string(),
    };
    assert!(
        direct.contains("部材 ID 0") && direct.contains("ヤング係数 E は正の有限値が必要"),
        "{direct}"
    );
    assert!(
        cached.contains("ヤング係数 E は正の有限値が必要"),
        "{cached}"
    );
}

#[test]
fn invalid_beam_shear_modulus_is_rejected_by_public_static_entries_and_constructor() {
    use sepika_element::frame::beam::BeamElement;
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for (shear, poisson) in [
            (Some(f64::NAN), 0.3),
            (Some(f64::INFINITY), 0.3),
            (Some(f64::NEG_INFINITY), 0.3),
            (Some(-1.0), 0.3),
            (Some(0.0), 0.3),
            (None, -1.0),
            (None, -2.0),
            (None, f64::NAN),
            (None, f64::INFINITY),
        ] {
            let mut model = ss_beam(1000.0, vec![]);
            model.elements[0].kind = kind;
            model.materials[0].shear = shear;
            model.materials[0].poisson = poisson;
            let error = match BeamElement::try_new(&model.elements[0], &model) {
                Ok(_) => panic!("{kind:?}: G={shear:?}, nu={poisson} が受理された"),
                Err(error) => error,
            };
            assert!(
                error.contains("部材 ID 0") && error.contains("せん断弾性係数 G"),
                "{error}"
            );
            let direct = linear_static_once(&model, model.load_cases[0].id)
                .unwrap_err()
                .to_string();
            let prepared = match crate::statics::analysis::Analysis::prepare(&model) {
                Ok(_) => panic!("不正Gは解析前に拒否する"),
                Err(error) => error.to_string(),
            };
            assert!(direct.contains(&error), "{direct}");
            assert!(prepared.contains(&error), "{prepared}");
        }
        for (shear, poisson) in [(None, 0.3), (Some(80000.0), -1.0)] {
            let mut model = ss_beam(1000.0, vec![]);
            model.elements[0].kind = kind;
            model.materials[0].shear = shear;
            model.materials[0].poisson = poisson;
            assert!(BeamElement::try_new(&model.elements[0], &model).is_ok());
            assert!(linear_static_once(&model, model.load_cases[0].id).is_ok());
            assert!(crate::statics::analysis::Analysis::prepare(&model).is_ok());
        }
    }
}

#[test]
fn eccentric_member_load_reactions_keep_transfer_moment_after_end_releases_and_rigid_zones() {
    for e in [-100.0, 0.0, 100.0] {
        for released in [[false, false], [true, false], [true, true]] {
            for rigid in [0.0, 100.0] {
                for reversed in [false, true] {
                    let mut model = owned_udl_model();
                    model.elements[0].vertical_eccentricity = Some(e);
                    model.elements[0].end_cond = released.map(|v| {
                        if v {
                            EndCondition::Pinned
                        } else {
                            EndCondition::Fixed
                        }
                    });
                    model.elements[0].rigid_zone.length_i = rigid;
                    model.elements[0].rigid_zone.length_j = rigid;
                    if reversed {
                        model.elements[0].nodes.reverse();
                    }
                    model.load_cases[0].member[0] = MemberLoad::manual(
                        ElemId(0),
                        [1.0, 0.0, 0.0],
                        MemberLoadKind::Distributed {
                            a: 0.0,
                            b: 1000.0,
                            w1: 2.0,
                            w2: 2.0,
                        },
                    );
                    let map = DofMap::build(&model);
                    let reducer = Reducer::build(&model, &map);
                    let stiffness =
                        StaticStiffness::build(&model, &map, &reducer, model.load_cases[0].kind)
                            .unwrap();
                    let entries: Vec<_> = stiffness
                        .behaviors
                        .iter()
                        .map(|(id, b, g)| (*id, b.as_ref(), g.as_slice()))
                        .collect();
                    let ledger = PhysicalLoadLedger::new(&model, &model.load_cases[0]).unwrap();
                    let (result, reactions) =
                        recover_static(&model, &map, &[], &ledger, &entries).unwrap();
                    for reaction in reactions {
                        assert!((reaction[0] + 1000.0).abs() < 1e-7, "{reaction:?}");
                        assert!((reaction[4] + 1000.0 * e).abs() < 1e-7, "{reaction:?}");
                    }
                    for (_, force) in result.member_forces[0].1.at.iter() {
                        assert!(force[4].abs() < 1e-7 && force[5].abs() < 1e-7);
                    }
                }
            }
        }
    }
}

#[test]
fn building_rigid_zone_switch_preserves_inputs_and_changes_stiffness_and_cmq() {
    use sepika_element::frame::member_load::cmq_with_rigid_zone;
    let mut model = make_axial_cantilever();
    model.sections[0].iy = 1e7;
    model.sections[0].iz = 1e7;
    model.sections[0].as_y = 1e10;
    model.sections[0].as_z = 1e10;
    model.elements[0].rigid_zone = sepika_core::model::RigidZone {
        length_i: 200.0,
        length_j: 100.0,
        source_i: sepika_core::model::ZoneSource::Manual,
        source_j: sepika_core::model::ZoneSource::Manual,
        face_i: Some(250.0),
        face_j: Some(150.0),
        ..Default::default()
    };
    let saved = model.elements[0].rigid_zone;
    model.load_cases[0].nodal[0].values = [0.0, 0.0, -1000.0, 0.0, 0.0, 0.0];
    let on = linear_static_once(&model, LoadCaseId(1)).unwrap();
    model.stress_cfg.consider_rigid_zones = false;
    let off = linear_static_once(&model, LoadCaseId(1)).unwrap();
    assert!(off.disp[1][2].abs() > on.disp[1][2].abs());
    assert_eq!(model.elements[0].rigid_zone, saved);
    let mut baseline = model.clone();
    baseline.elements[0].rigid_zone.length_i = 0.0;
    baseline.elements[0].rigid_zone.length_j = 0.0;
    baseline.stress_cfg.consider_rigid_zones = true;
    let no_zone = linear_static_once(&baseline, LoadCaseId(1)).unwrap();
    assert_eq!(off.disp, no_zone.disp);
    let loads = [MemberLoad::manual(
        ElemId(0),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: 2.0,
            w2: 2.0,
        },
    )];
    for (enabled, expected) in [
        (true, [1100.0, 900.0, 785000.0 / 3.0, -485000.0 / 3.0]),
        (false, [1000.0, 1000.0, 500000.0 / 3.0, -500000.0 / 3.0]),
    ] {
        model.stress_cfg.consider_rigid_zones = enabled;
        let response = cmq_with_rigid_zone(&model, &model.elements[0], &loads).unwrap();
        let f = response.physical_frame_end_force_global;
        for (actual, expected) in [f[2], f[8], -f[4], -f[10]].into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
        }
        let mi = response.section.at(0.0).unwrap()[5];
        let mj = response.section.at(1.0).unwrap()[5];
        assert!((mi + expected[2]).abs() < 1e-6);
        assert!((mj - expected[3]).abs() < 1e-6);
    }
    model.elements[0].rigid_zone.panel_offset_i = 75.0;
    model.stress_cfg.consider_rigid_zones = false;
    let zone = model.analysis_rigid_zone(&model.elements[0]);
    assert_eq!(zone.rigid_length_i(), 75.0);
    assert_eq!(zone.face_i, Some(250.0));
    assert_eq!(model.elements[0].rigid_zone.length_i, 200.0);
}

#[test]
fn building_rigid_zone_cmq_keeps_load_force_and_moment_for_all_shapes_and_releases() {
    use sepika_element::frame::member_load::cmq_with_rigid_zone;
    let mut model = make_axial_cantilever();
    model.elements[0].rigid_zone.length_i = 200.0;
    model.elements[0].rigid_zone.length_j = 100.0;
    let cases = [
        (
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 2.0,
                w2: 2.0,
            },
            2000.0,
            1000000.0,
        ),
        (
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 150.0,
                w1: 2.0,
                w2: 2.0,
            },
            300.0,
            22500.0,
        ),
        (
            MemberLoadKind::Distributed {
                a: 100.0,
                b: 800.0,
                w1: 1.0,
                w2: 3.0,
            },
            1400.0,
            2135000.0 / 3.0,
        ),
        (
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 0.0,
                w2: 3.0,
            },
            1500.0,
            1000000.0,
        ),
    ];
    for enabled in [true, false] {
        model.stress_cfg.consider_rigid_zones = enabled;
        for (reverse, reference) in [
            (false, [0.0, 0.0, 1.0]),
            (true, [0.0, 0.0, 1.0]),
            (false, [0.0, 1.0, 1.0]),
        ] {
            model.elements[0].nodes = if reverse {
                smallvec::smallvec![NodeId(1), NodeId(0)]
            } else {
                smallvec::smallvec![NodeId(0), NodeId(1)]
            };
            model.elements[0].local_axis.ref_vector = reference;
            model.elements[0].rigid_zone.length_i = if reverse { 100.0 } else { 200.0 };
            model.elements[0].rigid_zone.length_j = if reverse { 200.0 } else { 100.0 };
            for release in [EndCondition::Fixed, EndCondition::Pinned] {
                model.elements[0].end_cond = [release, release];
                for (kind, resultant, moment) in cases.iter().cloned().chain(
                    [0.0, 100.0, 200.0, 500.0, 900.0, 950.0, 1000.0]
                        .map(|a| (MemberLoadKind::Point { a, p: 1000.0 }, 1000.0, a * 1000.0)),
                ) {
                    for sign in [-1.0, 1.0] {
                        let loads = [MemberLoad::manual(
                            ElemId(0),
                            [0.0, 0.0, -sign],
                            kind.clone(),
                        )];
                        let r = cmq_with_rigid_zone(&model, &model.elements[0], &loads).unwrap();
                        let f = r.physical_frame_end_force_global;
                        assert!((f[2] + f[8] - sign * resultant).abs() < 1e-6);
                        assert!(
                            (1000.0 * f[if reverse { 2 } else { 8 }]
                                - f[4]
                                - f[10]
                                - sign
                                    * if reverse {
                                        1000.0 * resultant - moment
                                    } else {
                                        moment
                                    })
                            .abs()
                                < 1e-6,
                            "enabled={enabled}, release={release:?}, load={kind:?}: {f:?}"
                        );
                        for xi in [0.0, 0.25, 0.5, 0.9, 1.0] {
                            assert!(r.section.at(xi).unwrap().iter().all(|v| v.is_finite()));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn building_rigid_zone_switch_reaches_incremental_and_timehistory_member_load_states() {
    use sepika_core::model::AnalysisKind;
    use sepika_element::behavior::{Ctx, LocalVec};
    use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};
    let mut input = make_axial_cantilever();
    input.materials[0].fy = Some(1e10);
    input.elements[0].rigid_zone.length_i = 200.0;
    input.elements[0].rigid_zone.length_j = 100.0;
    let loads = [MemberLoad::manual(
        ElemId(0),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: 2.0,
            w2: 2.0,
        },
    )];
    for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
        for regime in [
            ForceRegime::UniaxialBendingShear,
            ForceRegime::AxialBendingInteract,
        ] {
            let mut observed = vec![];
            for enabled in [true, false] {
                let mut model = input.clone();
                model.stress_cfg.consider_rigid_zones = enabled;
                model.elements[0].force_regime = regime;
                let basis = match kind {
                    AnalysisKind::Incremental => StrengthBasis::MaterialStrength,
                    AnalysisKind::TimeHistory => StrengthBasis::Nominal,
                };
                let mut behavior =
                    build_nonlinear_behavior(&model.elements[0], &model, basis, kind, None);
                let du = LocalVec {
                    data: vec![0.0; behavior.n_dof()].into(),
                };
                behavior
                    .update_state_with_member_loads(&du, &loads, true, &Ctx { model: &model })
                    .unwrap();
                let response = behavior.state_frame_load_response().unwrap().unwrap();
                let f = response.physical_frame_end_force_global;
                assert!((f[2] + f[8] - 2000.0).abs() < 1e-6);
                assert!((1000.0 * f[8] - f[4] - f[10] - 1000000.0).abs() < 1e-5);
                observed.push(response.section.at(0.0).unwrap()[5]);
                if !enabled {
                    let mut baseline = model.clone();
                    baseline.stress_cfg.consider_rigid_zones = true;
                    baseline.elements[0].rigid_zone.length_i = 0.0;
                    baseline.elements[0].rigid_zone.length_j = 0.0;
                    let mut base = build_nonlinear_behavior(
                        &baseline.elements[0],
                        &baseline,
                        basis,
                        kind,
                        None,
                    );
                    base.update_state_with_member_loads(
                        &du,
                        &loads,
                        true,
                        &Ctx { model: &baseline },
                    )
                    .unwrap();
                    let expected = base.state_frame_load_response().unwrap().unwrap();
                    for (actual, expected) in f.iter().zip(expected.physical_frame_end_force_global)
                    {
                        assert!((actual - expected).abs() < 1e-6);
                    }
                }
            }
            assert!(
                (observed[0] - observed[1]).abs() > 1.0,
                "{kind:?} {regime:?}"
            );
        }
    }
}
