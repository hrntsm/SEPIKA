use sepika_core::dof::Dof6Mask;
use sepika_core::ids::*;
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_core::units::GRAVITY_MM_S2;
use sepika_job::auto_loads::{apply_auto_load_cases, compute_gravity_auto_load_cases};
use sepika_load::story_gen::{
    generate_stories, generate_stories_with_opts, generate_stories_with_synced_self_weight,
};

fn fixture(top: f64) -> Model {
    let mut model = Model::default();
    for (i, coord) in [
        [0.0, 0.0, 3000.0],
        [4000.0, 0.0, 3000.0],
        [4000.0, 0.0, top],
        [0.0, 0.0, top],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 9000.0],
        [0.0, 0.0, 6000.0],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: if i == 4 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    let mut section = SectionShape::RcWall {
        thickness: 200.0,
        pwh_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(0), "壁".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    let mut support = model.sections[0].clone();
    support.id = SectionId(1);
    support.shape = None;
    support.area = 0.0;
    support.width = 0.0;
    support.depth = 0.0;
    support.thickness = None;
    model.sections.push(support);
    model.materials.push(Material {
        id: MaterialId(0),
        name: "検証γ24".into(),
        category: MaterialCategory::Concrete,
        young: 24000.0,
        poisson: 0.2,
        density: 24e-6 / GRAVITY_MM_S2,
        shear: None,
        fc: Some(24.0),
        fy: None,
        strength_factor: None,
        concrete_class: Default::default(),
    });
    for (i, [a, b]) in [[0, 1], [1, 2], [2, 3], [3, 0]].into_iter().enumerate() {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(1)),
            local_axis: LocalAxis {
                ref_vector: if a == 1 || a == 3 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(0)),
            self_weight_shares: vec![],
            dl_support: Some(WallDlSupport::HeightMidpoint),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
        },
    );
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "検証構面".into(),
        boundary: vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        wall_plate_ids: vec![WallPlateId(0)],
        posts: vec![],
    });
    model
}
fn opening(x: f64, z: f64) -> WallOpening {
    WallOpening {
        width: 2000.0,
        height: 1000.0,
        offset: Some([x, z]),
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "actual {a}, expected {b}");
}
fn bands(model: &Model, expected: &[f64]) {
    let gen = generate_stories(model, None).unwrap();
    for (s, &w) in gen.stories.iter().zip(expected) {
        close(s.seismic_weight.unwrap(), w);
        close(s.dynamic_mass.unwrap().mass_equiv_weight_n, w);
        close(s.wall_weights.iter().map(|w| w.band.design_n).sum(), w);
    }
    assert_eq!(gen.stories.len(), expected.len());
}
#[test]
fn independent_opening_and_multistory_volumes() {
    let mut model = fixture(6000.0);
    bands(&model, &[0.0, 28800.0, 28800.0, 0.0]);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
    model.wall_plates[0].openings = vec![opening(1000.0, 0.0)];
    bands(&model, &[0.0, 19200.0, 28800.0, 0.0]);
    model.wall_plates[0].openings = vec![opening(1000.0, 1000.0)];
    bands(&model, &[0.0, 24000.0, 24000.0, 0.0]);
    model.wall_plates[0].openings = vec![opening(0.0, 2000.0), opening(1000.0, 2000.0)];
    bands(&model, &[0.0, 28800.0, 14400.0, 0.0]);
    close(
        model
            .wall_weight(&model.wall_plates[0])
            .unwrap()
            .totals
            .design_n,
        43200.0,
    );
    bands(&fixture(9000.0), &[0.0, 28800.0, 57600.0, 28800.0]);
}
#[test]
fn dl_selection_and_double_sync_leave_geometric_story_weight_unchanged() {
    for support in [
        WallDlSupport::LowerBeam,
        WallDlSupport::UpperBeam,
        WallDlSupport::HeightMidpoint,
    ] {
        let mut model = fixture(6000.0);
        model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
        model.wall_plates[0].dl_support = Some(support);
        bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
        let ratio = sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap();
        close(ratio.iter().sum(), 1.0);
        for _ in 0..2 {
            let auto = compute_gravity_auto_load_cases(&model).unwrap();
            apply_auto_load_cases(&mut model, &auto.cases);
            let dl = model
                .load_cases
                .iter()
                .find(|c| c.kind == LoadCaseKind::Dead)
                .unwrap()
                .id;
            for method in [MassMethod::LumpedOnly, MassMethod::CorrectedLumped] {
                let g = generate_stories_with_synced_self_weight(&model, &[dl], method).unwrap();
                close(g.stories[1].seismic_weight.unwrap(), 28800.0);
                close(g.stories[2].seismic_weight.unwrap(), 19200.0);
                close(
                    g.stories
                        .iter()
                        .map(|s| s.dynamic_mass.as_ref().unwrap().mass_equiv_weight_n)
                        .sum(),
                    48000.0,
                );
            }
        }
        let dl = model
            .load_cases
            .iter()
            .find(|c| c.kind == LoadCaseKind::Dead)
            .unwrap();
        let gravity =
            generate_stories_with_opts(&model, &[dl.id], false, MassMethod::LumpedOnly).unwrap();
        close(
            gravity
                .stories
                .iter()
                .map(|s| s.seismic_weight.unwrap())
                .sum(),
            48000.0,
        );
        let generated = model.clone();
        let auto = compute_gravity_auto_load_cases(&model).unwrap();
        apply_auto_load_cases(&mut model, &auto.cases);
        assert_eq!(model.load_cases, generated.load_cases);
    }
}
#[test]
fn complete_three_side_slit_overrides_dl_but_never_story_bands() {
    let mut model = fixture(6000.0);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    model.wall_plates[0].slit.column_face = [true, true];
    model.wall_plates[0].slit.beam_face = [true, false];
    model.wall_plates[0].dl_support = Some(WallDlSupport::LowerBeam);
    bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
    let r = sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap();
    close(r[0], 0.0);
    close(r[2], 1.0);
    let auto = compute_gravity_auto_load_cases(&model).unwrap();
    let dl = auto
        .cases
        .iter()
        .find(|c| c.kind == LoadCaseKind::Dead)
        .unwrap();
    assert!(dl.member.iter().all(|m| m.elem == ElemId(2)));
    close(
        dl.member
            .iter()
            .map(|m| match m.kind {
                MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.0,
                MemberLoadKind::Point { p, .. } => p,
            })
            .sum(),
        48000.0,
    );
}
#[test]
fn unknown_positions_keep_totals_and_block_required_partition() {
    for kind in 0..3 {
        let mut model = fixture(6000.0);
        model.wall_plates[0].dl_support = Some(WallDlSupport::LowerBeam);
        match kind {
            0 => model.wall_plates[0].opening_area = 2e6,
            1 => {
                model.wall_plates[0].openings = vec![WallOpening {
                    width: 2000.0,
                    height: 1000.0,
                    offset: None,
                }]
            }
            _ => model.wall_plates[0].opening_weight = 5000.0,
        }
        let weight = model.wall_weight(&model.wall_plates[0]).unwrap();
        close(
            weight.totals.design_n,
            if kind < 2 { 48000.0 } else { 62600.0 },
        );
        assert!(weight.partition_issue.is_some());
        let e = generate_stories(&model, None).unwrap_err();
        assert!(e.contains("壁版 0") && e.contains("未算定"), "{e}");
        let auto = compute_gravity_auto_load_cases(&model).unwrap();
        apply_auto_load_cases(&mut model, &auto.cases);
        let dl = model.load_cases[0].id;
        assert!(
            generate_stories_with_synced_self_weight(&model, &[dl], MassMethod::LumpedOnly)
                .is_err()
        );
        assert!(generate_stories_with_opts(&model, &[dl], false, MassMethod::LumpedOnly).is_ok());
    }
}
#[test]
fn invalid_geometry_and_nonfinite_input_are_diagnostics() {
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut model = fixture(6000.0);
        model.wall_plates[0].openings = vec![WallOpening {
            width: value,
            height: 1000.0,
            offset: Some([0.0, 0.0]),
        }];
        assert!(model
            .wall_weight(&model.wall_plates[0])
            .unwrap_err()
            .contains("壁版 0"));
        let mut model = fixture(6000.0);
        model.sections[0].thickness = Some(value);
        assert!(model.wall_weight(&model.wall_plates[0]).is_err());
    }
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        let mut model = fixture(6000.0);
        model.wall_plates[0].loads = vec![AreaLoad {
            kind: "不正仕上げ".into(),
            value,
        }];
        assert!(model.wall_weight(&model.wall_plates[0]).is_err());
    }
    let mut model = fixture(6000.0);
    model.sections[0].material = None;
    assert!(model
        .wall_weight(&model.wall_plates[0])
        .unwrap_err()
        .contains("主材料"));
    let mut model = fixture(6000.0);
    model.wall_plates[0].openings = vec![opening(3000.0, 2000.0)];
    assert!(model
        .wall_weight(&model.wall_plates[0])
        .unwrap_err()
        .contains("外側"));
    let mut model = fixture(6000.0);
    let a = model.nodes[1].coord;
    model.nodes[1].coord = model.nodes[2].coord;
    model.nodes[2].coord = a;
    assert!(model.wall_weight(&model.wall_plates[0]).is_err());
    let mut model = fixture(6000.0);
    model.wall_assignment_regions.regions.clear();
    assert!(generate_stories(&model, None).is_err());
}

#[test]
fn real_assembly_preserves_physical_wall_mass_in_both_routes_and_methods() {
    for slit in [false, true] {
        for generated in [true, false] {
            for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
                let mut model = fixture(6000.0);
                model.mass_method = method;
                if slit {
                    model.wall_plates[0].slit.column_face = [true, true];
                    model.wall_plates[0].slit.beam_face = [true, false];
                }
                model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
                model.wall_plates[0].loads.push(AreaLoad {
                    kind: "仕上げ".into(),
                    value: 0.001,
                });
                if !generated {
                    model.wall_regions.clear();
                }
                let gen = generate_stories_with_opts(&model, &[], true, method).unwrap();
                for (n, story) in model.nodes.iter_mut().zip(&gen.node_story) {
                    n.story = *story;
                }
                model.nodes.extend(gen.rep_nodes);
                model.wall_weight_generation = Some(gen.wall_weight_generation);
                model.stories = gen.stories;
                model.constraints = gen.constraints;
                model.generated_masters = gen.generated_masters;
                model.damper_mass_generation = Some(gen.damper_mass_generation);
                let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
                assert_eq!(
                    expanded
                        .elements
                        .iter()
                        .filter(|e| index.plate_of(e.id).is_some())
                        .count(),
                    usize::from(generated)
                );
                let map = sepika_core::dof::DofMap::build(&expanded);
                let matrix = sepika_solver::common::assemble::assemble_global_m(
                    &expanded,
                    &map,
                    sepika_element::behavior::MassOption::Lumped,
                )
                .unwrap()
                .to_dense();
                let ux: Vec<_> = (0..expanded.nodes.len())
                    .filter_map(|i| map.active(i * 6).map(|v| v as usize))
                    .collect();
                let total: f64 = ux
                    .iter()
                    .flat_map(|&i| ux.iter().map(move |&j| (i, j)))
                    .map(|(i, j)| matrix[(i, j)])
                    .sum();
                assert!(
                    (total * GRAVITY_MM_S2 - 58000.0).abs() < 1e-5,
                    "generated={generated} method={method:?} actual={} wall={:?}",
                    total * GRAVITY_MM_S2,
                    expanded.wall_weight(&expanded.wall_plates[0])
                );
            }
        }
    }
}

#[test]
fn asymmetric_real_faces_separate_wall_midpoint_from_floor_midpoint() {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
    let mut model = fixture(6000.0);
    for (id, depth) in [(2, 400.0), (3, 800.0)] {
        let mut section = SectionShape::RcColumnRect {
            b: 400.0,
            d: depth,
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
        .to_section(SectionId(id), "支持矩形".into());
        section.material = Some(MaterialId(0));
        model.sections.push(section);
    }
    for e in &mut model.elements {
        e.section = Some(SectionId(if e.id == ElemId(2) { 3 } else { 2 }));
    }
    // Concrete区分の内法はFcの有無で切り替えない。
    let mut support_material = model.materials[0].clone();
    support_material.id = MaterialId(1);
    support_material.fc = None;
    model.materials.push(support_material);
    for section in model.sections.iter_mut().skip(2) {
        section.material = Some(MaterialId(1));
    }
    let w = model.wall_weight(&model.wall_plates[0]).unwrap();
    close(w.totals.design_n, 41472.0);
    close(w.band(3000.0, 4500.0).unwrap().design_n, 22464.0);
    close(w.band(4500.0, 6000.0).unwrap().design_n, 19008.0);
    let ratios = sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap();
    close(ratios[0] * w.totals.design_n, 20736.0);
    close(ratios[2] * w.totals.design_n, 20736.0);
    for section in model.sections.iter_mut().skip(2) {
        if let Some(SectionShape::RcColumnRect { b, d, rebar }) = section.shape.clone() {
            section.shape = Some(SectionShape::SrcColumnRect {
                b,
                d,
                rebar,
                steel_height: 200.0,
                steel_width: 150.0,
                steel_web_thick: 8.0,
                steel_flange_thick: 10.0,
            });
        }
    }
    close(
        model
            .wall_weight(&model.wall_plates[0])
            .unwrap()
            .totals
            .design_n,
        41472.0,
    );
    model.materials[1].category = MaterialCategory::Steel;
    close(
        model
            .wall_weight(&model.wall_plates[0])
            .unwrap()
            .net_area_mm2,
        12000000.0,
    );
}
#[test]
fn rotated_trapezoid_and_arbitrary_edges_keep_independent_geometry() {
    let mut model = fixture(6000.0);
    model.nodes[2].coord[0] = 3000.0;
    model.nodes[3].coord[0] = 1000.0;
    let angle = 0.7_f64;
    for n in &mut model.nodes {
        let x = n.coord[0];
        n.coord[0] = x * angle.cos();
        n.coord[1] = x * angle.sin();
    }
    let w = model.wall_weight(&model.wall_plates[0]).unwrap();
    close(w.totals.design_n, 43200.0);
    close(w.band(3000.0, 4500.0).unwrap().design_n, 25200.0);
    close(w.band(4500.0, 6000.0).unwrap().design_n, 18000.0);
    model.wall_plates[0].dl_support = None;
    model.wall_plates[0].self_weight_shares = vec![0.75, 0.0, 0.25, 0.0];
    assert_eq!(
        sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap(),
        vec![0.75, 0.0, 0.25, 0.0]
    );
    close(
        model
            .wall_weight(&model.wall_plates[0])
            .unwrap()
            .totals
            .design_n,
        43200.0,
    );
    model.wall_plates[0].dl_support = Some(WallDlSupport::LowerBeam);
    assert!(
        sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0])
            .unwrap_err()
            .contains("同時")
    );
}

#[test]
fn rc_standard_physical_density_is_not_replaced_by_saved_density() {
    let mut model = fixture(6000.0);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    model.materials[0].density = 1.0e-9;
    let w = model.wall_weight(&model.wall_plates[0]).unwrap();
    close(
        w.totals.design_n,
        200.0 * 10000000.0 * 1.0e-9 * GRAVITY_MM_S2,
    );
    close(w.totals.physical_n, 48000.0);
    close(w.totals.matrix_n, 48000.0);
    let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
    let elem = expanded
        .elements
        .iter()
        .find(|e| index.plate_of(e.id).is_some())
        .unwrap();
    let behavior = sepika_element::factory::build_behavior(elem, &expanded);
    let matrix = behavior.mass_matrix(sepika_element::behavior::MassOption::Lumped);
    let total: f64 = (0..4).map(|i| matrix.get(i * 6, i * 6)).sum();
    close(total * GRAVITY_MM_S2, 48000.0);
}

#[test]
fn equal_dl_total_but_changed_opening_position_requires_fresh_story_bands() {
    let mut model = fixture(6000.0);
    model.wall_plates[0].dl_support = Some(WallDlSupport::LowerBeam);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    assert!(model
        .validate_wall_weight_generation()
        .unwrap_err()
        .contains("未設定"));
    let gen = generate_stories(&model, None).unwrap();
    model.wall_weight_generation = Some(gen.wall_weight_generation);
    model.stories = gen.stories;
    model.validate_wall_weight_generation().unwrap();
    let before = compute_gravity_auto_load_cases(&model).unwrap().cases;
    model.wall_plates[0].openings[0].offset = Some([1000.0, 0.0]);
    let after = compute_gravity_auto_load_cases(&model).unwrap().cases;
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(a.nodal, b.nodal);
        assert_eq!(a.member, b.member);
    }
    assert!(model
        .validate_wall_weight_generation()
        .unwrap_err()
        .contains("壁版 0"));
    let auto =
        sepika_job::auto_loads::compute_seismic_auto_load_cases(&model, &Default::default(), None);
    assert!(auto.cases.is_empty());
    assert!(auto.notices.iter().any(|m| m.contains("壁版 0")));
    let gen = generate_stories(&model, None).unwrap();
    model.stories = gen.stories;
    model.validate_wall_weight_generation().unwrap();
    model.wall_plates[0].opening_weight = 1000.0;
    model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
    assert!(model
        .validate_wall_weight_generation()
        .unwrap_err()
        .contains("混在"));
    let gen = generate_stories_with_opts(&model, &[], false, model.mass_method).unwrap();
    model.stories = gen.stories;
    model.validate_wall_weight_generation().unwrap();
}

#[test]
fn enclosed_secondary_beam_receives_all_dl_before_reaction_cascade() {
    for mode in [
        WallDlSupport::LowerBeam,
        WallDlSupport::UpperBeam,
        WallDlSupport::HeightMidpoint,
    ] {
        let mut model = fixture(6000.0);
        model.wall_plates[0].dl_support = Some(mode);
        model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
        model.nodes.push(Node {
            id: NodeId(7),
            coord: [4000.0, 0.0, 0.0],
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
        model.elements[0].nodes = vec![NodeId(4), NodeId(7)].into();
        let key = SecondaryMemberId(0);
        model.unassigned_beams.push(SecondaryMember {
            id: key,
            kind: SecondaryMemberKind::Beam,
            ends: SecondaryMemberEnds::Supported([
                SecondaryMemberAnchor {
                    support: SupportMemberId::Primary(ElemId(3)),
                    position: 1.0,
                },
                SecondaryMemberAnchor {
                    support: SupportMemberId::Primary(ElemId(1)),
                    position: 0.0,
                },
            ]),
            section: Some(SectionId(1)),
            name: "支持小梁".into(),
            gravity_end_shares: None,
        });
        model.wall_assignment_regions.regions[0].boundary[0].support =
            SupportMemberId::Secondary(key);
        let ratios =
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap();
        assert_eq!(ratios, vec![1.0, 0.0, 0.0, 0.0]);
        let loads = sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model).unwrap();
        let MemberLoadKind::Distributed { a, b, w1, w2 } = &loads.posts[&key].member_loads[0]
        else {
            panic!("小梁区間荷重")
        };
        close((b - a) * (w1 + w2) / 2.0, 48000.0);
        let cascade = sepika_load::cascade::solve_with_basis(
            &model,
            |_| 0.0,
            true,
            sepika_load::cascade::SelfWeightBasis::Design,
        )
        .unwrap();
        assert!(cascade.unresolved.is_empty() && cascade.cyclic.is_empty());
        let (nodes, members) = cascade.primary_loads(&model);
        close(
            nodes.iter().map(|(_, w)| w).sum::<f64>()
                + members.iter().map(|p| p.cmq.q_i + p.cmq.q_j).sum::<f64>(),
            48000.0,
        );
        let w = model.wall_weight(&model.wall_plates[0]).unwrap();
        close(w.band(3000.0, 4500.0).unwrap().design_n, 28800.0);
        close(w.band(4500.0, 6000.0).unwrap().design_n, 19200.0);
        model.unassigned_beams[0].ends = SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Secondary(key),
                position: 0.0,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Secondary(key),
                position: 1.0,
            },
        ]);
        assert!(compute_gravity_auto_load_cases(&model).is_err());
    }
}
#[test]
fn attached_waist_hanging_parapet_and_partial_trapezoid_use_geometry_bands() {
    for (nodes, extent, span, expected) in [
        (
            [NodeId(0), NodeId(1)],
            [1000.0, 1000.0],
            [0.25, 0.75],
            9600.0,
        ),
        (
            [NodeId(0), NodeId(1)],
            [-1000.0, -1000.0],
            [0.25, 0.75],
            9600.0,
        ),
        (
            [NodeId(3), NodeId(2)],
            [1000.0, 1000.0],
            [0.0, 1.0],
            19200.0,
        ),
        (
            [NodeId(0), NodeId(1)],
            [1000.0, 2000.0],
            [0.25, 0.75],
            14400.0,
        ),
    ] {
        let mut model = fixture(6000.0);
        model.wall_plates.clear();
        model.wall_regions.clear();
        model.wall_assignment_regions.regions.clear();
        model.wall_plates.push(WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes,
                    span,
                    transfer: LoadTransfer::Anchor,
                },
                extent: Some(extent),
            },
            section: Some(SectionId(0)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
            dl_support: None,
            self_weight_shares: vec![],
        });
        let w = model.wall_weight(&model.wall_plates[0]).unwrap();
        close(w.totals.design_n, expected);
        close(w.totals.physical_n, expected);
        close(w.totals.matrix_n, 0.0);
        let parts = w.projected_design_line_loads().unwrap();
        close(
            parts
                .iter()
                .map(|p| (p[1] - p[0]) * (p[2] + p[3]) / 2.0)
                .sum(),
            expected,
        );
        let gen = generate_stories_with_opts(&model, &[], true, MassMethod::LumpedOnly).unwrap();
        close(
            gen.stories
                .iter()
                .map(|s| s.wall_weights.iter().map(|w| w.band.design_n).sum::<f64>())
                .sum(),
            expected,
        );
        close(
            gen.rep_nodes
                .iter()
                .filter_map(|n| n.mass)
                .map(|m| m[0] * GRAVITY_MM_S2)
                .sum(),
            expected,
        );
        if extent == [1000.0, 2000.0] {
            close(w.band(3000.0, 4500.0).unwrap().design_n, 13200.0);
            close(w.band(4500.0, 6000.0).unwrap().design_n, 1200.0);
        }
    }
}

#[test]
fn generated_wall_origin_preserves_plate_identity_and_deduplicates_assignments() {
    let mut model = fixture(6000.0);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    model.wall_regions[0].wall_plate_ids.push(WallPlateId(0));
    let duplicate = model.wall_regions[0].clone();
    model.wall_regions.push(duplicate);
    let (expanded, index, report) = sepika_load::wall_expand::expand_wall_elements(&model);
    assert_eq!(report.generated, 1);
    assert_eq!(index.len(), 1);
    let id = index.generated_elem_ids().next().unwrap();
    assert_eq!(
        expanded.generated_wall_origins.get(&id),
        Some(&WallPlateId(0))
    );
    let mut second = model.wall_plates[0].clone();
    second.id = WallPlateId(1);
    let mut section = model.sections[0].clone();
    section.id = SectionId(2);
    section.thickness = Some(100.0);
    section.shape = Some(SectionShape::RcWall {
        thickness: 100.0,
        pwh_ratio: None,
        ps: 0.0025,
    });
    model.sections.push(section);
    second.section = Some(SectionId(2));
    model.wall_plates.push(second);
    let mut assignment = model.wall_assignment_regions.regions[0].clone();
    assignment.id = WallPlateAssignmentRegionId(1);
    assignment.assignment = PlateAssignment::Plate(WallPlateId(1));
    model.wall_assignment_regions.regions.push(assignment);
    model.wall_regions[0].wall_plate_ids.push(WallPlateId(1));
    let (expanded, index, report) = sepika_load::wall_expand::expand_wall_elements(&model);
    assert_eq!(report.generated, 2);
    for id in index.generated_elem_ids() {
        let plate = index.plate_of(id).unwrap();
        let elem = expanded.element(id).unwrap();
        let behavior = sepika_element::factory::build_behavior(elem, &expanded);
        let matrix = behavior.mass_matrix(sepika_element::behavior::MassOption::Lumped);
        let total: f64 = (0..4).map(|i| matrix.get(i * 6, i * 6)).sum();
        close(
            total * GRAVITY_MM_S2,
            if plate == WallPlateId(0) {
                48000.0
            } else {
                24000.0
            },
        );
        let mut native = expanded.clone();
        native.generated_wall_origins.clear();
        native.wall_plates[0].opening_area = 1.0;
        native.wall_plates[0].openings.clear();
        let native_behavior = sepika_element::factory::build_behavior(elem, &native);
        let native_matrix =
            native_behavior.mass_matrix(sepika_element::behavior::MassOption::Lumped);
        let native_total: f64 = (0..4).map(|i| native_matrix.get(i * 6, i * 6)).sum();
        close(native_total, total);
    }
}

#[test]
fn native_wall_without_physical_plate_is_explicitly_unavailable_for_weight() {
    let model = fixture(6000.0);
    let (mut native, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
    let wall_id = index.generated_elem_ids().next().unwrap();
    native.generated_wall_origins.clear();
    native.wall_plates.clear();
    native.wall_regions.clear();
    let error = generate_stories(&native, None).unwrap_err();
    assert!(error.contains(&format!("解析壁要素 {}", wall_id.0)) && error.contains("変換"));
    assert!(compute_gravity_auto_load_cases(&native).is_err());
    let map = sepika_core::dof::DofMap::build(&native);
    assert!(sepika_solver::common::assemble::assemble_global_m(
        &native,
        &map,
        sepika_element::behavior::MassOption::Lumped
    )
    .is_err());
}

#[test]
fn enclosed_partial_beam_spans_and_midspan_posts_support_all_dl_modes() {
    use sepika_load::floor::{LoadShape, LoadTarget};
    for (mode, lower, upper) in [
        (WallDlSupport::LowerBeam, 48000.0, 0.0),
        (WallDlSupport::UpperBeam, 0.0, 48000.0),
        (WallDlSupport::HeightMidpoint, 28800.0, 19200.0),
    ] {
        let mut model = fixture(6000.0);
        // 6m梁の両端に節点、内側の4m壁の左右は梁中間に取付く間柱。
        // 壁の下端3000/上端6000。四隅にはモデル節点を作らない。
        for (id, x) in [(0, -1000.0), (1, 5000.0), (2, 5000.0), (3, -1000.0)] {
            model.nodes[id].coord[0] = x;
        }
        model.nodes[6].coord[0] = -1000.0;
        model.wall_regions[0].wall_plate_ids.clear();
        for (id, lower_t, upper_t) in [(0, 1.0 / 6.0, 5.0 / 6.0), (1, 5.0 / 6.0, 1.0 / 6.0)] {
            model.unassigned_posts.push(SecondaryMember {
                id: SecondaryMemberId(id),
                kind: SecondaryMemberKind::Post,
                ends: SecondaryMemberEnds::Supported([
                    SecondaryMemberAnchor {
                        support: SupportMemberId::Primary(ElemId(0)),
                        position: lower_t,
                    },
                    SecondaryMemberAnchor {
                        support: SupportMemberId::Primary(ElemId(2)),
                        position: upper_t,
                    },
                ]),
                section: Some(SectionId(1)),
                name: "梁中間の間柱".into(),
                gravity_end_shares: Some([1.0, 0.0]),
            });
        }
        let boundary = &mut model.wall_assignment_regions.regions[0].boundary;
        boundary[0].span = [1.0 / 6.0, 5.0 / 6.0];
        boundary[1].support = SupportMemberId::Secondary(SecondaryMemberId(1));
        boundary[2].span = [1.0 / 6.0, 5.0 / 6.0];
        boundary[3].support = SupportMemberId::Secondary(SecondaryMemberId(0));
        model.wall_plates[0].dl_support = Some(mode);
        model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
        let p = &model.wall_plates[0];
        assert!(p.boundary_nodes(&model).is_none());
        let w = model.wall_weight(p).unwrap();
        close(w.totals.design_n, 48000.0);
        close(w.totals.physical_n, 48000.0);
        close(w.totals.matrix_n, 0.0);
        bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
        let dl = sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model).unwrap();
        let mut actual = [0.0, 0.0];
        for load in &dl.primary {
            let LoadTarget::Span { t, .. } = load.target else {
                panic!("支持梁の部分区間")
            };
            close(t[0], 1.0 / 6.0);
            close(t[1], 5.0 / 6.0);
            let LoadShape::Uniform { w } = load.shape else {
                panic!("梁区間DL")
            };
            actual[if load.elem == ElemId(0) {
                0
            } else {
                assert_eq!(load.elem, ElemId(2));
                1
            }] += w * 4000.0;
        }
        close(actual[0], lower);
        close(actual[1], upper);
        assert!(dl.posts.is_empty());
        let auto = compute_gravity_auto_load_cases(&model).unwrap();
        apply_auto_load_cases(&mut model, &auto.cases);
        let dl_id = model
            .load_cases
            .iter()
            .find(|c| c.kind == LoadCaseKind::Dead)
            .unwrap()
            .id;
        let synced =
            generate_stories_with_synced_self_weight(&model, &[dl_id], MassMethod::LumpedOnly)
                .unwrap();
        close(synced.stories[1].seismic_weight.unwrap(), 28800.0);
        close(synced.stories[2].seismic_weight.unwrap(), 19200.0);
        // 任意辺率でも同じ実支持・スリット契約を用いる。
        model.wall_plates[0].dl_support = None;
        model.wall_plates[0].self_weight_shares = vec![0.0, 0.0, 0.0, 1.0];
        assert_eq!(
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap(),
            vec![0.0, 0.0, 0.0, 1.0]
        );
        let dl = sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model).unwrap();
        let total: f64 = dl.posts[&SecondaryMemberId(0)]
            .member_loads
            .iter()
            .map(|l| match l {
                MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.0,
                MemberLoadKind::Point { p, .. } => *p,
            })
            .sum();
        close(total, 48000.0);
        assert!(dl.primary.is_empty());
        assert!(compute_gravity_auto_load_cases(&model).is_ok());
        bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
        // 指定スリットが境界へ解決できない場合は切れていないと推定しない。
        model.wall_plates[0].slit.column_face[0] = true;
        assert!(
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0])
                .unwrap_err()
                .contains("スリット対応")
        );
        let error = match compute_gravity_auto_load_cases(&model) {
            Err(error) => error,
            Ok(_) => panic!("対応が未解決のスリットを拒否する"),
        };
        assert!(error.to_string().contains("壁版 0"));
        model.wall_plates[0].dl_support = Some(WallDlSupport::HeightMidpoint);
        model.wall_plates[0].self_weight_shares.clear();
        model.wall_plates[0].section = None;
        assert!(
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0])
                .unwrap_err()
                .contains("スリット対応")
        );
        assert!(compute_gravity_auto_load_cases(&model).is_err());
        model.wall_plates[0].slit = Default::default();
        assert_eq!(
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap(),
            vec![0.0; 4]
        );
    }
}

#[test]
fn attached_sign_reversal_openings_use_all_components_and_union() {
    for (openings, total, below, above) in [
        (
            vec![WallOpening {
                width: 500.0,
                height: 200.0,
                offset: Some([3000.0, 1250.0]),
            }],
            9120.0,
            4800.0,
            4320.0,
        ),
        (
            vec![WallOpening {
                width: 500.0,
                height: 200.0,
                offset: Some([500.0, 550.0]),
            }],
            9120.0,
            4320.0,
            4800.0,
        ),
        (
            vec![
                WallOpening {
                    width: 500.0,
                    height: 200.0,
                    offset: Some([3000.0, 1250.0]),
                },
                WallOpening {
                    width: 500.0,
                    height: 200.0,
                    offset: Some([3250.0, 1250.0]),
                },
            ],
            8880.0,
            4800.0,
            4080.0,
        ),
        (
            vec![
                WallOpening {
                    width: 500.0,
                    height: 200.0,
                    offset: Some([3000.0, 1250.0]),
                },
                WallOpening {
                    width: 500.0,
                    height: 200.0,
                    offset: Some([500.0, 550.0]),
                },
            ],
            8640.0,
            4320.0,
            4320.0,
        ),
    ] {
        let mut model = fixture(6000.0);
        model.wall_regions.clear();
        model.wall_assignment_regions.regions.clear();
        let p = &mut model.wall_plates[0];
        p.shape = WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: Some([-1000.0, 1000.0]),
        };
        p.dl_support = None;
        p.openings = openings;
        let w = model.wall_weight(&model.wall_plates[0]).unwrap();
        // 各三角形1m²、γt=4.8kN/m²。単一開口0.1m²、重複和集合0.15m²。
        close(w.totals.design_n, total);
        close(w.totals.physical_n, total);
        close(w.band(2000.0, 3000.0).unwrap().design_n, below);
        close(w.band(3000.0, 4000.0).unwrap().design_n, above);
        close(
            w.projected_design_line_loads()
                .unwrap()
                .iter()
                .map(|p| (p[1] - p[0]) * (p[2] + p[3]) / 2.0)
                .sum(),
            total,
        );
        bands(&model, &[0.0, total, 0.0, 0.0]);
        let loads = sepika_load::wall_attached::attached_wall_beam_loads(&model).unwrap();
        close(
            loads
                .iter()
                .map(|l| match (l.target, l.shape) {
                    (
                        sepika_load::floor::LoadTarget::Span { t, .. },
                        sepika_load::floor::LoadShape::Linear { w_i, w_j },
                    ) => 4000.0 * (t[1] - t[0]).abs() * (w_i + w_j) / 2.0,
                    (
                        sepika_load::floor::LoadTarget::Span { t, .. },
                        sepika_load::floor::LoadShape::Uniform { w },
                    ) => 4000.0 * (t[1] - t[0]).abs() * w,
                    _ => panic!("線アンカー投影DL"),
                })
                .sum(),
            total,
        );
        for opening in [
            WallOpening {
                width: 1000.0,
                height: 100.0,
                offset: Some([1500.0, 950.0]),
            },
            WallOpening {
                width: 500.0,
                height: 200.0,
                offset: Some([3000.0, 1750.0]),
            },
        ] {
            model.wall_plates[0].openings = vec![opening];
            assert!(model
                .wall_weight(&model.wall_plates[0])
                .unwrap_err()
                .contains("外側"));
        }
    }
}

#[test]
fn resolved_arbitrary_dl_rates_obey_slit_edges_and_mode_exclusion() {
    let mut model = fixture(6000.0);
    model.wall_plates[0].openings = vec![opening(1000.0, 2000.0)];
    model.wall_plates[0].dl_support = None;
    model.wall_plates[0].slit.beam_face = [false, true];
    model.wall_plates[0].self_weight_shares = vec![1.0, 0.0, 0.0, 0.0];
    assert_eq!(
        sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap(),
        vec![1.0, 0.0, 0.0, 0.0]
    );
    assert!(compute_gravity_auto_load_cases(&model).is_ok());
    bands(&model, &[0.0, 28800.0, 19200.0, 0.0]);
    model.wall_plates[0].self_weight_shares = vec![0.0, 0.0, 1.0, 0.0];
    assert!(sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).is_err());
    assert!(compute_gravity_auto_load_cases(&model).is_err());
    for mode in [
        WallDlSupport::LowerBeam,
        WallDlSupport::UpperBeam,
        WallDlSupport::HeightMidpoint,
    ] {
        model.wall_plates[0].dl_support = Some(mode);
        assert!(
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0])
                .unwrap_err()
                .contains("同時指定")
        );
    }
}

#[test]
fn nonrectangular_slits_require_every_specified_role_but_preserve_resolved_beams() {
    let mut model = fixture(6000.0);
    model.nodes[2].coord[0] = 3500.0;
    model.nodes[3].coord[0] = 500.0;
    model.wall_plates[0].dl_support = None;
    model.wall_plates[0].self_weight_shares = vec![0.0, 1.0, 0.0, 0.0];
    let weight = model.wall_weight(&model.wall_plates[0]).unwrap();
    assert!((weight.totals.design_n - 50400.0).abs() < 1e-6);
    bands(&model, &[0.0, 27000.0, 23400.0, 0.0]);
    assert!(compute_gravity_auto_load_cases(&model).is_ok());
    let loads = sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model).unwrap();
    assert!(
        (loads
            .primary
            .iter()
            .map(|l| l.cmq.q_i + l.cmq.q_j)
            .sum::<f64>()
            - 50400.0)
            .abs()
            < 1e-6
    );
    for mode in [
        WallDlSupport::LowerBeam,
        WallDlSupport::UpperBeam,
        WallDlSupport::HeightMidpoint,
    ] {
        let mut selected = model.clone();
        selected.wall_plates[0].self_weight_shares.clear();
        selected.wall_plates[0].dl_support = Some(mode);
        assert!(compute_gravity_auto_load_cases(&selected).is_ok());
        bands(&selected, &[0.0, 27000.0, 23400.0, 0.0]);
    }
    let mut one_vertical = model.clone();
    one_vertical.nodes[3].coord[0] = 0.0;
    one_vertical.wall_plates[0].slit.column_face = [true, false];
    assert!(
        sepika_load::wall_plate_load::slit_specification_is_reflected(
            &one_vertical,
            &one_vertical.wall_plates[0]
        )
    );
    assert!(compute_gravity_auto_load_cases(&one_vertical).is_ok());
    one_vertical.wall_plates[0].slit.column_face = [true, true];
    assert!(
        !sepika_load::wall_plate_load::slit_specification_is_reflected(
            &one_vertical,
            &one_vertical.wall_plates[0]
        )
    );
    assert!(compute_gravity_auto_load_cases(&one_vertical).is_err());
    let mut rectangle = fixture(6000.0);
    rectangle.wall_plates[0].slit.column_face = [true, true];
    rectangle.wall_plates[0].slit.beam_face = [true, true];
    assert!(
        sepika_load::wall_plate_load::slit_specification_is_reflected(
            &rectangle,
            &rectangle.wall_plates[0]
        )
    );
    assert!(compute_gravity_auto_load_cases(&rectangle).is_err());
    model.wall_plates[0].slit.beam_face = [false, true];
    assert!(
        sepika_load::wall_plate_load::slit_specification_is_reflected(
            &model,
            &model.wall_plates[0]
        )
    );
    assert!(compute_gravity_auto_load_cases(&model).is_ok());
    for columns in [[true, false], [false, true], [true, true]] {
        model.wall_plates[0].slit.column_face = columns;
        assert!(
            !sepika_load::wall_plate_load::slit_specification_is_reflected(
                &model,
                &model.wall_plates[0]
            )
        );
        let error =
            sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap_err();
        assert!(
            error.contains("壁版") && error.contains("スリット対応"),
            "{error}"
        );
        assert!(compute_gravity_auto_load_cases(&model).is_err());
    }
    model.wall_plates[0].self_weight_shares.clear();
    for mode in [
        WallDlSupport::LowerBeam,
        WallDlSupport::UpperBeam,
        WallDlSupport::HeightMidpoint,
    ] {
        model.wall_plates[0].dl_support = Some(mode);
        assert!(sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).is_err());
        assert!(compute_gravity_auto_load_cases(&model).is_err());
    }
    model.wall_plates[0].section = None;
    assert!(sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).is_err());
    model.wall_plates[0].slit = Default::default();
    assert_eq!(
        sepika_load::wall_plate_load::dl_ratios(&model, &model.wall_plates[0]).unwrap(),
        vec![0.0; 4]
    );
}

#[test]
fn wall_generation_outputs_are_excluded_from_input_key_and_tampering_is_refreshed() {
    use sepika_job::weight_preparation::{
        apply_generated_weights, weight_input_key, weights_are_current,
    };
    let mut model = fixture(6000.0);
    let method = MassMethod::CorrectedLumped;
    let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
    apply_generated_weights(&mut model, generated, method);
    close(
        model
            .stories
            .iter()
            .flat_map(|s| &s.wall_weights)
            .map(|w| w.band.design_n)
            .sum(),
        57600.0,
    );
    let input = weight_input_key(&model, method);
    assert!(weights_are_current(&model, method));
    model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
    for story in &mut model.stories {
        story.wall_weights.clear();
    }
    assert_eq!(weight_input_key(&model, method), input);
    assert!(!weights_are_current(&model, method));
    let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
    apply_generated_weights(&mut model, generated, method);
    assert_eq!(weight_input_key(&model, method), input);
    assert!(weights_are_current(&model, method));
    model.validate_wall_weight_generation().unwrap();
    let record = model.seismic_weight_generation.clone();
    let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
    apply_generated_weights(&mut model, generated, method);
    assert_eq!(model.seismic_weight_generation, record);
}

#[test]
fn high_density_steel_wall_rejects_all_design_routes_but_preserves_physical_weight() {
    for attached in [false, true] {
        let mut model = fixture(6000.0);
        model.sections[0].shape = None;
        model.materials[0].category = MaterialCategory::Steel;
        model.materials[0].fc = None;
        model.materials[0].density = 85e-6 / 9806.65;
        // 支持梁の無重量材料と壁鋼材を共有させない。
        let mut support_material = model.materials[0].clone();
        support_material.id = MaterialId(1);
        support_material.density = 0.0;
        model.materials.push(support_material);
        model.sections[1].material = Some(MaterialId(1));
        if attached {
            model.wall_plates[0].shape = WallPlateShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::Anchor,
                },
                extent: Some([3000.0, 3000.0]),
            };
            model.wall_plates[0].dl_support = None;
        }
        let weight = model.wall_weight(&model.wall_plates[0]).unwrap();
        close(weight.totals.physical_n, 204000.0);
        let error = compute_gravity_auto_load_cases(&model)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("材料 0") && error.contains("壁版 0") && error.contains("過小評価"),
            "{error}"
        );
        assert!(generate_stories(&model, None)
            .unwrap_err()
            .contains("壁版 0"));
        assert!(
            sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model)
                .unwrap_err()
                .contains("壁版 0")
        );
        if attached {
            assert!(sepika_load::wall_attached::attached_wall_beam_loads(&model)
                .unwrap_err()
                .contains("壁版 0"));
            let mut nodes = vec![0.0; model.nodes.len()];
            assert!(
                sepika_load::wall_attached::accumulate_attached_wall_dl_weight(&model, &mut nodes)
                    .is_err()
            );
            sepika_load::wall_attached::accumulate_attached_wall_dl_mass_equiv(&model, &mut nodes);
            close(nodes.iter().sum(), 204000.0);
        } else {
            assert!(
                sepika_load::wall_plate_load::distribute_enclosed_wall_plates_with_basis(
                    &model,
                    sepika_load::cascade::SelfWeightBasis::MassEquiv
                )
                .is_ok()
            );
        }
    }
}
