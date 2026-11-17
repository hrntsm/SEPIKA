use sepika_core::geom::vec3::local_frame_axes;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::joint_wiring::{collect_joint_checks, ForcesAt};
use sepika_design_jp::{CheckOutcome, LoadTerm};

#[path = "fixtures/rc_joint_model.rs"]
mod fixture;
use fixture::model;

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

fn skipped(model: &Model, forces: &[(ElemId, ForcesAt<'_>)]) -> String {
    match outcome(model, forces).expect("モデルの中央接合部を保持する") {
        CheckOutcome::Skipped { reason } => {
            for required in [
                "検討目的・設計状態",
                "検討方向別",
                "上下柱せん断力",
                "材料・配筋・定着",
            ] {
                assert!(reason.contains(required), "{reason}");
            }
            reason
        }
        result => panic!("成立未確認の終局自動算定は未算定でなければならない: {result:?}"),
    }
}

#[test]
fn symmetric_joint_is_skipped_and_preserves_registration_and_end_order() {
    let mut model = model();
    let result = skipped(&model, &forces());
    model.elements.reverse();
    for (index, element) in model.elements.iter_mut().enumerate() {
        element.id = ElemId(index as u32);
    }
    let registered_forces: Vec<_> = forces()
        .into_iter()
        .map(|(id, force)| (ElemId(3 - id.0), force))
        .collect();
    assert_eq!(skipped(&model, &registered_forces), result);
    for element in &mut model.elements {
        element.nodes.reverse();
    }
    assert_eq!(skipped(&model, &registered_forces), result);
}

#[test]
fn perpendicular_one_sided_beams_are_skipped() {
    let mut model = model();
    let reference = skipped(&model, &forces());
    model.nodes[4].coord = [0.0, 6000.0, 3000.0];
    let result = skipped(&model, &forces());
    assert_eq!(result, reference);
}

#[test]
fn asymmetric_beams_are_skipped_independently_of_force_order() {
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
    let first = skipped(&model, &forces());
    let mut reordered = forces();
    reordered.swap(2, 3);
    let second = skipped(&model, &reordered);
    assert_eq!(first, second);
}

#[test]
fn different_column_sections_are_skipped_independently_of_force_order() {
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
    let lower = skipped(&model, &forces());
    let mut reordered = forces();
    reordered.swap(0, 1);
    let upper = skipped(&model, &reordered);
    assert_eq!(lower, upper);
}

#[test]
fn transverse_column_shear_does_not_enable_ultimate_evaluation() {
    let model = model();
    for element in &model.elements[..2] {
        let axes = local_frame_axes(
            model.node(element.nodes[0]).unwrap().coord,
            model.node(element.nodes[1]).unwrap().coord,
            element.local_axis.ref_vector,
        )
        .unwrap();
        let global_shear =
            std::array::from_fn::<_, 3, _>(|i| 100_000.0 * axes[1][i] + 900_000.0 * axes[2][i]);
        assert_eq!(global_shear, [100_000.0, 900_000.0, 0.0]);
    }
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
    assert_eq!(skipped(&model, &forces), skipped(&model, &self::forces()));
}

#[test]
fn empty_and_omitted_column_forces_preserve_skipped_joint() {
    let model = model();
    let mut forces = forces();
    forces[0].1 = &[];
    forces[1].1 = &[];
    let empty = skipped(&model, &forces);
    assert_eq!(empty, skipped(&model, &self::forces()));
    forces.remove(0);
    assert_eq!(skipped(&model, &forces), empty);
    forces.remove(0);
    assert_eq!(skipped(&model, &forces), empty);
    assert_eq!(skipped(&model, &[]), empty);
}

#[test]
fn missing_rebar_material_or_unset_beam_rebar_is_skipped() {
    let mut model = model();
    model.sections[2].rebar_material = None;
    let result = skipped(&model, &forces());
    assert!(!result.is_empty());
    model.sections[2].rebar_material = Some(MaterialId(1));
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[2].shape else {
        unreachable!()
    };
    rebar.top.clear();
    rebar.bottom.clear();
    let result = skipped(&model, &forces());
    assert!(!result.is_empty());
}

#[test]
fn four_beams_do_not_prove_transverse_restraint() {
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
    skipped(&model, &forces);
}

#[test]
fn missing_concrete_and_all_material_assignments_preserve_joint_candidate() {
    let mut model = model();
    for section in &mut model.sections {
        section.material = None;
        section.rebar_material = None;
        section.shear_rebar_material = None;
    }
    assert_eq!(skipped(&model, &[]), skipped(&self::model(), &forces()));
}

#[test]
fn model_without_rc_column_beam_connection_has_no_ultimate_candidate() {
    let mut model = model();
    model.elements.truncate(2);
    assert!(outcome(&model, &[]).is_none());
}

#[test]
fn explicit_scalar_arithmetic_does_not_establish_model_joint_applicability() {
    use sepika_design_jp::rc::joint::JointShape;
    use sepika_design_jp::ultimate::{rc_joint_ultimate, RcJointUltimateInput};
    let result = rc_joint_ultimate(&RcJointUltimateInput {
        shape: JointShape::Cross,
        phi: 0.85,
        fc: 24.0,
        bj: 500.0,
        dj: 600.0,
        t_top: 677_405.915_930_299,
        t_bottom: 677_405.915_930_299,
        qcu: 100_000.0,
        alpha: 1.0,
    });
    // 径25 mmの円面積×4本×345 N/mm²を各梁の引張力として明示する算術のみ。
    assert!((result.vju - 1_887_026.663).abs() < 0.001);
    assert!((result.qdu - 1_254_811.831_860_598).abs() < 1e-6);
    assert!((result.qdu / result.vju - 0.664_967_727_528_876_9).abs() < 1e-10);
    skipped(&model(), &forces());
}
