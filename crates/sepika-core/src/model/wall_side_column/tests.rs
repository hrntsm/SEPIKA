use super::*;
use crate::dof::Dof6Mask;
use crate::ids::{MaterialId, SectionId};
use crate::model::{
    EndCondition, ForceRegime, LocalAxis, Material, Node, Section, WallAttr, WallSlit,
};
use crate::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};

fn member(id: u32, kind: ElementKind, nodes: &[u32], section: u32) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind,
        nodes: nodes.iter().map(|n| NodeId(*n)).collect(),
        section: Some(SectionId(section)),
        local_axis: LocalAxis {
            ref_vector: [0., 1., 0.],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    }
}

fn material(id: u32, category: MaterialCategory) -> Material {
    Material {
        id: MaterialId(id),
        name: format!("M{id}"),
        category,
        young: 23000.,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.),
        fy: Some(235.),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn fixture(kind: ElementKind) -> Model {
    let mut model = Model::default();
    for (i, coord) in [
        [0., 0., 0.],
        [4000., 0., 0.],
        [4000., 0., 3000.],
        [0., 0., 3000.],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.materials = vec![
        material(0, MaterialCategory::Concrete),
        material(1, MaterialCategory::Steel),
    ];
    for (i, usage) in [
        None,
        Some(FrameSectionUse::Girder),
        Some(FrameSectionUse::Column),
    ]
    .into_iter()
    .enumerate()
    {
        let mut sec = Section::zero(SectionId(i as u32), format!("S{i}"));
        sec.frame_use = usage;
        sec.material = Some(MaterialId(0));
        sec.thickness = Some(150.);
        model.sections.push(sec);
    }
    model.elements = vec![
        member(0, ElementKind::Wall, &[0, 1, 2, 3], 0),
        member(1, kind, &[0, 1], 1),
        member(2, kind, &[3, 2], 1),
        member(3, kind, &[0, 3], 2),
        member(4, kind, &[1, 2], 2),
    ];
    model
}

fn has(result: &WallSideColumnClassification, reason: WallSideColumnReason) -> bool {
    result.issues.iter().any(|i| i.reason == reason)
}

fn classify(model: &Model, id: u32) -> WallSideColumnClassification {
    WallSideColumnIndex::build(model).classify(
        model.elements.iter().find(|e| e.id == ElemId(id)).unwrap(),
        model,
    )
}

#[test]
fn c00_c01_c02_all_three_representations_have_identical_physical_roles() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let model = fixture(kind);
        let index = WallSideColumnIndex::build(&model);
        assert_eq!(
            index
                .wall_bindings(ElemId(0))
                .map(|b| (b.column, b.side, b.edge))
                .collect::<Vec<_>>(),
            vec![
                (ElemId(3), 0, [NodeId(0), NodeId(3)]),
                (ElemId(4), 1, [NodeId(1), NodeId(2)])
            ]
        );
        let result = classify(&model, 3);
        assert_eq!(
            result.bindings[0].role,
            WallSideColumnRole::RcSrcReplacement
        );
        assert_eq!(result.bindings[0].column_structure, Some(StructureKind::Rc));
        assert_eq!(result.bindings[0].normal, [0., -1., 0.]);
        assert!(result.issues.is_empty(), "{result:?}");
    }
}

#[test]
fn c03_wall_free_frames_are_explicit_non_side_columns() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(kind);
        model.elements.remove(0);
        assert!(has(
            &classify(&model, 3),
            WallSideColumnReason::NoMatchingWall
        ));
        assert!(classify(&model, 3).bindings.is_empty());
    }
}

#[test]
fn c04_horizontal_diagonal_and_remote_members_are_not_side_columns() {
    let mut model = fixture(ElementKind::Beam);
    assert!(has(&classify(&model, 2), WallSideColumnReason::NotVertical));
    model
        .elements
        .push(member(5, ElementKind::Beam, &[0, 2], 2));
    assert!(classify(&model, 5).bindings.is_empty());
    model.nodes.extend([
        Node {
            id: NodeId(4),
            coord: [8000., 0., 0.],
            ..model.nodes[0].clone()
        },
        Node {
            id: NodeId(5),
            coord: [8000., 0., 3000.],
            ..model.nodes[3].clone()
        },
    ]);
    model
        .elements
        .push(member(6, ElementKind::Beam, &[4, 5], 2));
    assert!(has(
        &classify(&model, 6),
        WallSideColumnReason::NoMatchingWall
    ));
}

#[test]
fn c05_exact_both_endpoint_ids_are_required() {
    let mut model = fixture(ElementKind::Beam);
    for id in [4, 5] {
        model.nodes.push(Node {
            id: NodeId(id),
            ..model.nodes[if id == 4 { 0 } else { 3 }].clone()
        });
    }
    for nodes in [[4, 5], [0, 5], [4, 3]] {
        model.elements[3].nodes = nodes.into_iter().map(NodeId).collect();
        assert!(has(
            &classify(&model, 3),
            WallSideColumnReason::NoMatchingWall
        ));
    }
    model.elements[3].nodes = [NodeId(3), NodeId(0)].into_iter().collect();
    assert_eq!(classify(&model, 3).bindings.len(), 1);
}

#[test]
fn c06_inactive_walls_retain_candidates_and_reasons() {
    for variant in 0..4 {
        let mut model = fixture(ElementKind::Beam);
        if variant == 0 {
            model.sections[0].thickness = Some(119.);
        }
        if variant == 1 || variant == 2 {
            model.wall_attrs.push(WallAttr {
                elem: ElemId(0),
                opening_area: if variant == 1 { 3e6 } else { 0. },
                opening_weight: 0.,
                slit: WallSlit {
                    column_face: [variant == 2, false],
                    beam_face: [false; 2],
                },
                openings: vec![],
                finish_intensity: 0.,
            });
        }
        if variant == 3 {
            model.elements.remove(2);
        }
        let result = classify(&model, 3);
        assert_eq!(result.bindings[0].role, WallSideColumnRole::InactiveWall);
        assert!(has(&result, WallSideColumnReason::WallNotSeismic));
    }
}

#[test]
fn c07_no_generated_wall_and_five_node_wall_never_release_columns() {
    let mut model = fixture(ElementKind::Beam);
    model.elements[0].nodes.push(NodeId(0));
    let index = WallSideColumnIndex::build(&model);
    assert!(index.bindings().is_empty());
    assert_eq!(
        index.walls()[0].issues[0].reason,
        WallSideColumnReason::InvalidWallGeometry
    );
    model.elements.remove(0);
    assert!(WallSideColumnIndex::build(&model).walls().is_empty());
}

#[test]
fn c08_missing_material_and_inconsistent_shapes_never_default_to_rc() {
    for wall in [false, true] {
        let mut model = fixture(ElementKind::Beam);
        model.sections[if wall { 0 } else { 2 }].material = None;
        let result = classify(&model, 3);
        assert!(has(&result, WallSideColumnReason::MissingMaterial));
        if wall {
            assert_eq!(result.bindings[0].wall_structure, None);
            assert_eq!(result.bindings[0].role, WallSideColumnRole::Unresolved);
        } else {
            assert_eq!(result.bindings[0].column_structure, None);
        }
    }
    let mut model = fixture(ElementKind::Beam);
    model.sections[0].shape = Some(SectionShape::RcWall {
        thickness: 150.,
        ps: 0.0025,
        pwh_ratio: None,
        pwv_ratio: None,
    });
    model.sections[0].material = Some(MaterialId(1));
    assert!(has(
        &classify(&model, 3),
        WallSideColumnReason::ShapeMaterialMismatch
    ));
    for usage in [
        None,
        Some(FrameSectionUse::Girder),
        Some(FrameSectionUse::Brace),
    ] {
        let mut model = fixture(ElementKind::Beam);
        model.sections[2].frame_use = usage;
        assert!(has(
            &classify(&model, 3),
            WallSideColumnReason::ColumnUseRequired
        ));
    }
    let mut model = fixture(ElementKind::Beam);
    model.sections[2].material = Some(MaterialId(99));
    let result = classify(&model, 3);
    assert_eq!(result.bindings[0].column_structure, None);
    assert!(has(&result, WallSideColumnReason::MissingMaterial));
}

#[test]
fn general_section_geometry_does_not_override_explicit_primary_material() {
    let mut model = fixture(ElementKind::Beam);
    model.sections[2].shape = Some(SectionShape::SteelH {
        height: 600.,
        width: 400.,
        web_thick: 12.,
        flange_thick: 20.,
        root_r: Some(0.),
    });
    let result = classify(&model, 3);
    assert_eq!(result.bindings[0].column_structure, Some(StructureKind::Rc));
    assert!(result.issues.is_empty());
}

#[test]
fn brace_and_wrong_girder_usage_are_reported_without_discarding_candidates() {
    let mut model = fixture(ElementKind::Beam);
    model.elements[3].kind = ElementKind::Brace {
        tension_only: false,
    };
    model.sections[1].frame_use = Some(FrameSectionUse::Column);
    let result = classify(&model, 3);
    assert_eq!(result.bindings.len(), 1);
    assert!(has(&result, WallSideColumnReason::NotFrameMember));
    assert!(has(&result, WallSideColumnReason::GirderUseRequired));
}

#[test]
fn c09_duplicate_candidates_report_all_stable_ids() {
    let mut model = fixture(ElementKind::Beam);
    model
        .elements
        .push(member(8, ElementKind::Fiber, &[3, 0], 2));
    model
        .elements
        .push(member(7, ElementKind::MultiSpring, &[0, 3], 2));
    let index = WallSideColumnIndex::build(&model);
    assert_eq!(
        index
            .wall_bindings(ElemId(0))
            .filter(|b| b.side == 0)
            .map(|b| b.column)
            .collect::<Vec<_>>(),
        vec![ElemId(3), ElemId(7), ElemId(8)]
    );
    let result = classify(&model, 3);
    assert_eq!(
        result
            .issues
            .iter()
            .find(|i| i.reason == WallSideColumnReason::DuplicateColumns)
            .unwrap()
            .members,
        vec![ElemId(3), ElemId(7), ElemId(8)]
    );
    model.elements.reverse();
    assert_eq!(classify(&model, 3), result);
}

fn add_shared_wall(model: &mut Model, perpendicular: bool, reverse: bool) {
    let x = if perpendicular { 0. } else { -4000. };
    let y = if perpendicular { 4000. } else { 0. };
    model.nodes.push(Node {
        id: NodeId(4),
        coord: [x, y, 0.],
        ..model.nodes[0].clone()
    });
    model.nodes.push(Node {
        id: NodeId(5),
        coord: [x, y, 3000.],
        ..model.nodes[3].clone()
    });
    model.elements.push(member(
        5,
        ElementKind::Wall,
        if reverse {
            &[0, 4, 5, 3]
        } else {
            &[4, 0, 3, 5]
        },
        0,
    ));
    model
        .elements
        .push(member(6, ElementKind::Beam, &[4, 0], 1));
    model
        .elements
        .push(member(7, ElementKind::Beam, &[5, 3], 1));
}

#[test]
fn c10_c11_parallel_shared_walls_accept_opposite_normals_and_registration_order() {
    for reverse in [false, true] {
        let mut model = fixture(ElementKind::Beam);
        add_shared_wall(&mut model, false, reverse);
        let result = classify(&model, 3);
        assert_eq!(
            result.bindings.iter().map(|b| b.wall).collect::<Vec<_>>(),
            vec![ElemId(0), ElemId(5)]
        );
        assert!(result.issues.is_empty(), "{result:?}");
        assert_eq!(
            result.bindings[1].normal,
            if reverse { [0., 1., 0.] } else { [0., -1., 0.] }
        );
        model.elements.reverse();
        assert_eq!(classify(&model, 3), result);
    }
}

#[test]
fn c12_nonparallel_shared_walls_report_all_wall_ids_in_stable_order() {
    let mut model = fixture(ElementKind::Beam);
    add_shared_wall(&mut model, true, false);
    let result = classify(&model, 3);
    let entry = result
        .issues
        .iter()
        .find(|i| i.reason == WallSideColumnReason::NonParallelWalls)
        .unwrap();
    assert_eq!(entry.walls, vec![ElemId(0), ElemId(5)]);
    assert_eq!(entry.members, vec![ElemId(3)]);
    model.elements.reverse();
    assert_eq!(classify(&model, 3), result);
}

fn src_shape() -> SectionShape {
    SectionShape::SrcColumnRect {
        b: 600.,
        d: 600.,
        rebar: RcRectColumnRebar {
            main_dia: 22.,
            x: vec![4],
            y: vec![4],
            cover: 40.,
            hoop: RectColumnHoop {
                dia: 10.,
                pitch: 100.,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 300.,
        steel_width: 200.,
        steel_web_thick: 8.,
        steel_flange_thick: 12.,
    }
}

#[test]
fn c13_c14_src_is_recognized_for_symmetric_and_one_sided_frames() {
    for asymmetric in [false, true] {
        let mut model = fixture(ElementKind::Beam);
        model.sections[2].shape = Some(src_shape());
        model.sections[2].steel_material = Some(MaterialId(1));
        if asymmetric {
            let mut sec = model.sections[2].clone();
            sec.id = SectionId(3);
            sec.shape = None;
            model.sections.push(sec);
            model.elements[4].section = Some(SectionId(3));
        } else {
            model.sections[1].shape = Some(SectionShape::SrcBeamRect {
                b: 600.,
                d: 600.,
                rebar: RcBeamRebar {
                    main_dia: 22.,
                    top: vec![4],
                    bottom: vec![4],
                    cover: 40.,
                    stirrup: BeamStirrup {
                        dia: 10.,
                        pitch: 100.,
                        legs: 2,
                    },
                },
                steel_height: 300.,
                steel_width: 200.,
                steel_web_thick: 8.,
                steel_flange_thick: 12.,
            });
            model.sections[1].steel_material = Some(MaterialId(1));
        }
        let result = classify(&model, 3);
        assert_eq!(
            result.bindings[0].column_structure,
            Some(StructureKind::Src)
        );
        assert_eq!(
            result.bindings[0].role,
            WallSideColumnRole::RcSrcReplacement
        );
        assert!(result.issues.is_empty(), "{result:?}");
        assert_eq!(
            classify(&model, 4).bindings[0].column_structure,
            Some(if asymmetric {
                StructureKind::Rc
            } else {
                StructureKind::Src
            })
        );
    }
}

#[test]
fn c15_cft_recognition_does_not_claim_qu_or_nonlinear_applicability() {
    let mut model = fixture(ElementKind::MultiSpring);
    model.sections[2].shape = Some(SectionShape::CftBox {
        height: 600.,
        width: 600.,
        thick: 16.,
        corner_r: Some(32.),
    });
    model.sections[2].steel_material = Some(MaterialId(1));
    let result = classify(&model, 3);
    assert_eq!(
        result.bindings[0].column_structure,
        Some(StructureKind::Cft)
    );
    assert_eq!(
        result.bindings[0].role,
        WallSideColumnRole::RcSrcReplacement
    );
    assert!(result.issues.is_empty());
    model.sections[2].steel_material = None;
    assert!(has(
        &classify(&model, 3),
        WallSideColumnReason::MissingSteelMaterial
    ));
    model.sections[2].material = None;
    let result = classify(&model, 3);
    assert_eq!(
        result.bindings[0].column_structure,
        Some(StructureKind::Cft)
    );
    assert!(has(&result, WallSideColumnReason::MissingMaterial));
}

#[test]
fn c16_rc_wall_with_steel_column_or_girder_is_material_conflict() {
    for section in [1, 2] {
        let mut model = fixture(ElementKind::Beam);
        model.sections[section].material = Some(MaterialId(1));
        assert!(has(
            &classify(&model, 3),
            WallSideColumnReason::FrameMaterialMismatch
        ));
    }
}

#[test]
fn c17_c18_steel_plate_boundaries_never_take_rc_replacement_role() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(kind);
        for section in &mut model.sections {
            section.material = Some(MaterialId(1));
        }
        model.sections[0].thickness = Some(9.);
        let result = classify(&model, 3);
        assert_eq!(
            result.bindings[0].role,
            WallSideColumnRole::SteelPlateBoundary
        );
        assert!(result.issues.is_empty());
    }
}

#[test]
fn c19_embedded_steel_in_concrete_wall_is_not_pure_steel_plate() {
    let mut model = fixture(ElementKind::Beam);
    model.sections[0].shape = Some(SectionShape::RcWall {
        thickness: 150.,
        ps: 0.0025,
        pwh_ratio: None,
        pwv_ratio: None,
    });
    model.sections[0].steel_material = Some(MaterialId(1));
    assert_eq!(
        classify(&model, 3).bindings[0].role,
        WallSideColumnRole::RcSrcReplacement
    );
}

#[test]
fn c20_steel_wall_yield_input_error_is_retained_separately_from_role() {
    for fy in [None, Some(0.), Some(-1.), Some(f64::NAN)] {
        let mut model = fixture(ElementKind::Beam);
        for section in &mut model.sections {
            section.material = Some(MaterialId(1));
        }
        model.materials[1].fy = fy;
        let result = classify(&model, 3);
        assert_eq!(
            result.bindings[0].role,
            WallSideColumnRole::SteelPlateBoundary
        );
        assert!(has(&result, WallSideColumnReason::InvalidSteelYield));
    }
}

#[test]
fn c21_element_and_node_id_permutation_preserves_normalized_roles() {
    let model = fixture(ElementKind::Fiber);
    let reference = classify(&model, 3);
    let mut permuted = model.clone();
    let ids = [2, 0, 3, 1];
    let element_ids = [4, 2, 0, 1, 3];
    for node in &mut permuted.nodes {
        node.id = NodeId(ids[node.id.index()]);
    }
    permuted.nodes.sort_by_key(|n| n.id);
    for element in &mut permuted.elements {
        element.id = ElemId(element_ids[element.id.index()]);
        for node in &mut element.nodes {
            *node = NodeId(ids[node.index()]);
        }
    }
    permuted.elements.reverse();
    let actual = classify(&permuted, 1);
    assert_eq!(actual.column, ElemId(1));
    assert_eq!(actual.bindings[0].wall, ElemId(4));
    assert_eq!(actual.bindings[0].role, reference.bindings[0].role);
    assert_eq!(actual.bindings[0].column_structure, Some(StructureKind::Rc));
    assert_eq!(actual.bindings[0].edge, [NodeId(2), NodeId(1)]);
    assert_eq!(actual.bindings[0].normal, reference.bindings[0].normal);
    assert!(actual.issues.is_empty());
}

#[test]
fn inactive_wall_girder_usage_does_not_poison_active_replacement() {
    let mut model = fixture(ElementKind::Fiber);
    add_shared_wall(&mut model, false, true);
    let mut inactive = model.sections[0].clone();
    inactive.id = SectionId(3);
    inactive.thickness = Some(100.0);
    model.sections.push(inactive);
    model
        .elements
        .iter_mut()
        .find(|e| e.id == ElemId(5))
        .unwrap()
        .section = Some(SectionId(3));
    let mut girder = model.sections[1].clone();
    girder.id = SectionId(4);
    girder.frame_use = None;
    model.sections.push(girder);
    for e in model
        .elements
        .iter_mut()
        .filter(|e| [ElemId(6), ElemId(7)].contains(&e.id))
    {
        e.section = Some(SectionId(4));
    }
    let index = WallSideColumnIndex::build(&model);
    let classification = index.classify(&model.elements[3], &model);
    assert!(has(&classification, WallSideColumnReason::WallNotSeismic));
    assert!(has(
        &classification,
        WallSideColumnReason::GirderUseRequired
    ));
    let selected = index.replacement_bindings(ElemId(3)).unwrap();
    assert_eq!(
        selected.iter().map(|b| b.wall).collect::<Vec<_>>(),
        vec![ElemId(0)]
    );
    assert_eq!(index.wall_replacement_bindings(ElemId(0)).unwrap().len(), 2);
    assert!(index
        .wall_replacement_bindings(ElemId(5))
        .unwrap()
        .is_empty());
}

#[test]
fn unknown_material_on_inactive_shared_wall_is_not_hidden() {
    let mut model = fixture(ElementKind::Fiber);
    add_shared_wall(&mut model, false, true);
    let mut unknown = model.sections[0].clone();
    unknown.id = SectionId(3);
    unknown.material = None;
    unknown.thickness = Some(100.0);
    model.sections.push(unknown);
    model
        .elements
        .iter_mut()
        .find(|e| e.id == ElemId(5))
        .unwrap()
        .section = Some(SectionId(3));
    let index = WallSideColumnIndex::build(&model);
    assert_eq!(
        index
            .walls()
            .iter()
            .find(|w| w.wall == ElemId(5))
            .unwrap()
            .role,
        WallSideColumnRole::Unresolved
    );
    assert!(index
        .replacement_bindings(ElemId(3))
        .unwrap_err()
        .iter()
        .any(|i| i.reason == WallSideColumnReason::MissingMaterial && i.walls == [ElemId(5)]));
    assert!(index.wall_replacement_bindings(ElemId(0)).is_err());
}

#[test]
fn invalid_nonparallel_candidate_retains_cause_without_plane_conflict() {
    let mut model = fixture(ElementKind::Beam);
    add_shared_wall(&mut model, true, false);
    let mut girder = model.sections[1].clone();
    girder.id = SectionId(3);
    girder.material = None;
    model.sections.push(girder);
    model
        .elements
        .iter_mut()
        .find(|e| e.id == ElemId(6))
        .unwrap()
        .section = Some(SectionId(3));
    let index = WallSideColumnIndex::build(&model);
    let errors = index.replacement_bindings(ElemId(3)).unwrap_err();
    assert!(errors
        .iter()
        .any(|i| i.reason == WallSideColumnReason::MissingMaterial));
    assert!(!errors
        .iter()
        .any(|i| i.reason == WallSideColumnReason::NonParallelWalls));
    assert!(index.wall_replacement_bindings(ElemId(5)).is_err());
}

#[test]
fn invalid_only_candidates_are_not_selected_as_no_matching_wall() {
    let mut model = fixture(ElementKind::MultiSpring);
    model.sections[2].frame_use = None;
    let index = WallSideColumnIndex::build(&model);
    let errors = index.replacement_bindings(ElemId(3)).unwrap_err();
    assert!(errors
        .iter()
        .any(|i| i.reason == WallSideColumnReason::ColumnUseRequired));
    assert!(!errors
        .iter()
        .any(|i| i.reason == WallSideColumnReason::NoMatchingWall));
}
