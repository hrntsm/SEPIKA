use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};
use sepika_design_jp::joint_wiring::{collect_joint_checks, ForcesAt};
use sepika_design_jp::{CheckOutcome, CheckResult, LoadTerm};

fn model() -> Model {
    let column = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
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
    };
    let beam = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let sections = [column.clone(), column, beam.clone(), beam]
        .into_iter()
        .enumerate()
        .map(|(i, shape)| {
            let mut section = shape.to_section(SectionId(i as u32), format!("断面{i}"));
            section.frame_use = Some(if i < 2 {
                FrameSectionUse::Column
            } else {
                FrameSectionUse::Girder
            });
            section.material = Some(MaterialId(0));
            section.rebar_material = Some(MaterialId(1));
            section.shear_rebar_material = Some(MaterialId(1));
            section
        })
        .collect();
    Model {
        nodes: [
            [0.0, 0.0, 3000.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 6000.0],
            [-6000.0, 0.0, 3000.0],
            [6000.0, 0.0, 3000.0],
        ]
        .into_iter()
        .enumerate()
        .map(|(i, coord)| Node {
            id: NodeId(i as u32),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect(),
        elements: [(1, 0), (0, 2), (3, 0), (0, 4)]
            .into_iter()
            .enumerate()
            .map(|(i, (a, b))| ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
                section: Some(SectionId(i as u32)),
                local_axis: LocalAxis {
                    ref_vector: [0.0, 1.0, 0.0],
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                end_offsets_mm: [[0.0; 3]; 2],
                plastic_zone: None,
                spring: None,
            })
            .collect(),
        sections,
        materials: [
            (MaterialCategory::Concrete, "Fc24", Some(24.0), None),
            (MaterialCategory::Rebar, "SD345", None, Some(345.0)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (category, name, fc, fy))| Material {
            id: MaterialId(i as u32),
            name: name.into(),
            category,
            young: 23000.0,
            poisson: 0.2,
            density: 0.0,
            shear: None,
            fc,
            fy,
            concrete_class: Default::default(),
            strength_factor: None,
        })
        .collect(),
        ..Default::default()
    }
}

const COLUMN: [(f64, [f64; 6]); 2] = [
    (0.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
    (1.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
];
const BEAM: [(f64, [f64; 6]); 2] = [(0.0, [0.0; 6]), (1.0, [0.0; 6])];

fn forces() -> Vec<(ElemId, ForcesAt<'static>)> {
    vec![
        (ElemId(0), &COLUMN),
        (ElemId(1), &COLUMN),
        (ElemId(2), &BEAM),
        (ElemId(3), &BEAM),
    ]
}

fn outcome(model: &Model, forces: &[(ElemId, ForcesAt<'_>)]) -> Option<CheckOutcome> {
    collect_joint_checks(model, forces, LoadTerm::Short)
        .into_iter()
        .find(|(node, label, _)| *node == NodeId(0) && label == "接合部終局(RC)")
        .map(|(_, _, result)| result)
}

fn checked(model: &Model, forces: &[(ElemId, ForcesAt<'_>)]) -> CheckResult {
    match outcome(model, forces).expect("中央接合部の結果") {
        CheckOutcome::Checked(result) => result,
        result => panic!("現行の算定結果を期待: {result:?}"),
    }
}

fn detail_contains(result: &CheckResult, expected: &str) {
    assert!(
        result.components[0].detail.contains(expected),
        "{}",
        result.components[0].detail
    );
}

#[test]
fn symmetric_joint_matches_independent_arithmetic_and_preserves_registration_and_end_order() {
    let mut model = model();
    let result = checked(&model, &forces());
    // 円面積π×12.5²の8本×345=1,354,811.831860598 N、そこから100 kNを差し引く。
    detail_contains(&result, "Qdu=1254811.8 N");
    detail_contains(&result, "Vju=1887026.7 N");
    assert!((result.ratio() - 0.6649677275288769).abs() < 1e-10);
    model.elements.reverse();
    for (index, element) in model.elements.iter_mut().enumerate() {
        element.id = ElemId(index as u32);
    }
    let registered_forces: Vec<_> = forces()
        .into_iter()
        .map(|(id, force)| (ElemId(3 - id.0), force))
        .collect();
    assert_eq!(checked(&model, &registered_forces).ratio(), result.ratio());
    for element in &mut model.elements {
        element.nodes.reverse();
    }
    assert_eq!(checked(&model, &registered_forces).ratio(), result.ratio());
}

#[test]
fn perpendicular_second_beam_still_counts_as_cross_and_same_demand() {
    let mut model = model();
    let reference = checked(&model, &forces());
    model.nodes[4].coord = [0.0, 6000.0, 3000.0];
    let result = checked(&model, &forces());
    detail_contains(&result, "κ=1.00");
    assert_eq!(result.ratio(), reference.ratio());
}

#[test]
fn asymmetric_beam_force_order_changes_width_and_rebar_selection() {
    let mut model = model();
    let SectionShape::RcBeamRect { d, mut rebar, .. } = model.sections[3].shape.clone().unwrap()
    else {
        unreachable!()
    };
    rebar.top = vec![6];
    rebar.bottom = vec![5];
    let mut section =
        SectionShape::RcBeamRect { b: 500.0, d, rebar }.to_section(SectionId(3), "幅広梁".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    model.sections[3] = section;
    let first = checked(&model, &forces());
    let mut reordered = forces();
    reordered.swap(2, 3);
    let second = checked(&model, &reordered);
    detail_contains(&first, "bj=500.0 mm");
    detail_contains(&second, "bj=550.0 mm");
    detail_contains(&second, "Qdu=1762866.3 N");
    assert!(second.ratio() > first.ratio());
}

#[test]
fn upper_column_force_order_changes_adopted_depth_and_capacity() {
    let mut model = model();
    let SectionShape::RcColumnRect { rebar, .. } = model.sections[1].shape.clone().unwrap() else {
        unreachable!()
    };
    let mut section = SectionShape::RcColumnRect {
        b: 800.0,
        d: 800.0,
        rebar,
    }
    .to_section(SectionId(1), "上柱800".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    model.sections[1] = section;
    let lower = checked(&model, &forces());
    let mut reordered = forces();
    reordered.swap(0, 1);
    let upper = checked(&model, &reordered);
    detail_contains(&lower, "Dj=600.0 mm");
    detail_contains(&upper, "Dj=800.0 mm");
    assert!(upper.ratio() < lower.ratio());
}

#[test]
fn transverse_column_shear_is_subtracted_from_the_same_demand() {
    let model = model();
    let mut column = COLUMN;
    for (_, f) in &mut column {
        f[2] = 900_000.0;
    }
    let forces = vec![
        (ElemId(0), column.as_slice()),
        (ElemId(1), column.as_slice()),
        (ElemId(2), &BEAM),
        (ElemId(3), &BEAM),
    ];
    let result = checked(&model, &forces);
    detail_contains(&result, "Qcu=900000.0 N");
    detail_contains(&result, "Qdu=454811.8 N");
    assert!(result.ratio() < checked(&model, &self::forces()).ratio());
}

#[test]
fn empty_column_forces_are_zero_but_omitted_columns_change_shape_or_remove_result() {
    let model = model();
    let mut forces = forces();
    forces[0].1 = &[];
    forces[1].1 = &[];
    let empty = checked(&model, &forces);
    detail_contains(&empty, "Qcu=0.0 N");
    detail_contains(&empty, "κ=1.00");
    forces.remove(0);
    detail_contains(&checked(&model, &forces), "κ=0.70");
    forces.remove(0);
    assert!(outcome(&model, &forces).is_none());
}

#[test]
fn missing_rebar_material_or_unset_beam_rebar_still_produces_zero_demand_checked_result() {
    let mut model = model();
    model.sections[2].rebar_material = None;
    let result = checked(&model, &forces());
    detail_contains(&result, "Qdu=0.0 N");
    assert_eq!(result.ratio(), 0.0);
    model.sections[2].rebar_material = Some(MaterialId(1));
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[2].shape else {
        unreachable!()
    };
    rebar.top.clear();
    rebar.bottom.clear();
    let result = checked(&model, &forces());
    detail_contains(&result, "Qdu=0.0 N");
    assert_eq!(result.ratio(), 0.0);
}

#[test]
fn four_beams_without_both_sided_transverse_restraint_still_receive_phi_one() {
    let mut model = model();
    let mut forces = forces();
    for (id, coord) in [(4, [0.0, 6000.0, 3000.0]), (5, [3000.0, 6000.0, 3000.0])] {
        let mut node = model.nodes[4].clone();
        node.id = NodeId(id + 1);
        node.coord = coord;
        model.nodes.push(node);
        let mut beam = model.elements[3].clone();
        beam.id = ElemId(id);
        beam.nodes = [NodeId(0), NodeId(id + 1)].into_iter().collect();
        model.elements.push(beam);
        forces.push((ElemId(id), &BEAM));
    }
    detail_contains(&checked(&model, &forces), "φ=1.00");
}
