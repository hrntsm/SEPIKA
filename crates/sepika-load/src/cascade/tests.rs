//! 二次部材の反力の逐次伝達のテスト。
//!
//! 床分配を混ぜると期待値の算定が分配則に依存してしまうため、ここでは面荷重を 0 にし、
//! **自重だけ**を載せて逐次伝達の骨格（支持関係の判定・順序・反力の分解・総和保存）を
//! 確かめる。床分配との結線は `sepika-job` 側の統合テストで見る。

use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LoadCfg, LocalAxis, Material,
    MaterialCategory, Node, Section,
};

/// 設計用単位体積重量 [N/mm³]（テストの期待値算定用。鋼材 78.5 kN/m³）。
const DESIGN_UNIT_WEIGHT_N_PER_MM3: f64 = 78.5e-6;
/// 物理質量密度 [t/mm³]（鋼）。
const DENSITY: f64 = 7.85e-9;
const AREA: f64 = 10_000.0; // mm²

/// 設計重量の等分布荷重 [N/mm]（設計単位体積重量 × 断面積）。
fn w_self() -> f64 {
    DESIGN_UNIT_WEIGHT_N_PER_MM3 * AREA
}

fn node(id: u32, x: f64, y: f64, z: f64) -> Node {
    Node {
        id: NodeId(id),
        coord: [x, y, z],
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    }
}

fn element_beam(id: u32, i: u32, j: u32) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        plastic_zone: None,
        spring: None,
    }
}

fn secondary_beam(model: &Model, a: u32, b: u32, name: &str) -> SecondaryMember {
    SecondaryMember {
        gravity_end_shares: None,
        id: sepika_core::ids::SecondaryMemberId(a),
        kind: SecondaryMemberKind::Beam,
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            model.nodes[a as usize].coord,
            model.nodes[b as usize].coord,
        ]),
        section: Some(SectionId(0)),
        name: name.to_string(),
    }
}

/// 鋼の材料と断面（自重が出る最小構成）を持つ空モデル。
fn base_model() -> Model {
    let mut m = Model::default();
    m.materials.push(Material {
        id: MaterialId(0),
        name: "SN400".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: DENSITY,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    m.sections.push(Section {
        frame_use: None,
        id: SectionId(0),
        name: "H".into(),
        floor: None,
        area: AREA,
        iy: 1.0e8,
        iz: 1.0e7,
        j: 1.0e6,
        depth: 400.0,
        width: 200.0,
        as_y: 0.0,
        as_z: 0.0,
        panel_thickness: None,
        thickness: None,
        shape: Some(sepika_core::section_shape::SectionShape::SteelFlatBar {
            width: 200.0,
            thick: 400.0,
        }),
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    });
    m
}

fn solved(model: &mut Model) -> SecondaryTransfer {
    // 鉄骨重量割増は `load_cfg` 未設定なら 1.0（`beam_self_weight_udl`）。
    model.anchorize_secondary_members();
    solve(model, |_| 0.0, true).unwrap()
}

fn face_model(span: f64, widths: [f64; 2]) -> Model {
    let mut model = base_model();
    model.sections[0].width = widths[0];
    model.sections[0].shape = Some(sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: widths[0],
        thick: 400.0,
    });
    let mut end = model.sections[0].clone();
    end.id = SectionId(1);
    end.width = widths[1];
    end.shape = Some(sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: widths[1],
        thick: 400.0,
    });
    model.sections.push(end);
    model.nodes = vec![
        node(0, -1000.0, 0.0, 0.0),
        node(1, 1000.0, 0.0, 0.0),
        node(2, -1000.0, span, 0.0),
        node(3, 1000.0, span, 0.0),
    ];
    model.elements = vec![element_beam(0, 0, 1), element_beam(1, 2, 3)];
    model.elements[1].section = Some(SectionId(1));
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(10),
        section: Some(SectionId(0)),
        ends: sepika_core::model::SecondaryMemberEnds::Supported([
            sepika_core::model::SecondaryMemberAnchor {
                support: sepika_core::model::SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            sepika_core::model::SecondaryMemberAnchor {
                support: sepika_core::model::SupportMemberId::Primary(ElemId(1)),
                position: 0.5,
            },
        ]),
        ..Default::default()
    });
    model
}

#[test]
fn rcとs小梁の支持幅400は内法3600に元の線荷重を載せる() {
    for concrete in [false, true] {
        let mut model = face_model(4000.0, [400.0; 2]);
        if concrete {
            model.materials[0].category = MaterialCategory::Concrete;
            model.materials[0].fc = Some(24.0);
            model.materials[0].density = 2.4e-9;
        }
        let transfer = solve(&model, |_| 0.0, true).unwrap();
        let member = &transfer.members[&SecondaryMemberId(10)];
        let w = beam_self_weight_udl(&model, &model.unassigned_beams[0])
            .unwrap()
            .unwrap();
        assert_eq!(member.span, 4000.0);
        assert_eq!(
            member.member_loads,
            vec![MemberLoadKind::Distributed {
                a: 200.0,
                b: 3800.0,
                w1: w,
                w2: w
            }]
        );
        assert!((member.reactions.iter().sum::<f64>() - w * 3600.0).abs() < 1e-9);
        if let sepika_core::model::SecondaryMemberEnds::Supported(anchors) =
            &mut model.unassigned_beams[0].ends
        {
            for anchor in anchors {
                anchor.position = 0.0;
            }
        }
        assert_eq!(
            sepika_core::face_distance::secondary_self_weight_interval(
                &model,
                &model.unassigned_beams[0]
            )
            .unwrap(),
            [200.0, 3800.0]
        );
    }
}

#[test]
fn secondary_fireproof_support_reactions_design_loads_and_mass() {
    use sepika_core::model::{FireproofKind, FrameSectionUse};
    use sepika_core::section_shape::SectionShape;
    for cantilever in [false, true] {
        let mut model = face_model(4000.0, [400.0; 2]);
        for sec in &mut model.sections {
            sec.frame_use = Some(FrameSectionUse::Girder);
            sec.shape = Some(SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 12.0,
            });
        }
        let mut beam = SectionShape::SteelH {
            height: 300.0,
            width: 100.0,
            web_thick: 6.0,
            flange_thick: 9.0,
            root_r: Some(10.0),
        }
        .to_section(SectionId(2), "小梁".into());
        beam.material = Some(MaterialId(0));
        model.sections.push(beam);
        model.unassigned_beams[0].section = Some(SectionId(2));
        if cantilever {
            model.unassigned_beams[0].ends = sepika_core::model::SecondaryMemberEnds::Cantilever {
                support: sepika_core::model::SecondaryMemberAnchor {
                    support: sepika_core::model::SupportMemberId::Primary(ElemId(0)),
                    position: 0.5,
                },
                free_end_vector: [0.0, 4000.0],
            };
        }
        model.load_cfg = Some(LoadCfg {
            steel_weight_factor: 1.8,
            ..Default::default()
        });
        model.stories = vec![sepika_core::model::Story {
            wall_weights: Vec::new(),
            id: sepika_core::ids::StoryId(0),
            name: "1F".into(),
            elevation: 0.0,
            node_ids: vec![],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        }];
        let baseline = model.clone();
        let base_transfer = solve(&baseline, |_| 0.0, true).unwrap();
        let base_mass =
            solve_with_basis(&baseline, |_| 0.0, true, SelfWeightBasis::MassEquiv).unwrap();
        model.stories[0].fireproof.steel_kind = FireproofKind::Spray;
        model.stories[0].fireproof.steel_beam_area_weight = 0.002;
        let transfer = solve(&model, |_| 0.0, true).unwrap();
        let physical = solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::MassEquiv).unwrap();
        let key = SecondaryMemberId(10);
        let member = &transfer.members[&key];
        let interval = sepika_core::face_distance::secondary_self_weight_interval(
            &model,
            &model.unassigned_beams[0],
        )
        .unwrap();
        let p = 988.0 - (8.0 - 2.0 * std::f64::consts::PI) * 10.0;
        let extra = 0.002 * p * (interval[1] - interval[0]);
        assert!(
            (member.reactions.iter().sum::<f64>()
                - base_transfer.members[&key].reactions.iter().sum::<f64>()
                - extra)
                .abs()
                < 1e-7
        );
        assert!(
            (physical.members[&key].reactions.iter().sum::<f64>()
                - base_mass.members[&key].reactions.iter().sum::<f64>()
                - extra)
                .abs()
                < 1e-7
        );
        if cantilever {
            assert_eq!(member.reactions[1], 0.0);
        } else {
            for end in 0..2 {
                assert!(
                    (member.reactions[end]
                        - base_transfer.members[&key].reactions[end]
                        - extra / 2.0)
                        .abs()
                        < 1e-7
                );
            }
        }
        let loads = &member.member_loads;
        let old_loads = &base_transfer.members[&key].member_loads;
        let extremes = if cantilever {
            crate::floor::cantilever_extremes
        } else {
            crate::floor::simple_beam_extremes
        };
        assert!(
            extremes(loads, member.span, 205000.0, 1.0e8).m_max
                > extremes(old_loads, member.span, 205000.0, 1.0e8).m_max
        );
        let new_gen = crate::story_gen::generate_stories(&model, None).unwrap();
        let old_gen = crate::story_gen::generate_stories(&baseline, None).unwrap();
        let total_extra = extra + 0.002 * std::f64::consts::PI * 400.0 * 4000.0;
        assert!(
            (new_gen.stories[0].seismic_weight.unwrap()
                - old_gen.stories[0].seismic_weight.unwrap()
                - total_extra)
                .abs()
                < 1e-7
        );
        assert!(
            (new_gen.stories[0].dynamic_mass.unwrap().mass_equiv_weight_n
                - old_gen.stories[0].dynamic_mass.unwrap().mass_equiv_weight_n
                - total_extra)
                .abs()
                < 1e-7
        );
        let mut post = model.unassigned_beams[0].clone();
        post.kind = SecondaryMemberKind::Post;
        assert_eq!(
            beam_self_weight_udl(&model, &post).unwrap(),
            beam_self_weight_udl(&baseline, &post).unwrap()
        );
        model.sections[2].shape = None;
        assert!(solve(&model, |_| 0.0, true).is_err());
        assert!(beam_self_weight_udl(&model, &model.unassigned_beams[0]).is_err());
        assert!(beam_mass_equiv_udl(&model, &model.unassigned_beams[0]).is_err());
        model.sections[2].shape = Some(SectionShape::SteelH {
            height: 300.0,
            width: 100.0,
            web_thick: 6.0,
            flange_thick: 9.0,
            root_r: None,
        });
        assert!(solve(&model, |_| 0.0, true).is_err());
        assert!(beam_self_weight_udl(&model, &model.unassigned_beams[0]).is_err());
        assert!(beam_mass_equiv_udl(&model, &model.unassigned_beams[0]).is_err());
        assert!(solve(&model, |_| 0.0, false).is_ok());
        model.sections[2].shape = None;
        model.sections[2].material = None;
        assert!(beam_self_weight_udl(&model, &model.unassigned_beams[0]).is_err());
        assert!(beam_mass_equiv_udl(&model, &model.unassigned_beams[0]).is_err());
    }
}

#[test]
fn 小梁支持と非対称反力は芯々スパンを保持する() {
    let mut model = face_model(3000.0, [300.0, 200.0]);
    let support = SecondaryMember {
        id: SecondaryMemberId(20),
        section: Some(SectionId(1)),
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            [-1000.0, 3000.0, 0.0],
            [1000.0, 3000.0, 0.0],
        ]),
        ..Default::default()
    };
    model.elements.pop();
    model.unassigned_beams.push(support);
    if let sepika_core::model::SecondaryMemberEnds::Supported(anchors) =
        &mut model.unassigned_beams[0].ends
    {
        anchors[1].support = sepika_core::model::SupportMemberId::Secondary(SecondaryMemberId(20));
    }
    let transfer = solve(&model, |_| 0.0, true).unwrap();
    let member = &transfer.members[&SecondaryMemberId(10)];
    let w = w_self();
    assert_eq!(member.span, 3000.0);
    assert_eq!(
        member.member_loads,
        vec![MemberLoadKind::Distributed {
            a: 150.0,
            b: 2900.0,
            w1: w,
            w2: w
        }]
    );
    let total = w * 2750.0;
    assert!((member.reactions[0] - total * (3000.0 - 1525.0) / 3000.0).abs() < 1e-9);
    assert!((member.reactions[1] - total * 1525.0 / 3000.0).abs() < 1e-9);
}

#[test]
fn 勾配小梁と勾配支持小梁は鉛直基準の投影フェースと三次元軸を使う() {
    let mut model = face_model(4000.0, [400.0; 2]);
    model.nodes[2].coord[2] = 3000.0;
    model.nodes[3].coord[2] = 3000.0;
    let support = SecondaryMember {
        id: SecondaryMemberId(20),
        section: Some(SectionId(1)),
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            [-1000.0, 4000.0, 2250.0],
            [1000.0, 4000.0, 3750.0],
        ]),
        ..Default::default()
    };
    model.elements.pop();
    model.unassigned_beams.push(support);
    if let sepika_core::model::SecondaryMemberEnds::Supported(anchors) =
        &mut model.unassigned_beams[0].ends
    {
        anchors[1].support = sepika_core::model::SupportMemberId::Secondary(SecondaryMemberId(20));
    }
    let interval = sepika_core::face_distance::secondary_self_weight_interval(
        &model,
        &model.unassigned_beams[0],
    )
    .unwrap();
    assert_eq!(interval, [250.0, 4750.0]);
    let transfer = solve(&model, |_| 0.0, true).unwrap();
    let member = &transfer.members[&SecondaryMemberId(10)];
    assert_eq!(member.span, 5000.0);
    assert!((member.reactions.iter().sum::<f64>() - w_self() * 4500.0).abs() < 1e-9);
    let physical = solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::MassEquiv).unwrap();
    assert!(
        (physical.members[&SecondaryMemberId(10)]
            .reactions
            .iter()
            .sum::<f64>()
            - DENSITY * 160000.0 * sepika_core::units::GRAVITY_MM_S2 * 4500.0)
            .abs()
            < 1e-9
    );
    model.sections[1].shape = Some(sepika_core::section_shape::SectionShape::SteelChannel {
        height: 300.0,
        width: 100.0,
        web_thick: 10.0,
        flange_thick: 20.0,
    });
    let centroid = (4000.0 * 50.0 + 2600.0 * 5.0) / 6600.0;
    let interval = sepika_core::face_distance::secondary_self_weight_interval(
        &model,
        &model.unassigned_beams[0],
    )
    .unwrap();
    assert!((interval[1] - (5000.0 - (100.0 - centroid) / 0.8)).abs() < 1e-9);
}

#[test]
fn 片持ち自重は支持端だけ控除し不正長と未解決参照はエラー() {
    let mut model = face_model(3000.0, [300.0, 200.0]);
    model.unassigned_beams[0].ends = sepika_core::model::SecondaryMemberEnds::Cantilever {
        support: sepika_core::model::SecondaryMemberAnchor {
            support: sepika_core::model::SupportMemberId::Primary(ElemId(0)),
            position: 0.5,
        },
        free_end_vector: [0.0, 3000.0],
    };
    let transfer = solve(&model, |_| 0.0, true).unwrap();
    let member = &transfer.members[&SecondaryMemberId(10)];
    assert!((member.reactions[0] - w_self() * 2850.0).abs() < 1e-9);
    assert_eq!(member.reactions[1], 0.0);
    model.sections[0].width = 6000.0;
    model.sections[0].shape = Some(sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: 6000.0,
        thick: 400.0,
    });
    let error = solve(&model, |_| 0.0, true).unwrap_err().to_string();
    assert!(
        error.contains("二次部材 10") && error.contains("3000") && error.contains("終端控除 0"),
        "{error}"
    );
    model.elements[0].section = None;
    assert!(solve(&model, |_| 0.0, true)
        .unwrap_err()
        .to_string()
        .contains("断面を解決できません"));
}

/// 両端が大梁に載る小梁は、自重の半分ずつを主架構へ渡して終端する。
#[test]
fn beam_on_girders_terminates_at_primary() {
    let mut m = base_model();
    // 大梁 0-1（X 方向、y=0）と 2-3（X 方向、y=4000）。小梁は 4-5（Y 方向、x=3000）。
    for (i, c) in [
        [0.0, 0.0, 0.0],
        [6000.0, 0.0, 0.0],
        [0.0, 4000.0, 0.0],
        [6000.0, 4000.0, 0.0],
        [3000.0, 0.0, 0.0],
        [3000.0, 4000.0, 0.0],
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 2, 3));
    m.unassigned_beams.push(secondary_beam(&m, 4, 5, "SB1"));

    let t = solved(&mut m);
    let key = sepika_core::ids::SecondaryMemberId(4);
    let sm = t.members.get(&key).expect("小梁");
    assert_eq!(sm.supports, [SupportAt::Primary, SupportAt::Primary]);
    let expected = w_self() * 3800.0 / 2.0;
    for r in sm.reactions {
        assert!((r - expected).abs() / expected < 1e-9, "反力 {r}");
    }
    assert!(t.unresolved.is_empty());
    assert!(t.cyclic.is_empty());
    assert!(super::secondary_crossings(&m).is_empty());
}

/// 物理質量基準（[`SelfWeightBasis::MassEquiv`]）でも、二次部材（鋼）の自重に
/// 設計重量と同じ鉄骨重量割増が掛かる（主架構線材と同じ規則）。
#[test]
fn beam_mass_equiv_applies_steel_weight_factor() {
    let mut m = base_model();
    m.load_cfg = Some(LoadCfg {
        steel_weight_factor: 1.3,
        ..Default::default()
    });
    for (i, c) in [
        [0.0, 0.0, 0.0],
        [6000.0, 0.0, 0.0],
        [0.0, 4000.0, 0.0],
        [6000.0, 4000.0, 0.0],
        [3000.0, 0.0, 0.0],
        [3000.0, 4000.0, 0.0],
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 2, 3));
    m.unassigned_beams.push(secondary_beam(&m, 4, 5, "SB1"));
    m.anchorize_secondary_members();

    let design = solve_with_basis(&m, |_| 0.0, true, SelfWeightBasis::Design).unwrap();
    let mass = solve_with_basis(&m, |_| 0.0, true, SelfWeightBasis::MassEquiv).unwrap();
    let key = sepika_core::ids::SecondaryMemberId(4);
    let rd = design.members.get(&key).expect("設計の小梁").reactions;
    let rm = mass.members.get(&key).expect("物理質量の小梁").reactions;
    let factor = 1.3;
    let expect_d = DESIGN_UNIT_WEIGHT_N_PER_MM3 * AREA * factor * 3800.0 / 2.0;
    let expect_m = DENSITY * 80000.0 * sepika_core::units::GRAVITY_MM_S2 * factor * 3800.0 / 2.0;
    assert!(
        (rd[0] - expect_d).abs() / expect_d < 1e-9,
        "設計反力 {}",
        rd[0]
    );
    assert!(
        (rm[0] - expect_m).abs() / expect_m < 1e-9,
        "物理反力 {}",
        rm[0]
    );
}

#[test]
fn rounded_secondary_supplied_area_mass_reaches_stories_and_unknown_is_error() {
    use sepika_core::model::PropertyBasis;
    use sepika_core::section_shape::SectionShape;
    for shape in [
        SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 8.0,
            flange_thick: 13.0,
            root_r: Some(13.0),
        },
        SectionShape::SteelBox {
            height: 500.0,
            width: 300.0,
            thick: 10.0,
            corner_r: Some(30.0),
        },
        SectionShape::CftBox {
            height: 500.0,
            width: 300.0,
            thick: 10.0,
            corner_r: Some(30.0),
        },
    ] {
        let mut model = face_model(4000.0, [400.0; 2]);
        model.nodes.push(node(4, -1000.0, 0.0, -3000.0));
        model.elements.push(element_beam(2, 4, 0));
        model.elements[2].local_axis.ref_vector = [1.0, 0.0, 0.0];
        model.load_cfg = Some(LoadCfg {
            steel_weight_factor: 1.3,
            ..Default::default()
        });
        let is_cft = matches!(shape, SectionShape::CftBox { .. });
        let mut section = shape.to_section(SectionId(2), "保護A小梁".into());
        section.material = Some(MaterialId(0));
        section.area = 20000.0;
        section.property_basis.area = PropertyBasis::Supplied;
        if is_cft {
            let mut concrete = model.materials[0].clone();
            concrete.id = MaterialId(1);
            concrete.category = MaterialCategory::Concrete;
            concrete.fc = Some(36.0);
            model.materials.push(concrete);
            section.material = Some(MaterialId(1));
            section.steel_material = Some(MaterialId(0));
        }
        model.sections.push(section);
        model.unassigned_beams[0].section = Some(SectionId(2));
        let sec = &model.sections[2];
        let main = model
            .secondary_material(&model.unassigned_beams[0])
            .unwrap();
        let mu = sepika_core::model::SectionMassProperties::try_from_section(
            sec,
            Some(main),
            None,
            None,
            Some(&model.materials[0]),
        )
        .unwrap()
        .mass_per_length;
        let core_mu = if is_cft {
            main.cft_core_mass_density() * shape.try_cft_core_props().unwrap().unwrap().area
        } else {
            0.0
        };
        let expected = (mu + 0.3 * (mu - core_mu)) * sepika_core::units::GRAVITY_MM_S2 * 3600.0;
        let transfer = solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::MassEquiv).unwrap();
        assert!(
            (transfer.members[&SecondaryMemberId(10)]
                .reactions
                .iter()
                .sum::<f64>()
                - expected)
                .abs()
                < 1e-8
        );
        let mut baseline = model.clone();
        baseline.unassigned_beams.clear();
        let sum_mass = |m: &Model| {
            crate::story_gen::generate_stories(m, None)
                .unwrap()
                .stories
                .iter()
                .map(|s| s.dynamic_mass.unwrap().mass_equiv_weight_n)
                .sum::<f64>()
        };
        assert!((sum_mass(&model) - sum_mass(&baseline) - expected).abs() < 1e-8);
        assert_eq!(model.sections[2].area, 20000.0);
        let design_udl = beam_self_weight_udl(&model, &model.unassigned_beams[0])
            .unwrap()
            .unwrap();
        let factor = if is_cft { 1.0 } else { 1.3 };
        assert!((design_udl - main.design_unit_weight_n_per_mm3() * 20000.0 * factor).abs() < 1e-9);
        if !is_cft {
            let mut unknown = shape;
            match &mut unknown {
                SectionShape::SteelH { root_r, .. } => *root_r = None,
                SectionShape::SteelBox { corner_r, .. } => *corner_r = None,
                _ => unreachable!(),
            }
            model.sections[2] = unknown
                .input_section(SectionId(2), "未知小梁".into())
                .unwrap();
            model.sections[2].material = Some(MaterialId(0));
            model.sections[2].area = 20000.0;
            model.sections[2].property_basis.area = PropertyBasis::Supplied;
            assert!(beam_self_weight_udl(&model, &model.unassigned_beams[0]).is_ok());
            assert!(solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::MassEquiv).is_err());
            let error = crate::story_gen::generate_stories(&model, None).unwrap_err();
            assert!(
                error.contains("フィレット") || error.contains("角R"),
                "{error}"
            );
        }
    }
}

/// 小梁 B の端点が小梁 A の内部に載るとき、B の反力は A の集中荷重として渡り、
/// 主架構へ渡る総和は 2 本の自重の合計に一致する（荷重が消えない）。
#[test]
fn beam_on_beam_cascades_to_primary() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],       // 0 大梁端
        [6000.0, 0.0, 0.0],    // 1 大梁端
        [0.0, 4000.0, 0.0],    // 2 大梁端
        [6000.0, 4000.0, 0.0], // 3 大梁端
        [3000.0, 0.0, 0.0],    // 4 A の端（大梁上）
        [3000.0, 4000.0, 0.0], // 5 A の端（大梁上）
        [3000.0, 2000.0, 0.0], // 6 A の中央（B の端。どの大梁にも載らない）
        [6000.0, 2000.0, 0.0], // 7 B の端（右側の大梁 1-3 のスパン上）
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 2, 3));
    m.elements.push(element_beam(2, 1, 3)); // 右側の大梁（節点 7 がこのスパン上に載る）
    m.unassigned_beams.push(secondary_beam(&m, 4, 5, "A"));
    m.unassigned_beams.push(secondary_beam(&m, 6, 7, "B"));

    let t = solved(&mut m);
    let ka = sepika_core::ids::SecondaryMemberId(4);
    let kb = sepika_core::ids::SecondaryMemberId(6);
    let a = t.members.get(&ka).expect("A");
    let b = t.members.get(&kb).expect("B");

    // B の節点 6 側は A の内部に載る（軸は支持端→自由端の順なので B は端 0）。
    let i6 = 0;
    assert!(
        matches!(b.supports[i6], SupportAt::Secondary { key, .. } if key == ka),
        "B の端は A に載る: {:?}",
        b.supports
    );
    // A の両端は大梁上で終端する。
    assert_eq!(a.supports, [SupportAt::Primary, SupportAt::Primary]);

    // 主架構へ渡る総和 = A の自重 + B の自重。
    let (nodal, member) = t.primary_loads(&m);
    let total: f64 = nodal
        .iter()
        .map(|(_, r)| *r)
        .chain(member.iter().map(|bl| bl.cmq.q_i + bl.cmq.q_j))
        .sum();
    let expected = w_self() * (3800.0 + 2800.0);
    assert!(
        (total - expected).abs() / expected < 1e-9,
        "主架構へ渡る総和 {total} != 自重合計 {expected}"
    );
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
}

/// 大梁の材軸中間（座標が一致するモデル節点が無い位置）へ載る小梁の反力は、
/// その大梁の中間集中荷重として渡る（節点が無いことを理由に荷重を捨てない）。
#[test]
fn beam_anchored_to_girder_midspan_becomes_point_load() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],       // 0 大梁 A 端
        [6000.0, 0.0, 0.0],    // 1 大梁 A 端
        [0.0, 4000.0, 0.0],    // 2 大梁 B 端
        [6000.0, 4000.0, 0.0], // 3 大梁 B 端
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 2, 3));
    // 両端が大梁 A・B の材軸中間（節点の無い位置）に載る小梁。
    m.unassigned_beams.push(SecondaryMember {
        gravity_end_shares: None,
        id: sepika_core::ids::SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            [3000.0, 0.0, 0.0],
            [3000.0, 4000.0, 0.0],
        ]),
        section: Some(SectionId(0)),
        name: "SB".into(),
    });

    let t = solved(&mut m);
    let sm = t
        .members
        .get(&sepika_core::ids::SecondaryMemberId(0))
        .expect("小梁");
    assert_eq!(
        sm.supports,
        [SupportAt::Primary, SupportAt::Primary],
        "{:?}",
        sm.supports
    );
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);

    let (nodal, member) = t.primary_loads(&m);
    assert!(
        nodal.is_empty(),
        "座標一致節点が無いので節点荷重は無い: {nodal:?}"
    );
    let expected = w_self() * 3800.0;
    let total: f64 = member.iter().map(|bl| bl.cmq.q_i + bl.cmq.q_j).sum();
    assert!(
        (total - expected).abs() / expected < 1e-9,
        "中間集中荷重の総和 {total} != 自重 {expected}"
    );
    assert_eq!(member.len(), 2, "2 本の大梁へ 1 件ずつ: {member:?}");
    for bl in &member {
        match bl.shape {
            LoadShape::Point { p, x } => {
                assert!((x - 3000.0).abs() < 1e-9, "大梁 i 端から 3000: {x}");
                assert!(
                    (p - expected / 2.0).abs() / (expected / 2.0) < 1e-9,
                    "反力 {p}"
                );
            }
            _ => panic!("中間集中荷重になるはず: {bl:?}"),
        }
    }
}

/// 鉛直な間柱は、利用者が明示した端部負担率で自重を両端へ配る。
///
/// 水平投影が 0 だと鉛直反力のモーメントのつり合いが退化するため、幾何から等分は
/// せず、未指定・不正値は逐次伝達の対象から外して解析前チェックがエラーにする（ADR 0018）。
#[test]
fn vertical_post_splits_load_by_explicit_shares() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],
        [6000.0, 0.0, 0.0],
        [0.0, 0.0, 3000.0],
        [6000.0, 0.0, 3000.0],
        [3000.0, 0.0, 0.0],
        [3000.0, 0.0, 3000.0],
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1)); // 下の梁
    m.elements.push(element_beam(1, 2, 3)); // 上の梁
    m.unassigned_posts.push(SecondaryMember {
        gravity_end_shares: Some([0.5, 0.5]),
        id: sepika_core::ids::SecondaryMemberId(4),
        kind: SecondaryMemberKind::Post,
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            m.nodes[4].coord,
            m.nodes[5].coord,
        ]),
        section: Some(SectionId(0)),
        name: "P1".into(),
    });

    let key = sepika_core::ids::SecondaryMemberId(4);
    let half = w_self() * 3000.0 / 2.0;
    let t = solved(&mut m);
    let p = t.members.get(&key).expect("間柱");
    for r in p.reactions {
        assert!((r - half).abs() / half < 1e-9, "反力 {r} != {half}");
    }

    // 明示した負担率どおりに配る（幾何の等分には戻らない）。
    for (ratios, expected) in [
        ([1.0, 0.0], [2.0 * half, 0.0]),
        ([0.25, 0.75], [0.5 * half, 1.5 * half]),
        ([0.0, 1.0], [0.0, 2.0 * half]),
    ] {
        m.unassigned_posts[0].gravity_end_shares = Some(ratios);
        let t = solved(&mut m);
        let p = t.members.get(&key).expect("間柱");
        for (actual, expected) in p.reactions.iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "反力 {actual} != {expected}"
            );
        }
    }

    // 未指定・不正値は逐次伝達の対象から外れ、自重を主架構へ渡さない（解析前エラー）。
    for ratios in [
        None,
        Some([0.0, 0.0]),
        Some([-0.5, 1.5]),
        Some([f64::NAN, 1.0]),
    ] {
        m.unassigned_posts[0].gravity_end_shares = ratios;
        let t = solved(&mut m);
        assert_eq!(t.invalid_end_shares, vec![key]);
        assert!(
            !t.members.contains_key(&key),
            "不正な端部負担率の間柱は逐次伝達の対象から外れる"
        );
        let (nodal, member) = t.primary_loads(&m);
        assert!(nodal.is_empty(), "節点荷重を渡さない: {nodal:?}");
        assert!(member.is_empty(), "中間集中荷重を渡さない: {member:?}");
    }
}

/// 傾斜した二次部材の鉛直反力は、材軸上の按分（単純梁の反力）と一致する。
///
/// 鉛直反力は水平てこでのモーメントつり合いで決まり、荷重の材軸上の位置は水平投影へ
/// 線形に写るため、水平投影が 0 でなければ成分へ分ける必要はない。「材軸方向成分を
/// 1/2 ずつ、直交成分を単純梁反力」として `|u_z|` で混ぜると、総和は保存するが配分が
/// 誤り、載荷側の反力を過小評価する（受け側にとって危険側）。実フィクスチャの小梁は
/// すべて水平なのでこの誤りを検出できない。ここで固定する。
#[test]
fn inclined_beam_reactions_match_simple_beam() {
    let mut m = base_model();
    // 水平投影 4000・鉛直 3000（L=5000）の傾斜小梁。両端は大梁に載せて終端させる。
    for (i, c) in [
        [0.0, -1000.0, 0.0],
        [0.0, 1000.0, 0.0],
        [4000.0, -1000.0, 3000.0],
        [4000.0, 1000.0, 3000.0],
        [0.0, 0.0, 0.0],       // 4 傾斜小梁の下端（下の大梁上）
        [4000.0, 0.0, 3000.0], // 5 傾斜小梁の上端（上の大梁上）
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 2, 3));
    m.unassigned_beams.push(secondary_beam(&m, 4, 5, "SB"));

    let t = solved(&mut m);
    let key = sepika_core::ids::SecondaryMemberId(4);
    let sm = t.members.get(&key).expect("傾斜小梁");
    assert_eq!(sm.supports, [SupportAt::Primary, SupportAt::Primary]);

    // 自重は等分布なので、鉛直反力は両端等分（総量は ρAgL）。
    let total = w_self() * (5000.0 - 2.0 * 100.0 / 0.8);
    for r in sm.reactions {
        assert!(
            (r - total / 2.0).abs() / total < 1e-9,
            "等分布の鉛直反力は両端等分: {r}"
        );
    }

    // 材軸上 1/5 の位置に集中荷重を足すと、鉛直反力は 4:1 に分かれる
    // （水平てこでのモーメントつり合い。混ぜると 0.62:0.38 になってしまう）。
    let p = 1000.0_f64;
    let (ri, rj) = super::reactions_of(&MemberLoadKind::Point { a: 1000.0, p }, 5000.0, None);
    assert!((ri - 0.8 * p).abs() < 1e-9, "R_i={ri}");
    assert!((rj - 0.2 * p).abs() < 1e-9, "R_j={rj}");

    // 端部負担率を両端 1/2 に固定すると、両端の負担は 1/2 ずつになる。
    let (ri, rj) = super::reactions_of(
        &MemberLoadKind::Point { a: 1000.0, p },
        5000.0,
        Some([0.5, 0.5]),
    );
    assert!((ri - 0.5 * p).abs() < 1e-9 && (rj - 0.5 * p).abs() < 1e-9);
}

/// 端部がどの主架構にも二次部材にも載らない二次部材は `unresolved` に入る。
#[test]
fn floating_beam_is_unresolved() {
    let mut m = base_model();
    m.nodes.push(node(0, 0.0, 0.0, 0.0));
    m.nodes.push(node(1, 4000.0, 0.0, 0.0));
    m.unassigned_beams.push(secondary_beam(&m, 0, 1, "SB"));

    let t = solved(&mut m);
    assert_eq!(t.unresolved, vec![sepika_core::ids::SecondaryMemberId(0)]);
}

/// 支持関係が一巡する二次部材は荷重を流せないので `cyclic` に入り、逐次伝達の対象から
/// 外れる（モデルの誤りであり、診断のエラーで知らせる）。
///
/// 直線 2 本では相互支持は幾何的に成立しない（互いの端点が相手の内部にある配置が
/// 作れない）ため、3 本で一巡させる。
#[test]
fn cyclic_support_is_reported() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],         // 0 A 始点（C の内部に載る）
        [4000.0, 0.0, 0.0],      // 1 A 終点
        [2000.0, 0.0, 0.0],      // 2 B 始点（A の内部に載る）
        [2000.0, 4000.0, 0.0],   // 3 B 終点
        [2000.0, 2000.0, 0.0],   // 4 C 始点（B の内部に載る）
        [-2000.0, -2000.0, 0.0], // 5 C 終点
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.unassigned_beams.push(secondary_beam(&m, 0, 1, "A"));
    m.unassigned_beams.push(secondary_beam(&m, 2, 3, "B"));
    m.unassigned_beams.push(secondary_beam(&m, 4, 5, "C"));

    // 循環はアンカー解決できない（支持を辿ると自分へ戻る）ため、生座標のまま解く。
    let t = solve(&m, |_| 0.0, true).unwrap();
    let keys = [
        sepika_core::ids::SecondaryMemberId(0),
        sepika_core::ids::SecondaryMemberId(2),
        sepika_core::ids::SecondaryMemberId(4),
    ];
    for k in keys {
        assert!(
            t.cyclic.contains(&k),
            "{k:?} が循環に含まれる: {:?}",
            t.cyclic
        );
        assert!(
            !t.members.contains_key(&k),
            "循環した二次部材は逐次伝達の対象から外れる"
        );
    }
}

/// 節点を共有せず交差する 2 本は、受け側・架け側を決められないので `crossings` に入る。
#[test]
fn crossing_without_shared_node_is_reported() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 2000.0, 0.0],
        [6000.0, 2000.0, 0.0],
        [3000.0, 0.0, 0.0],
        [3000.0, 4000.0, 0.0],
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.unassigned_beams.push(secondary_beam(&m, 0, 1, "A"));
    m.unassigned_beams.push(secondary_beam(&m, 2, 3, "B"));

    let crossings = super::secondary_crossings(&m);
    assert_eq!(crossings.len(), 1, "交差 1 組: {crossings:?}");
}

/// 荷重を持たない二次部材は、端部の行き先が決まらなくても報告しない。
///
/// 断面が未割当なら自重も床分配も載らず、失う荷重がない。形だけ置かれた支持点で
/// 解析前チェックのエラーを出さないための扱いである（`solve` 末尾の判定）。
#[test]
fn floating_beam_without_load_is_not_reported() {
    let mut m = base_model();
    m.nodes.push(node(0, 0.0, 0.0, 0.0));
    m.nodes.push(node(1, 4000.0, 0.0, 0.0));
    m.unassigned_beams.push(SecondaryMember {
        gravity_end_shares: None,
        id: sepika_core::ids::SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            m.nodes[0].coord,
            m.nodes[1].coord,
        ]),
        section: None,
        name: "SB".into(),
    });

    let t = solved(&mut m);
    let key = sepika_core::ids::SecondaryMemberId(0);
    let sm = t.members.get(&key).expect("小梁");
    assert_eq!(sm.supports, [SupportAt::Unresolved; 2]);
    assert_eq!(sm.reactions, [0.0, 0.0]);
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
}

/// 実部材化された二次部材（両端を持つ実 `Beam` がある）は逐次伝達の対象外。
#[test]
fn materialized_beam_is_skipped() {
    let mut m = base_model();
    m.nodes.push(node(0, 0.0, 0.0, 0.0));
    m.nodes.push(node(1, 4000.0, 0.0, 0.0));
    m.elements.push(element_beam(0, 0, 1));
    m.unassigned_beams.push(secondary_beam(&m, 0, 1, "SB"));

    let t = solved(&mut m);
    assert!(t.members.is_empty(), "実部材化済みは対象外");
}

/// 片持ち小梁を作る（`free_at` は自由端の位置。幾何から自由端を推定するため
/// `_free_at` は使わない。自由端が幾何的に支持されるテストは配置を調整する）。
fn cantilever(model: &Model, a: u32, b: u32, _free_at: usize, name: &str) -> SecondaryMember {
    secondary_beam(model, a, b, name)
}

/// 大梁 0-1 に載る片持ち小梁の自重は、基端の反力だけになって主架構へ渡る。
#[test]
fn cantilever_beam_transfers_to_base_only() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],       // 0 大梁端
        [6000.0, 0.0, 0.0],    // 1 大梁端
        [3000.0, 0.0, 0.0],    // 2 基端（大梁のスパン上）
        [3000.0, 4000.0, 0.0], // 3 自由端
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.unassigned_beams.push(cantilever(&m, 2, 3, 1, "SB"));

    let t = solved(&mut m);
    let key = sepika_core::ids::SecondaryMemberId(2);
    let sm = t.members.get(&key).expect("片持ち小梁");
    assert_eq!(
        sm.supports,
        [SupportAt::Primary, SupportAt::Free],
        "基端のみ支持"
    );
    let expected = w_self() * 3900.0;
    assert!((sm.reactions[0] - expected).abs() / expected < 1e-9);
    assert_eq!(sm.reactions[1], 0.0);
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
    assert!(t.cyclic.is_empty());
}

/// 基端が nodes[1] の片持ち小梁も、全反力が基端へ渡る。
#[test]
fn cantilever_beam_base_at_second_node() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 4000.0, 0.0],    // 0 大梁端
        [6000.0, 4000.0, 0.0], // 1 大梁端
        [3000.0, 1000.0, 0.0], // 2 自由端
        [3000.0, 4000.0, 0.0], // 3 基端（大梁のスパン上）
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.unassigned_beams.push(cantilever(&m, 2, 3, 0, "SB"));

    let t = solved(&mut m);
    let key = sepika_core::ids::SecondaryMemberId(2);
    let sm = t.members.get(&key).expect("片持ち小梁");
    assert_eq!(sm.supports, [SupportAt::Primary, SupportAt::Free]);
    let expected = w_self() * 2900.0;
    assert!((sm.reactions[0] - expected).abs() / expected < 1e-9);
    assert_eq!(sm.reactions[1], 0.0);
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
}

/// 先端リブは片持ち小梁の自由端に載れる。リブの反力は自由端を通り、
/// 片持ち小梁の基端へまとめて渡る（荷重は消えない）。
#[test]
fn tip_rib_on_cantilever_free_ends_cascades_to_bases() {
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],       // 0 大梁端
        [6000.0, 0.0, 0.0],    // 1 大梁端
        [1000.0, 0.0, 0.0],    // 2 片持ち A 基端（大梁のスパン上）
        [1000.0, 3000.0, 0.0], // 3 片持ち A 自由端
        [5000.0, 0.0, 0.0],    // 4 片持ち B 基端（大梁のスパン上）
        [5000.0, 3000.0, 0.0], // 5 片持ち B 自由端
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.unassigned_beams.push(cantilever(&m, 2, 3, 1, "CA"));
    m.unassigned_beams.push(cantilever(&m, 4, 5, 1, "CB"));
    m.unassigned_beams.push(secondary_beam(&m, 3, 5, "RIB"));

    let t = solved(&mut m);
    let ca = t
        .members
        .get(&sepika_core::ids::SecondaryMemberId(2))
        .expect("片持ち A");
    let cb = t
        .members
        .get(&sepika_core::ids::SecondaryMemberId(4))
        .expect("片持ち B");
    let rib = t
        .members
        .get(&sepika_core::ids::SecondaryMemberId(3))
        .expect("先端リブ");

    for (beam, base) in [(&ca, 2u32), (&cb, 4)] {
        assert_eq!(beam.supports[1], SupportAt::Free);
        assert_eq!(beam.reactions[1], 0.0, "自由端の反力は 0");
        assert_eq!(beam.end_points[0], m.nodes[base as usize].coord);
    }
    assert!(matches!(rib.supports[0], SupportAt::Secondary { .. }));
    assert!(matches!(rib.supports[1], SupportAt::Secondary { .. }));

    let expected_total = w_self() * (2900.0 * 2.0 + 4000.0);
    let total = ca.reactions[0] + cb.reactions[0];
    assert!(
        (total - expected_total).abs() / expected_total < 1e-9,
        "総和={total} expected={expected_total}"
    );
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);
    assert!(t.cyclic.is_empty());
}

/// 取り付く床板の荷重は、境界の片持ち小梁（二次部材）へ渡らず、取付き大梁へ残る。
#[test]
fn attached_slab_load_ignores_side_beam() {
    use sepika_core::ids::SlabId;
    use sepika_core::model::{AreaLoad, LoadTransfer, RegionAnchor, Slab, SlabPlate, SlabShape};
    let w = 0.005_f64;
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],    // 0 大梁端（取付き線）
        [6000.0, 0.0, 0.0], // 1 大梁端
        [0.0, 1500.0, 0.0], // 2 小梁の自由端
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.unassigned_beams.push(cantilever(&m, 0, 2, 1, "J"));
    m.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1500.0, 1500.0],
        },
        plate: SlabPlate {
            loads: vec![AreaLoad {
                kind: "DL".into(),
                value: w,
            }],
            ..Default::default()
        },
        tip_loads: Vec::new(),
    });

    m.anchorize_secondary_members();
    let t = solve(&m, |_| w, true).unwrap();
    let beam = t
        .members
        .get(&sepika_core::ids::SecondaryMemberId(0))
        .expect("小梁");
    let expected = w_self() * 1400.0;
    assert!(
        (beam.reactions[0] - expected).abs() / expected < 0.02,
        "基端反力={} expected={expected}",
        beam.reactions[0]
    );
    assert_eq!(beam.reactions[1], 0.0, "自由端の反力は 0");
    assert!(t.unresolved.is_empty(), "{:?}", t.unresolved);

    let leftover_total: f64 = t
        .leftover_region_loads
        .iter()
        .map(|bl| bl.cmq.q_i + bl.cmq.q_j)
        .sum();
    let rest = w * (6000.0 * 1500.0);
    assert!(
        (leftover_total - rest).abs() / rest < 0.02,
        "取付き辺 {leftover_total} expected={rest}"
    );
}

/// 小梁端が大梁材軸中間（モデル節点なし）にある床板で、`distribute_slab_resolved`
/// からカスケードを経て主架構へ渡る荷重の総和が `w × 全床面積` と一致する。
///
/// 床板の小梁境界の辺荷重は `LoadTarget::Secondary` で小梁を指し、小梁の両端反力は
/// 節点の無い大梁材軸中間へ中間集中荷重として渡る。分配（`distribute_slab_resolved`）
/// の総和と、カスケード後の主架構への総和の両方を確かめ、取り落とし・二重計上が
/// 無いことを固定する。
#[test]
fn midspan_beam_floor_conserves_total_through_cascade() {
    use crate::floor::{distribute_slab_resolved, LoadTarget};
    use sepika_core::ids::SlabId;
    use sepika_core::model::{
        AreaLoad, DistributionMethod, PlateAssignment, Slab, SlabPlate, SlabShape, SupportMemberId,
    };

    let w = 0.005_f64;
    let area = 6000.0 * 4000.0;
    let mut m = base_model();
    for (i, c) in [
        [0.0, 0.0, 0.0],       // 0
        [6000.0, 0.0, 0.0],    // 1
        [6000.0, 4000.0, 0.0], // 2
        [0.0, 4000.0, 0.0],    // 3
    ]
    .iter()
    .enumerate()
    {
        m.nodes.push(node(i as u32, c[0], c[1], c[2]));
    }
    m.elements.push(element_beam(0, 0, 1));
    m.elements.push(element_beam(1, 1, 2));
    m.elements.push(element_beam(2, 2, 3));
    m.elements.push(element_beam(3, 3, 0));
    // 中央小梁は下辺大梁（0-1）と上辺大梁（2-3）の材軸中間にアンカーする。
    // アンカー位置にはモデル節点が無い。
    let beam_key = sepika_core::ids::SecondaryMemberId(0);
    m.unassigned_beams.push(SecondaryMember {
        gravity_end_shares: None,
        id: beam_key,
        kind: SecondaryMemberKind::Beam,
        ends: sepika_core::model::SecondaryMemberEnds::Supported([
            sepika_core::model::SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            sepika_core::model::SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 0.5,
            },
        ]),
        section: Some(SectionId(0)),
        name: "SB".into(),
    });
    let report = m.rebuild_floor_assignment_regions();
    assert_eq!(report.regions, 2, "中央小梁で 2 面");

    let region_ids: Vec<_> = m
        .floor_assignment_regions
        .regions
        .iter()
        .map(|region| region.id)
        .collect();
    for (i, region_id) in region_ids.iter().enumerate() {
        let slab_id = SlabId(i as u32);
        m.slabs.push(Slab {
            id: slab_id,
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                loads: vec![AreaLoad {
                    kind: "DL".into(),
                    value: w,
                }],
                method: DistributionMethod::TriTrapezoid,
                ..Default::default()
            },
            tip_loads: Vec::new(),
        });
        m.floor_assignment_regions
            .get_mut(*region_id)
            .expect("直前に作った割当領域")
            .assignment = PlateAssignment::Plate(slab_id);
    }

    // ① 分配（Blocker 1 の経路）だけで総和保存する。
    let resolved: Vec<BeamLoad> = m
        .slabs
        .iter()
        .flat_map(|slab| distribute_slab_resolved(&m, slab, w).unwrap())
        .collect();
    let resolved_total: f64 = resolved.iter().map(|bl| bl.cmq.q_i + bl.cmq.q_j).sum();
    assert!(
        (resolved_total - w * area).abs() / (w * area) < 1e-9,
        "分配の総和 {resolved_total} != {}",
        w * area
    );
    assert!(
        resolved.iter().any(
            |bl| matches!(bl.target, LoadTarget::Secondary { member, .. } if member == beam_key)
        ),
        "小梁境界の辺荷重は Secondary で小梁を指す: {resolved:?}"
    );

    // ② カスケードを経て主架構へ渡る荷重（残りの辺荷重 + 小梁の反力）の総和。
    let transfer = solve(&m, |_| w, false).unwrap();
    assert!(transfer.unresolved.is_empty(), "{:?}", transfer.unresolved);
    assert!(transfer.cyclic.is_empty());
    let (nodal, member) = transfer.primary_loads(&m);
    assert!(
        nodal.is_empty(),
        "アンカー位置に節点が無いので節点荷重は無い: {nodal:?}"
    );
    let leftover_total: f64 = transfer
        .leftover_region_loads
        .iter()
        .map(|bl| bl.cmq.q_i + bl.cmq.q_j)
        .sum();
    let primary_total: f64 = member.iter().map(|bl| bl.cmq.q_i + bl.cmq.q_j).sum();
    let beam = transfer
        .members
        .get(&beam_key)
        .expect("小梁がカスケードの対象になっている");
    assert!(
        !beam.member_loads.is_empty(),
        "小梁が受け持つ荷重が空でない: {beam:?}"
    );
    assert!(
        beam.reactions[0].abs() + beam.reactions[1].abs() > 0.0,
        "小梁の反力が主架構へ渡る: {:?}",
        beam.reactions
    );
    assert!(
        primary_total > 0.0,
        "主架構への中間集中荷重が含まれる: {member:?}"
    );
    let total = leftover_total + primary_total;
    assert!(
        (total - w * area).abs() / (w * area) < 1e-9,
        "カスケード後の総和 {total} != {}（取り落とし・二重計上）",
        w * area
    );
}

#[test]
fn secondary_high_density_design_guard_does_not_block_physical_cascade() {
    for kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
        let mut model = face_model(4000.0, [400.0; 2]);
        model.unassigned_beams[0].kind = kind;
        if kind == SecondaryMemberKind::Post {
            let post = model.unassigned_beams.pop().unwrap();
            model.unassigned_posts.push(post);
        }
        model.materials[0].density = 85e-6 / 9806.65;
        let error = beam_self_weight_udl(
            &model,
            model.secondary_member(SecondaryMemberId(10)).unwrap(),
        )
        .unwrap_err();
        assert!(
            error.contains("材料 0") && error.contains("二次部材 10"),
            "{error}"
        );
        assert!(beam_mass_equiv_udl(
            &model,
            model.secondary_member(SecondaryMemberId(10)).unwrap()
        )
        .is_ok());
        assert!(solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::Design).is_err());
        assert!(solve_with_basis(&model, |_| 0.0, true, SelfWeightBasis::MassEquiv).is_ok());
    }
}
