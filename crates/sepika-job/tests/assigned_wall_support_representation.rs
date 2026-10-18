use sepika_core::dof::Dof6Mask;
use sepika_core::ids::*;
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_job::{prepare::prepare_model_for_analysis, settings::AnalysisSettings};

fn fixture(kind: ElementKind) -> Model {
    let mut model = Model {
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    };
    model.materials.push(Material {
        id: MaterialId(0),
        name: "鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205000.,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    for (i, usage) in [FrameSectionUse::Girder, FrameSectionUse::Column]
        .into_iter()
        .enumerate()
    {
        let mut section = SectionShape::SteelH {
            height: 400.,
            width: 200.,
            web_thick: 8.,
            flange_thick: 13.,
            root_r: Some(0.),
        }
        .to_section(SectionId(i as u32), format!("H{i}"));
        section.frame_use = Some(usage);
        section.material = Some(MaterialId(0));
        model.sections.push(section);
    }
    model.materials.push(Material {
        id: MaterialId(1),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut wall = SectionShape::RcWall {
        thickness: 150.,
        ps: 0.002,
        pwh_ratio: None,
        pwv_ratio: None,
    }
    .to_section(SectionId(2), "壁t150".into());
    wall.material = Some(MaterialId(1));
    wall.shape = None;
    model.sections.push(wall);
    for (i, coord) in [
        [0., 0., 0.],
        [4000., 0., 0.],
        [4000., 0., 3000.],
        [0., 0., 3000.],
        [0., 0., 4500.],
        [4000., 0., 4500.],
        [0., -1000., 4500.],
        [4000., -1000., 4500.],
        [4000., 1000., 4500.],
        [0., 1000., 4500.],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: if i < 2 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, [a, b]) in [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [3, 4],
        [2, 5],
        [6, 7],
        [7, 5],
        [8, 9],
        [9, 4],
        [5, 8],
        [4, 6],
    ]
    .into_iter()
    .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: if i == 2 { kind } else { ElementKind::Beam },
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(if [1, 3, 4, 5].contains(&i) { 1 } else { 0 })),
            local_axis: LocalAxis {
                ref_vector: if i < 6 { [0., 1., 0.] } else { [0., 0., 1.] },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(4)),
                position: 1.,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(5)),
                position: 1.,
            },
        ]),
        section: Some(SectionId(0)),
        ..Default::default()
    });
    model.rebuild_assignment_regions().unwrap();
    assert_eq!(model.floor_assignment_regions.regions.len(), 2);
    for region in &mut model.floor_assignment_regions.regions {
        region.assignment = PlateAssignment::NoPlate;
    }
    model.wall_plates.push(WallPlate {
        id: WallPlateId(0),
        shape: WallPlateShape::Enclosed,
        section: Some(SectionId(2)),
        opening_area: 0.,
        opening_weight: 0.,
        openings: vec![],
        loads: vec![AreaLoad {
            kind: "仕上げ".into(),
            value: 0.0025,
        }],
        slit: WallSlit {
            column_face: [true, true],
            beam_face: [true, false],
        },
        self_weight_shares: vec![],
        dl_support: None,
    });
    for region in &mut model.wall_assignment_regions.regions {
        region.assignment = if region
            .boundary
            .iter()
            .any(|edge| edge.support == SupportMemberId::Secondary(SecondaryMemberId(0)))
        {
            PlateAssignment::Plate(WallPlateId(0))
        } else {
            PlateAssignment::NoPlate
        };
    }
    model
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

fn resultant(model: &Model, case: &LoadCase, source: Option<LoadSource>) -> [f64; 4] {
    let mut sum = [0.; 4];
    for load in &case.nodal {
        if source.is_some_and(|source| source != load.source) {
            continue;
        }
        let force = -load.values[2];
        sum[0] += force;
        for (axis, coord) in model.node(load.node).unwrap().coord.into_iter().enumerate() {
            sum[axis + 1] += force * coord;
        }
    }
    for load in &case.member {
        if source.is_some_and(|source| source != load.source) {
            continue;
        }
        assert_eq!(load.dir, [0., 0., -1.]);
        let element = model.element(load.elem).unwrap();
        let start = model.node(element.nodes[0]).unwrap().coord;
        let end = model.node(element.nodes[1]).unwrap().coord;
        let length = model.member_length(element);
        let (force, first) = match load.kind {
            MemberLoadKind::Point { a, p } => (p, p * a),
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                let force = (b - a) * (w1 + w2) / 2.;
                (force, a * force + (b - a).powi(2) * (w1 + 2. * w2) / 6.)
            }
        };
        sum[0] += force;
        for axis in 0..3 {
            sum[axis + 1] += force * start[axis] + first * (end[axis] - start[axis]) / length;
        }
    }
    sum
}

fn dl(model: &Model) -> &LoadCase {
    model
        .load_cases
        .iter()
        .find(|case| case.name == DL_CASE_NAME)
        .unwrap()
}

#[test]
fn assigned_secondary_supported_wall_keeps_slits_boundary_and_upper_reactions_for_each_kind() {
    let reference = bincode::serialize(&fixture(ElementKind::Beam)).unwrap();
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(kind);
        model.validate().unwrap();
        let mut same_input = model.clone();
        same_input.elements[2].kind = ElementKind::Beam;
        assert_eq!(bincode::serialize(&same_input).unwrap(), reference);
        let assignments = model.wall_assignment_regions.clone();
        let floor_assignments = model.floor_assignment_regions.clone();
        let plate = model.wall_plates[0].clone();
        let nodes = model.nodes.clone();
        let ends = model.secondary_member(SecondaryMemberId(0)).unwrap().ends;
        let boundary = plate.boundary_nodes(&model).unwrap();
        close(plate.area(&model), 6_000_000.);
        model.rebuild_assignment_regions().unwrap();
        let report = sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut model);
        assert_eq!(report.unassigned_wall_plates, 1);
        assert_eq!(model.wall_plates[0], plate);
        assert_eq!(model.wall_assignment_regions, assignments);
        assert_eq!(model.nodes, nodes);
        assert!(model
            .wall_regions
            .iter()
            .all(|region| !region.wall_plate_ids.contains(&plate.id)));
        let mut control = model.clone();
        control.wall_plates.clear();
        for region in &mut control.wall_assignment_regions.regions {
            region.assignment = PlateAssignment::NoPlate;
        }
        prepare_model_for_analysis(&mut control, &AnalysisSettings::default(), None).unwrap();
        close(resultant(&control, dl(&control), None)[0], 21_216.231424);
        for _ in 0..2 {
            let report =
                prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            assert!(!report.nodes_renumbered);
            model.validate().unwrap();
            assert_eq!(model.wall_plates[0], plate);
            assert_eq!(model.wall_assignment_regions, assignments);
            assert_eq!(model.floor_assignment_regions, floor_assignments);
            assert_eq!(model.nodes.len(), nodes.len());
            for (node, original) in model.nodes.iter().zip(&nodes) {
                assert_eq!(node.id, original.id);
                assert_eq!(node.coord, original.coord);
            }
            assert_eq!(
                model.wall_plates[0].boundary_nodes(&model).unwrap(),
                boundary
            );
            assert_eq!(
                model.secondary_member(SecondaryMemberId(0)).unwrap().ends,
                ends
            );
            assert!(model.floor_regions.iter().any(|region| region
                .secondary_beams
                .iter()
                .any(|beam| beam.id == SecondaryMemberId(0))));
            let case = dl(&model);
            let wall = resultant(&model, case, Some(LoadSource::WallSelfWeight));
            for (actual, expected) in
                wall.into_iter()
                    .zip([36_182.364, 72_364_728., 0., 162_820_638.])
            {
                close(actual, expected);
            }
            close(
                resultant(&model, case, None)[0] - resultant(&control, dl(&control), None)[0],
                36_182.364,
            );
            assert!(!case
                .member
                .iter()
                .any(|load| load.elem == ElemId(2) && load.source == LoadSource::WallSelfWeight));
            for id in [NodeId(4), NodeId(5)] {
                close(
                    case.nodal
                        .iter()
                        .filter(|load| load.node == id && load.source == LoadSource::WallSelfWeight)
                        .map(|load| -load.values[2])
                        .sum(),
                    18_091.182,
                );
            }
            let issues = sepika_solver::statics::analysis::precheck::model_issues(&model);
            assert!(issues
                .iter()
                .all(|issue| !issue.message.contains("耐震スリットの指定が効かない")));
            assert!(
                issues.iter().all(|issue| issue.severity
                    != sepika_solver::statics::analysis::precheck::IssueSeverity::Error),
                "{:?}",
                issues
                    .iter()
                    .map(|issue| &issue.message)
                    .collect::<Vec<_>>()
            );
            let result =
                sepika_job::compute::compute_linear_static(model.clone(), case.id).unwrap();
            assert_eq!(result.disp.len(), 10);
            assert!(result.disp.iter().flatten().all(|value| value.is_finite()));
        }
    }
}

#[test]
fn assigned_wall_preserves_explicit_dl_modes_and_arbitrary_rates_without_slits() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for (mode, lower_fraction) in [
            (WallDlSupport::LowerBeam, 1.),
            (WallDlSupport::UpperBeam, 0.),
            (WallDlSupport::HeightMidpoint, 0.5),
        ] {
            let mut model = fixture(kind);
            model.wall_plates[0].slit = Default::default();
            model.wall_plates[0].dl_support = Some(mode);
            let before = model.wall_plates[0].clone();
            let assignment = model.wall_assignment_regions.clone();
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            assert_eq!(model.wall_plates[0], before);
            assert_eq!(model.wall_assignment_regions, assignment);
            let wall = resultant(&model, dl(&model), Some(LoadSource::WallSelfWeight));
            close(wall[0], 36_182.364);
            close(
                wall[3],
                36_182.364 * (3000. * lower_fraction + 4500. * (1. - lower_fraction)),
            );
        }
        let mut model = fixture(kind);
        model.wall_plates[0].slit = Default::default();
        let boundary = model
            .wall_plate_assignment_region(WallPlateId(0))
            .unwrap()
            .boundary
            .clone();
        model.wall_plates[0].self_weight_shares = boundary
            .iter()
            .map(|edge| {
                if edge.support == SupportMemberId::Primary(ElemId(2)) {
                    0.75
                } else if edge.support == SupportMemberId::Secondary(SecondaryMemberId(0)) {
                    0.25
                } else {
                    0.
                }
            })
            .collect();
        let before = model.wall_plates[0].clone();
        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        assert_eq!(model.wall_plates[0], before);
        let wall = resultant(&model, dl(&model), Some(LoadSource::WallSelfWeight));
        close(wall[0], 36_182.364);
        close(wall[3], 36_182.364 * 3375.);
        model.wall_plates[0].self_weight_shares[0] = -1.;
        assert!(
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).is_err()
        );
    }
}

#[test]
fn invalid_wall_assignments_and_slit_selections_are_rejected_without_shape_conversion() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(kind);
        model.wall_plates[0].slit.beam_face = [true, true];
        assert!(
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).is_err()
        );
        assert_eq!(model.wall_plates[0].shape, WallPlateShape::Enclosed);

        let mut model = fixture(kind);
        let id = model
            .wall_plate_assignment_region(WallPlateId(0))
            .unwrap()
            .id;
        model
            .wall_assignment_regions
            .get_mut(id)
            .unwrap()
            .assignment = PlateAssignment::Unset;
        assert!(
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).is_err()
        );
        assert_eq!(model.wall_plates[0].shape, WallPlateShape::Enclosed);
    }
}

#[test]
fn existing_attached_wall_keeps_its_shape_and_public_solver_still_rejects_isolated_nodes() {
    let mut model = fixture(ElementKind::Beam);
    let id = model
        .wall_plate_assignment_region(WallPlateId(0))
        .unwrap()
        .id;
    model
        .wall_assignment_regions
        .get_mut(id)
        .unwrap()
        .assignment = PlateAssignment::Unset;
    model.wall_plates[0].shape = WallPlateShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [NodeId(3), NodeId(2)],
            span: [0., 1.],
            transfer: LoadTransfer::Anchor,
        },
        extent: Some([1500., 1500.]),
    };
    let plate = model.wall_plates[0].clone();
    let report =
        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
    assert_eq!(model.wall_plates[0], plate);
    assert!(report
        .notices
        .iter()
        .any(|notice| notice.contains("未設定")));
    close(
        resultant(&model, dl(&model), Some(LoadSource::WallSelfWeight))[0],
        36_182.364,
    );
    assert!(
        sepika_solver::statics::analysis::precheck::model_issues(&model)
            .iter()
            .any(
                |issue| issue.message.contains("耐震スリットの指定が効かない")
                    && issue.severity
                        == sepika_solver::statics::analysis::precheck::IssueSeverity::Warning
            )
    );
    model.nodes.push(Node {
        id: NodeId(10),
        coord: [2000., 0., 6000.],
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    });
    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
    assert_eq!(model.nodes.len(), 11);
    let error =
        sepika_job::compute::compute_linear_static(model.clone(), dl(&model).id).unwrap_err();
    assert!(error.to_string().contains("接続されていない"), "{error}");
}
