use sepika_core::{
    face_distance::{
        girder_self_weight_interval, secondary_self_weight_interval, wall_support_face,
    },
    ids::*,
    model::*,
    section_shape::SectionShape,
};
use sepika_edit::*;
use sepika_load::{cascade, floor::beam_self_weight_udl};

fn apply(model: &mut Model, edits: &mut UndoStack, command: impl EditCommand + 'static) {
    assert!(
        edits.run(model, Box::new(command)),
        "{:?}",
        edits.last_error()
    );
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn rectangle() -> SectionShape {
    SectionShape::SteelFlatBar {
        width: 400.0,
        thick: 400.0,
    }
}

fn fixture(
    shape: SectionShape,
    delta: [f64; 3],
    limits: [[f64; 2]; 2],
    reference: [f64; 3],
    reversed: bool,
) -> Model {
    let mut model = Model::default();
    let mut edits = UndoStack::new();
    for (name, category, young, density, fc, fy) in [
        (
            "鋼材",
            MaterialCategory::Steel,
            205000.0,
            7.85e-9,
            None,
            Some(235.0),
        ),
        (
            "Fc36",
            MaterialCategory::Concrete,
            30000.0,
            2.3e-9,
            Some(36.0),
            None,
        ),
    ] {
        apply(
            &mut model,
            &mut edits,
            AddMaterial {
                name: name.into(),
                category,
                young,
                poisson: 0.3,
                density,
                fc,
                fy,
                strength_factor: None,
                concrete_class: Default::default(),
            },
        );
    }
    let cft = matches!(
        shape,
        SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }
    );
    for (id, shape, usage) in [
        (0, shape, FrameSectionUse::Column),
        (
            1,
            SectionShape::SteelFlatBar {
                width: 100.0,
                thick: 200.0,
            },
            FrameSectionUse::Girder,
        ),
    ] {
        apply(
            &mut model,
            &mut edits,
            AddSectionShape {
                shape,
                new_id: SectionId(id),
                name: format!("断面{id}"),
                floor: None,
                frame_use: Some(usage),
            },
        );
        apply(
            &mut model,
            &mut edits,
            SetSectionMaterial {
                section: SectionId(id),
                role: SectionMaterialRole::Main,
                material: Some(MaterialId(if id == 0 && cft { 1 } else { 0 })),
            },
        );
        if id == 0 && cft {
            apply(
                &mut model,
                &mut edits,
                SetSectionMaterial {
                    section: SectionId(id),
                    role: SectionMaterialRole::Steel,
                    material: Some(MaterialId(0)),
                },
            );
        }
    }
    for coord in [
        [0.0, 0.0, limits[0][0]],
        [0.0, 0.0, limits[0][1]],
        [delta[0], delta[1], limits[1][0]],
        [delta[0], delta[1], limits[1][1]],
    ] {
        apply(
            &mut model,
            &mut edits,
            AddNode {
                coord,
                restraint: Default::default(),
            },
        );
    }
    for (id, a, b) in [(0, 0, 1), (1, 2, 3)] {
        apply(
            &mut model,
            &mut edits,
            AddMember {
                elem: ElementData {
                    id: ElemId(id),
                    kind: ElementKind::Beam,
                    nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
                    section: Some(SectionId(0)),
                    local_axis: LocalAxis {
                        ref_vector: reference,
                    },
                    end_cond: [EndCondition::Fixed; 2],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    vertical_eccentricity: Some(0.0),
                    plastic_zone: None,
                    spring: None,
                },
            },
        );
    }
    let mut anchors = std::array::from_fn(|end| SecondaryMemberAnchor {
        support: SupportMemberId::Primary(ElemId(end as u32)),
        position: (3000.0 + if end == 1 { delta[2] } else { 0.0 } - limits[end][0])
            / (limits[end][1] - limits[end][0]),
    });
    if reversed {
        anchors.reverse();
    }
    let member = SecondaryMember {
        id: SecondaryMemberId(42),
        name: "検証小梁".into(),
        section: Some(SectionId(1)),
        ends: SecondaryMemberEnds::Supported(anchors),
        ..Default::default()
    };
    let before = model.clone();
    apply(
        &mut model,
        &mut edits,
        AddUnassignedBeam { sm: member.clone() },
    );
    assert_eq!(model.secondary_member(member.id), Some(&member));
    assert_eq!(model.elements, before.elements);
    assert_eq!(model.nodes, before.nodes);
    assert_eq!(model.sections, before.sections);
    assert_eq!(model.materials, before.materials);
    assert!(model.validate().is_ok(), "{:?}", model.validate());
    let (a, b, length) = model.secondary_member_axis(&member).unwrap();
    let mut expected = [[0.0, 0.0, 3000.0], [delta[0], delta[1], 3000.0 + delta[2]]];
    if reversed {
        expected.reverse();
    }
    for (actual, expected) in [a, b].into_iter().zip(expected) {
        for k in 0..3 {
            near(actual[k], expected[k]);
        }
    }
    near(length, 6000.0);
    model
}

fn check_design(
    model: &Model,
    distance_mm: f64,
    expected_dl_n: f64,
    expected_faces: [[f64; 3]; 2],
) {
    let before = model.clone();
    let member = model.secondary_member(SecondaryMemberId(42)).unwrap();
    let interval = secondary_self_weight_interval(model, member).unwrap();
    near(interval[0], distance_mm);
    near(interval[1], 6000.0 - distance_mm);
    let (a, b, length) = model.secondary_member_axis(member).unwrap();
    for (s, point) in interval.into_iter().zip(expected_faces) {
        for k in 0..3 {
            near(a[k] + (b[k] - a[k]) * s / length, point[k]);
        }
    }
    near(beam_self_weight_udl(model, member).unwrap().unwrap(), 1.57);
    let result =
        cascade::solve_with_basis(model, |_| 0.0, true, cascade::SelfWeightBasis::Design).unwrap();
    let output = &result.members[&member.id];
    near(output.span, 6000.0);
    assert_eq!(output.member_loads.len(), 1);
    match output.member_loads[0] {
        MemberLoadKind::Distributed { a, b, w1, w2 } => {
            near(a, distance_mm);
            near(b, 6000.0 - distance_mm);
            near(w1, 1.57);
            near(w2, 1.57);
        }
        ref other => panic!("{other:?}"),
    }
    near(output.reactions[0], expected_dl_n / 2.0);
    near(output.reactions[1], expected_dl_n / 2.0);
    let mass = SectionMassProperties::try_from_section(
        &model.sections[1],
        model.secondary_material(member),
        None,
        None,
        None,
    )
    .unwrap();
    near(mass.total_mass(length), 0.942);
    assert_eq!(model.nodes, before.nodes);
    assert_eq!(model.elements, before.elements);
    assert_eq!(model.sections, before.sections);
    assert_eq!(model.materials, before.materials);
    assert_eq!(model.unassigned_beams, before.unassigned_beams);
}

fn assert_rejected(model: &Model, support: u32, reason: &str) {
    let member = model.secondary_member(SecondaryMemberId(42)).unwrap();
    let error = secondary_self_weight_interval(model, member).unwrap_err();
    assert!(
        error.contains(&format!("ElemId({support})")) && error.contains(reason),
        "{error}"
    );
    let error = cascade::solve_with_basis(model, |_| 0.0, true, cascade::SelfWeightBasis::Design)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!("ElemId({support})")) && error.contains(reason),
        "{error}"
    );
}

#[test]
fn 矩形柱の傾斜小梁は実交点と設計自重を保ち上下有限区間外を拒否する() {
    for reversed in [false, true] {
        let model = fixture(
            rectangle(),
            [4800.0, 0.0, 3600.0],
            [[0.0, 4000.0], [0.0, 7000.0]],
            [1.0, 0.0, 0.0],
            reversed,
        );
        let mut faces = [[200.0, 0.0, 3150.0], [4600.0, 0.0, 6450.0]];
        if reversed {
            faces.reverse();
        }
        check_design(&model, 250.0, 8635.0, faces);
        let interval = secondary_self_weight_interval(&model, &model.unassigned_beams[0]).unwrap();
        near(2.0 * (interval[1] - interval[0]), 11000.0);
        for (limits, support) in [
            ([[0.0, 3000.0], [0.0, 7000.0]], 0),
            ([[0.0, 4000.0], [6600.0, 7600.0]], 1),
        ] {
            let outside = fixture(
                rectangle(),
                [4800.0, 0.0, 3600.0],
                limits,
                [1.0, 0.0, 0.0],
                reversed,
            );
            assert_rejected(&outside, support, "有限区間");
        }
    }
}

#[test]
fn 円形と矩形の水平閉端と円形の傾斜は回転に依存せず実外面を使う() {
    for reversed in [false, true] {
        for shape in [rectangle(), SectionShape::SteelRoundBar { dia: 400.0 }] {
            let model = fixture(
                shape,
                [6000.0, 0.0, 0.0],
                [[0.0, 3000.0]; 2],
                [1.0, 0.0, 0.0],
                reversed,
            );
            let mut faces = [[200.0, 0.0, 3000.0], [5800.0, 0.0, 3000.0]];
            if reversed {
                faces.reverse();
            }
            check_design(&model, 200.0, 8792.0, faces);
            let interval =
                secondary_self_weight_interval(&model, &model.unassigned_beams[0]).unwrap();
            near(2.0 * (interval[1] - interval[0]), 11200.0);
        }
        for reference in [[1.0, 0.0, 0.0], [0.6, 0.8, 0.0]] {
            let model = fixture(
                SectionShape::SteelRoundBar { dia: 400.0 },
                [4800.0, 0.0, 3600.0],
                [[0.0, 4000.0], [0.0, 7000.0]],
                reference,
                reversed,
            );
            let mut faces = [[200.0, 0.0, 3150.0], [4600.0, 0.0, 6450.0]];
            if reversed {
                faces.reverse();
            }
            check_design(&model, 250.0, 8635.0, faces);
        }
    }
}

#[test]
fn 回転h柱は近いウェブでなく最外フランジを採りその点の有限高さを検査する() {
    let root101 = 101.0_f64.sqrt();
    for reversed in [false, true] {
        for horizontal in [false, true] {
            let xy = if horizontal { 6000.0 } else { 4800.0 };
            let dz = if horizontal { 0.0 } else { 3600.0 };
            let delta = [xy * 5.2 / root101, xy * 8.6 / root101, dz];
            let shape = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 10.0,
                flange_thick: 20.0,
                root_r: Some(0.0),
            };
            let distance = if horizontal {
                20.0 * root101
            } else {
                25.0 * root101
            };
            let rise = if horizontal { 0.0 } else { 15.0 * root101 };
            let mut faces = [
                [104.0, 172.0, 3000.0 + rise],
                [delta[0] - 104.0, delta[1] - 172.0, 3000.0 + dz - rise],
            ];
            if reversed {
                faces.reverse();
            }
            let model = fixture(
                shape.clone(),
                delta,
                [[0.0, 4000.0], [0.0, 7000.0]],
                [0.6, 0.8, 0.0],
                reversed,
            );
            check_design(&model, distance, 1.57 * (6000.0 - 2.0 * distance), faces);
            if !horizontal {
                assert!(3000.0 + 3.75 * root101 < 3100.0);
                assert!(3000.0 + rise > 3100.0);
                let outside = fixture(
                    shape,
                    delta,
                    [[0.0, 3100.0], [0.0, 7000.0]],
                    [0.6, 0.8, 0.0],
                    reversed,
                );
                assert_rejected(&outside, 0, "有限区間");
            }
        }
    }
}

#[test]
fn 鋼管とcft円柱は空洞内面と材料界面を除外する() {
    for shape in [
        SectionShape::SteelPipe {
            outer_dia: 400.0,
            thick: 20.0,
        },
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 20.0,
        },
    ] {
        for reversed in [false, true] {
            for horizontal in [false, true] {
                let (delta, distance, rise) = if horizontal {
                    ([6000.0, 0.0, 0.0], 200.0, 0.0)
                } else {
                    ([4800.0, 0.0, 3600.0], 250.0, 150.0)
                };
                let model = fixture(
                    shape.clone(),
                    delta,
                    [[0.0, 4000.0], [0.0, 7000.0]],
                    [0.6, 0.8, 0.0],
                    reversed,
                );
                let mut faces = [
                    [200.0, 0.0, 3000.0 + rise],
                    [delta[0] - 200.0, 0.0, 3000.0 + delta[2] - rise],
                ];
                if reversed {
                    faces.reverse();
                }
                check_design(
                    &model,
                    distance,
                    if horizontal { 8792.0 } else { 8635.0 },
                    faces,
                );
            }
        }
    }
}

#[test]
fn 角丸cft柱は実外角を保持し矩形置換と材料界面を採らない() {
    let root2 = 2.0_f64.sqrt();
    let distance = 160.0 * root2 + 40.0;
    let delta = [-1200.0 / root2, 8400.0 / root2, 0.0];
    for reversed in [false, true] {
        let model = fixture(
            SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 20.0,
                corner_r: Some(40.0),
            },
            delta,
            [[0.0, 4000.0]; 2],
            [0.6, 0.8, 0.0],
            reversed,
        );
        let dx = -(32.0 + 4.0 * root2);
        let dy = 224.0 + 28.0 * root2;
        let mut faces = [[dx, dy, 3000.0], [delta[0] - dx, delta[1] - dy, 3000.0]];
        if reversed {
            faces.reverse();
        }
        check_design(&model, distance, 8583.899106263758, faces);
        let expected_mass_t = 0.8583899106263757;
        let expected_weight_n = expected_mass_t * sepika_core::units::GRAVITY_MM_S2;
        let transfer =
            cascade::solve_with_basis(&model, |_| 0.0, true, cascade::SelfWeightBasis::MassEquiv)
                .unwrap();
        let member = &transfer.members[&SecondaryMemberId(42)];
        assert_eq!(
            member.end_points,
            if reversed {
                [[delta[0], delta[1], 3000.0], [0.0, 0.0, 3000.0]]
            } else {
                [[0.0, 0.0, 3000.0], [delta[0], delta[1], 3000.0]]
            }
        );
        near(member.reactions[0], expected_weight_n / 2.0);
        near(member.reactions[1], expected_weight_n / 2.0);
        near(
            member.reactions.iter().sum::<f64>() / sepika_core::units::GRAVITY_MM_S2,
            expected_mass_t,
        );
        let expected_udl_n_per_mm = 0.000157 * sepika_core::units::GRAVITY_MM_S2;
        match member.member_loads.as_slice() {
            [MemberLoadKind::Distributed { a, b, w1, w2 }] => {
                near(*a, distance);
                near(*b, 6000.0 - distance);
                near(*w1, expected_udl_n_per_mm);
                near(*w2, expected_udl_n_per_mm);
            }
            other => panic!("{other:?}"),
        }
        let (nodal, primary) = transfer.primary_loads(&model);
        assert!(nodal.is_empty());
        assert_eq!(primary.len(), 2);
        for (index, load) in primary.iter().enumerate() {
            assert_eq!(load.elem, ElemId(index as u32));
            match load.shape {
                sepika_load::floor::LoadShape::Point { p, x } => {
                    near(p, expected_weight_n / 2.0);
                    near(x, 3000.0);
                }
                ref other => panic!("{other:?}"),
            }
        }
        let mut node_mass_equiv_n = vec![0.0; 4];
        sepika_load::wall_plate_load::accumulate_wall_and_secondary_dl_mass_equiv(
            &model,
            &mut node_mass_equiv_n,
        )
        .unwrap();
        for (actual, fraction) in node_mass_equiv_n.iter().zip([0.125, 0.375, 0.125, 0.375]) {
            near(*actual, expected_weight_n * fraction);
        }
        near(
            node_mass_equiv_n.iter().sum::<f64>() / sepika_core::units::GRAVITY_MM_S2,
            expected_mass_t,
        );
        assert!(distance < 200.0 * root2 && distance > 160.0 * root2 + 20.0);
    }
}

#[test]
fn 平行小梁と形状欠落を支持id付きで拒否し寸法ゼロでも実形状は使う() {
    for reversed in [false, true] {
        let parallel = fixture(
            rectangle(),
            [0.0, 0.0, 6000.0],
            [[0.0, 4000.0], [6000.0, 10000.0]],
            [1.0, 0.0, 0.0],
            reversed,
        );
        assert_rejected(&parallel, if reversed { 1 } else { 0 }, "平行");
        let mut model = fixture(
            rectangle(),
            [4800.0, 0.0, 3600.0],
            [[0.0, 4000.0], [0.0, 7000.0]],
            [1.0, 0.0, 0.0],
            reversed,
        );
        model.sections[0].width = 0.0;
        model.sections[0].depth = 0.0;
        let interval = secondary_self_weight_interval(&model, &model.unassigned_beams[0]).unwrap();
        near(interval[0], 250.0);
        near(interval[1], 5750.0);
        model.sections[0].shape = None;
        assert_rejected(&model, if reversed { 1 } else { 0 }, "実形状が未定義");
    }
}

#[test]
fn 角丸cftの共通大梁と壁フェースも実外角を使い未知角rを推定しない() {
    let root2 = 2.0_f64.sqrt();
    let distance = 160.0 * root2 + 40.0;
    let delta = [-1200.0 / root2, 8400.0 / root2, 0.0];
    for reversed in [false, true] {
        let mut model = fixture(
            SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 20.0,
                corner_r: Some(40.0),
            },
            delta,
            [[0.0, 3000.0]; 2],
            [0.6, 0.8, 0.0],
            reversed,
        );
        let mut edits = UndoStack::new();
        let mut nodes = [NodeId(1), NodeId(3)];
        if reversed {
            nodes.reverse();
        }
        apply(
            &mut model,
            &mut edits,
            AddMember {
                elem: ElementData {
                    id: ElemId(2),
                    kind: ElementKind::Beam,
                    nodes: nodes.into_iter().collect(),
                    section: Some(SectionId(1)),
                    local_axis: LocalAxis {
                        ref_vector: [0.0, 0.0, 1.0],
                    },
                    end_cond: [EndCondition::Fixed; 2],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    vertical_eccentricity: Some(0.0),
                    plastic_zone: None,
                    spring: None,
                },
            },
        );
        let interval = girder_self_weight_interval(&model, &model.elements[2]).unwrap();
        near(interval[0], distance);
        near(interval[1], 6000.0 - distance);
        for sign in [-1.0, 1.0] {
            near(
                wall_support_face(
                    &model,
                    SupportMemberId::Primary(ElemId(0)),
                    [sign * delta[0] / 6000.0, sign * delta[1] / 6000.0, 0.0],
                )
                .unwrap(),
                distance,
            );
        }
        model.sections[0].shape = Some(SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: 20.0,
            corner_r: None,
        });
        let mut pending = model.clone();
        pending.elements.pop();
        assert_rejected(&pending, if reversed { 1 } else { 0 }, "角Rが未知");
        assert!(girder_self_weight_interval(&model, &model.elements[2])
            .unwrap_err()
            .contains("角Rが未知"));
        assert!(
            wall_support_face(&model, SupportMemberId::Primary(ElemId(0)), [1.0, 0.0, 0.0])
                .unwrap_err()
                .contains("角Rが未知")
        );
    }
}

#[test]
fn 水平大梁支持はhウェブ交点でなく既存の投影フランジを使う() {
    for reversed in [false, true] {
        let mut model = fixture(
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 10.0,
                flange_thick: 20.0,
                root_r: Some(0.0),
            },
            [6000.0, 0.0, 0.0],
            [[0.0, 4000.0]; 2],
            [0.6, 0.8, 0.0],
            reversed,
        );
        model.sections[0].frame_use = Some(FrameSectionUse::Girder);
        for (id, coord) in [
            [-3000.0, -4000.0, 3000.0],
            [1000.0, 4000.0 / 3.0, 3000.0],
            [3000.0, -4000.0, 3000.0],
            [7000.0, 4000.0 / 3.0, 3000.0],
        ]
        .into_iter()
        .enumerate()
        {
            model.nodes[id].coord = coord;
        }
        for element in &mut model.elements {
            element.local_axis.ref_vector = [0.0, 0.0, 1.0];
        }
        assert!(model.validate().is_ok());
        let mut faces = [[125.0, 0.0, 3000.0], [5875.0, 0.0, 3000.0]];
        if reversed {
            faces.reverse();
        }
        check_design(&model, 125.0, 9027.5, faces);
    }
}
