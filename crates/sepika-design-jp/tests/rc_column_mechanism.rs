#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;
#[path = "fixtures/rc_column_mechanism_model.rs"]
mod fixture;
#[path = "fixtures/rc_column_mechanism_reference.rs"]
mod reference;

use fixture::{COLUMN, GIRDER_SECTION, REBAR};
use reference::{CONTINUOUS_SHEAR_RATIO, TERMINAL_SHEAR_RATIO};
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    Constraint, ElementKind, EndCondition, FrameSectionUse, GirderTorsionMode, Model, NodalLoad,
    Story,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::rc::{
    compute_column_mechanism_sum_my, time_history_column_mechanism_sum_my, ColumnMechanismTopology,
};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::factory::build_behavior;
use sepika_element::frame::beam::{apply_auto_rigid_zones, MemberForces, RigidZoneRule};
use sepika_solver::statics::linear::{linear_static_once, superpose_static};

fn check(
    model: &Model,
    method: QdMethod,
    gravity_n: Option<f64>,
    forces: [f64; 6],
) -> Vec<CheckOutcome> {
    check_stations(model, method, gravity_n, forces, &[0.5])
}

fn check_stations(
    model: &Model,
    method: QdMethod,
    gravity_n: Option<f64>,
    forces: [f64; 6],
    stations: &[f64],
) -> Vec<CheckOutcome> {
    let dist = |force| {
        [(
            COLUMN,
            MemberForces {
                at: stations.iter().map(|&pos| (pos, force)).collect(),
            },
        )]
    };
    let gravity = gravity_n.map(|n| dist([n, 0.0, 0.0, 0.0, 0.0, 0.0]));
    run_member_design_checks(
        model,
        &dist(forces),
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: method,
            long_member_forces: gravity.as_ref().map(|forces| &forces[..]),
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .filter_map(|(elem, pos, row)| (elem == COLUMN && pos == 0.5).then_some(row))
    .collect()
}

fn yielding_head_model() -> Model {
    fixture::column_with_yielding_head_girders(column::column_model(false, 0.0))
}

fn set_head_story_name(model: &mut Model, name: &str) {
    model.stories[1].name = name.into();
    model.stories[1].node_ids = vec![NodeId(1), NodeId(2), NodeId(3)];
    for node in &mut model.nodes[1..=3] {
        node.story = Some(StoryId(1));
    }
}

fn assert_qd1_shear_ratio(model: &Model, expected: f64) {
    model.validate().unwrap();
    let rows = check_stations(
        model,
        QdMethod::Qd1,
        Some(-100_000.0),
        [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0],
        &[0.0, 0.5, 1.0],
    );
    let [CheckOutcome::Checked(result)] = &rows[..] else {
        panic!("{rows:?}");
    };
    let shear = result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap();
    assert!(
        (shear.ratio - expected).abs() < 1e-12,
        "せん断検定比: 結果={}, 期待値={expected}",
        shear.ratio,
    );
}

#[test]
fn qd1_terminal_column_uses_full_head_girder_moment() {
    let mut model = yielding_head_model();
    assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);

    model.elements[COLUMN.index()].nodes.swap(0, 1);
    fixture::add_column_above(&mut model, NodeId(2), ElementKind::Beam);
    set_head_story_name(&mut model, "B1F");
    let mut higher_story = Story::new(StoryId(2), "RF".into(), Some(4500.0), Some(0.0));
    let higher_node = model.nodes.last_mut().unwrap();
    higher_node.story = Some(higher_story.id);
    higher_story.node_ids.push(higher_node.id);
    model.stories.push(higher_story);
    model.constraints.push(Constraint::rigid_diaphragm(
        StoryId(1),
        NodeId(1),
        vec![NodeId(2), NodeId(3)],
    ));
    assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);
}

#[test]
fn qd1_continuous_column_uses_half_head_girder_moment() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = yielding_head_model();
        model.elements[COLUMN.index()].nodes.swap(0, 1);
        fixture::add_column_above(&mut model, NodeId(1), kind);
        set_head_story_name(&mut model, "RF");
        let upper = model.elements.last_mut().unwrap();
        upper.nodes.swap(0, 1);
        upper.end_offsets_mm[1] = [100.0, -70.0, 40.0];
        if kind == ElementKind::Beam {
            let steel = &mut model.materials[fixture::WRONG_REBAR.index()];
            steel.name = "SN400B".into();
            steel.fy = Some(235.0);
            let steel_section = SectionId(2);
            let mut section = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(0.0),
            }
            .to_section(steel_section, "上階の鉄骨柱".into());
            section.frame_use = Some(FrameSectionUse::Column);
            section.material = Some(steel.id);
            model.sections.push(section);
            upper.section = Some(steel_section);
        }
        assert_qd1_shear_ratio(&model, CONTINUOUS_SHEAR_RATIO);
    }
}

#[test]
fn qd1_continuous_column_compares_the_reduced_girder_moments() {
    for (head_bars, foot_bars, expected_n) in [
        (2, 1, reference::REVERSIBLE_ONE_BAR_FOOT_QD1_N),
        (2, 3, reference::TWO_HEAD_GIRDERS_HALF_QD1_N),
        (4, 3, reference::REVERSIBLE_THREE_BAR_FOOT_QD1_N),
    ] {
        let mut model = fixture::column_with_reversible_girders(
            column::column_model(false, 0.0),
            head_bars,
            foot_bars,
        );
        let ratio = expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N;
        assert_qd1_shear_ratio(&model, ratio);
        for elem in &mut model.elements {
            elem.nodes.swap(0, 1);
        }
        assert_qd1_shear_ratio(&model, ratio);
    }
}

#[test]
fn qd1_chooses_the_foot_when_both_unscaled_girder_sums_are_below_column_yield() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
    let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!();
    };
    rebar.x = vec![4];
    rebar.y = vec![4];
    // 柱My=217.288602 kN·m、未係数の梁和は柱頭190.333125 kN·m・柱脚95.166562 kN·m。
    assert_qd1_shear_ratio(
        &model,
        176_581.255_475_634 / reference::COLUMN_ALLOWABLE_SHEAR_N,
    );
}

#[test]
fn qd1_excludes_slab_supported_girder_yield_without_changing_qd2() {
    let forces = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
    for (continuous, height_mm, bare_n, slab_n) in [
        (
            false,
            1700.0,
            156_178.120_903_194,
            reference::TERMINAL_SLAB_HEAD_QD1_N,
        ),
        (
            true,
            1500.0,
            208_303.442_920_260,
            reference::CONTINUOUS_SLAB_HEAD_QD1_N,
        ),
    ] {
        let (mut model, edge) =
            fixture::column_with_slab_sensitive_head(column::column_model(false, 0.0), continuous);
        let time_history_mechanism = |model: &Model| {
            time_history_column_mechanism_sum_my(
                model,
                &ColumnMechanismTopology::new(model),
                &model.elements[COLUMN.index()],
                [Some(-100_000.0); 2],
                [Some(-100_000.0); 2],
            )
            .unwrap()
        };
        let qd2 = |model: &Model| {
            check_stations(
                model,
                QdMethod::Qd2,
                Some(-100_000.0),
                forces,
                &[0.0, 0.5, 1.0],
            )
        };
        let bare_qd2 = qd2(&model);
        let bare_th = time_history_mechanism(&model);
        assert_qd1_shear_ratio(&model, bare_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
        fixture::attach_slab(&mut model, edge);
        assert_qd1_shear_ratio(&model, slab_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
        let slab_th = time_history_mechanism(&model);
        assert_near(slab_th.qy.unwrap().unwrap() / height_mm, slab_n, 1e-6);
        assert_eq!(slab_th.qz.unwrap(), bare_th.qz.unwrap());

        for method in [QdMethod::Qd1, QdMethod::Min] {
            let rows = check_stations(&model, method, Some(-100_000.0), forces, &[0.0, 0.5, 1.0]);
            let [CheckOutcome::Checked(result)] = &rows[..] else {
                panic!("{rows:?}");
            };
            let detail = &result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Shear)
                .unwrap()
                .detail;
            let adopted_n = if method == QdMethod::Qd1 {
                slab_n
            } else {
                1500.0
            };
            assert!(
                detail.contains("Qy方向の柱頭") && detail.contains("梁端降伏曲げを確認できない"),
                "{detail}"
            );
            assert!(
                detail.contains(&format!("採用した設計用せん断力 {adopted_n:.1} N")),
                "{detail}"
            );
            assert!(!detail.contains("Qz方向"), "{detail}");
        }
        assert_eq!(bare_qd2, qd2(&model));
        let absent_end = check(&model, QdMethod::Qd1, Some(-100_000.0), forces);
        assert!(
            absent_end.iter().any(|outcome| matches!(
                outcome, CheckOutcome::Skipped { reason }
                    if reason.contains("Qy方向") && reason.contains("柱端の重力参照軸力が不足")
            )),
            "{absent_end:?}"
        );
        let mut bending_ng = forces;
        bending_ng[5] = 300e6;
        let rows = check(&model, QdMethod::Qd1, Some(-100_000.0), bending_ng);
        assert!(rows.iter().any(|outcome| matches!(
            outcome, CheckOutcome::Checked(result)
                if result.components.iter().any(|c| c.kind == CheckKind::AxialBending && c.ratio > 1.0)
        )), "{rows:?}");
    }
}

#[test]
fn qd1_keeps_an_independent_girder_candidate_when_the_other_end_supports_a_slab() {
    for (head_bars, foot_bars, slab_edge, expected_n) in [
        (
            2,
            1,
            [NodeId(1), NodeId(2)],
            reference::REVERSIBLE_ONE_BAR_FOOT_QD1_N,
        ),
        (4, 1, [NodeId(0), NodeId(8)], 227_114.990_935_399),
        (
            2,
            1,
            [NodeId(0), NodeId(7)],
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N,
        ),
    ] {
        let mut model = fixture::column_with_reversible_girders(
            column::column_model(false, 0.0),
            head_bars,
            foot_bars,
        );
        fixture::attach_slab(&mut model, slab_edge);
        assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
    }
}

#[test]
fn qd1_new_head_yield_requires_the_girders_to_bend_about_the_column_axis() {
    for (far_y, far_z, reference, expected_n) in [
        (
            0.0,
            1500.0,
            [0.0, 0.0, 1.0],
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N,
        ),
        (
            0.0,
            1600.0,
            [0.0, 0.0, 1.0],
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N,
        ),
        (750.0, 1500.0, [0.0, 0.0, 1.0], 227_114.990_935_399),
        (0.0, 1500.0, [0.0, 1.0, 0.0], 227_114.990_935_399),
        (0.0, 1500.0, [1.0, 0.0, 1e-13], 227_114.990_935_399),
    ] {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 4);
        model.nodes[2].coord = [4000.0, far_y, far_z];
        model.elements[1].local_axis.ref_vector = reference;
        assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
    }

    let mut prior = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 4);
    let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut prior.sections[0].shape else {
        unreachable!();
    };
    rebar.x = vec![4];
    rebar.y = vec![4];
    prior.elements[1].local_axis.ref_vector = [0.0, 1.0, 0.0];
    assert_qd1_shear_ratio(
        &prior,
        208_303.442_920_260 / reference::COLUMN_ALLOWABLE_SHEAR_N,
    );
}

#[test]
fn qd1_keeps_the_planar_mechanisms_after_rotating_the_frame_off_global_axes() {
    for (x_direction, y_direction, scale) in [(3.0, 4.0, 5.0), (3.0, 1.0, 4.0)] {
        for (head_bars, expected_n) in [
            (2, reference::TWO_HEAD_GIRDERS_HALF_QD1_N),
            (4, reference::REVERSIBLE_THREE_BAR_FOOT_QD1_N),
        ] {
            let mut model = fixture::column_with_reversible_girders(
                column::column_model(false, 0.0),
                head_bars,
                3,
            );
            for node in &mut model.nodes {
                let [x, y, z] = node.coord;
                node.coord = [
                    (x_direction * x - y_direction * y) / scale,
                    (y_direction * x + x_direction * y) / scale,
                    z,
                ];
            }
            for elem in &mut model.elements {
                if elem.section == Some(SectionId(0)) {
                    elem.local_axis.ref_vector = [x_direction, y_direction, 0.0];
                }
            }
            for girder in [1, 5, 6, 7] {
                model.elements[girder].local_axis.ref_vector = [x_direction, y_direction, 1.0];
            }
            assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
        }
    }
}

#[test]
fn qd1_new_head_yield_requires_concrete_base_material() {
    for (column_bars, expected_n) in [(3, 227_114.990_935_399), (4, 208_303.442_920_260)] {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 4);
        let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
            unreachable!();
        };
        rebar.x = vec![column_bars];
        rebar.y = vec![column_bars];
        model.sections[GIRDER_SECTION.index()].material = Some(fixture::WRONG_REBAR);
        assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
    }
}

#[test]
fn qd1_new_head_yield_excludes_a_girder_connected_to_a_floor_shell_at_both_ends() {
    for (column_bars, shares_both, expected_n) in [
        (3, true, 227_114.990_935_399),
        (3, false, reference::TWO_HEAD_GIRDERS_HALF_QD1_N),
        (4, true, 289_718.136_062_014),
    ] {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 4);
        let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
            unreachable!();
        };
        rebar.x = vec![column_bars];
        rebar.y = vec![column_bars];
        let section = SectionId(model.sections.len() as u32);
        let mut slab =
            SectionShape::RcSlab { thickness: 150.0 }.to_section(section, "床シェル".into());
        slab.material = model.sections[0].material;
        model.sections.push(slab);

        let mut far = model.nodes[2].clone();
        far.id = NodeId(model.nodes.len() as u32);
        far.coord[1] = 4000.0;
        let far_id = far.id;
        model.nodes.push(far);
        let beam_end = if shares_both {
            NodeId(2)
        } else {
            let mut independent = model.nodes[2].clone();
            independent.id = NodeId(model.nodes.len() as u32);
            let independent_id = independent.id;
            model.nodes.push(independent);
            independent_id
        };
        let mut other_edge_node = model.nodes[3].clone();
        other_edge_node.id = NodeId(model.nodes.len() as u32);
        let other_edge = other_edge_node.id;
        model.nodes.push(other_edge_node);
        let mut shell = model.elements[fixture::GIRDER.index()].clone();
        shell.id = ElemId(model.elements.len() as u32);
        shell.kind = ElementKind::Shell;
        shell.section = Some(section);
        shell.nodes = [beam_end, far_id, other_edge, NodeId(1)]
            .into_iter()
            .collect();
        model.elements.push(shell);

        assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
    }
}

#[test]
fn qd1_excludes_foot_girder_yield_if_a_girder_section_is_rotated() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
    model.elements[6].local_axis.ref_vector = [0.0, 1.0, 0.0];
    assert_qd1_shear_ratio(
        &model,
        reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
    );
}

#[test]
fn qd1_excludes_foot_girder_yield_unless_the_continuation_and_beam_pair_are_confirmed() {
    let lower = 4;
    let left = 7;
    for variant in 0..10 {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
        match variant {
            0 => {
                let mut separate = model.nodes[0].clone();
                separate.id = NodeId(model.nodes.len() as u32);
                model.elements[lower].nodes[1] = separate.id;
                model.nodes.push(separate);
            }
            1 => model.nodes[5].coord[0] = 100.0,
            2 => model.elements[lower].end_offsets_mm[1][0] = 100.0,
            3 => model.elements[lower].end_cond[1] = EndCondition::Pinned,
            4 => model.nodes[0].restraint = Dof6Mask::FIXED,
            5 => model.nodes[1].support_spring = Some([0.0, 0.0, 0.0, 0.0, 1000.0, 0.0]),
            6 => model.constraints.push(Constraint::RigidLink {
                master: NodeId(0),
                slaves: vec![NodeId(7)],
                dofs: Dof6Mask::FIXED,
            }),
            7 => model.elements[left].end_cond[1] = EndCondition::Pinned,
            8 => model.nodes[8].coord[2] = 100.0,
            9 => {
                model.nodes[8].restraint = Dof6Mask::FREE;
                model.elements.pop();
                model.nodes.pop();
            }
            _ => unreachable!(),
        }
        assert_qd1_shear_ratio(
            &model,
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
        );
    }
}

#[test]
fn qd1_excludes_foot_girder_yield_with_another_skew_or_orthogonal_girder() {
    for x in [750.0, 0.0] {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
        model.girder_torsion = GirderTorsionMode::Keep;
        let mut far = model.nodes[0].clone();
        far.id = NodeId(model.nodes.len() as u32);
        far.coord = [x, 4000.0, 0.0];
        far.restraint = Dof6Mask::FIXED;
        let mut girder = model.elements[6].clone();
        girder.id = ElemId(model.elements.len() as u32);
        girder.nodes[1] = far.id;
        model.elements.push(girder);
        model.nodes.push(far);

        assert_qd1_shear_ratio(
            &model,
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
        );
    }
}

#[test]
fn qd1_excludes_foot_girder_yield_when_a_rigid_link_restrains_rotation_to_ground() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
    let mut rotation = Dof6Mask::FREE;
    rotation.set_fixed(Dof::Ry);
    model.constraints.push(Constraint::RigidLink {
        master: NodeId(5),
        slaves: vec![NodeId(0)],
        dofs: rotation,
    });

    assert_qd1_shear_ratio(
        &model,
        reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
    );
}

#[test]
fn qd1_excludes_foot_girder_yield_if_an_rc_shape_has_a_nonconcrete_base_material() {
    for member in [3, 4, 6] {
        let mut model =
            fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
        let section = model.elements[member].section.unwrap();
        let mut nonconcrete = model.sections[section.index()].clone();
        nonconcrete.id = SectionId(model.sections.len() as u32);
        nonconcrete.name = "主材料が鋼材の断面".into();
        nonconcrete.material = Some(fixture::WRONG_REBAR);
        model.elements[member].section = Some(nonconcrete.id);
        model.sections.push(nonconcrete);

        assert_qd1_shear_ratio(
            &model,
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
        );
    }
}

#[test]
fn qd1_excludes_foot_girder_yield_at_wall_plate_before_and_after_expansion() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
    fixture::add_wall_below_reversible_column(&mut model);
    assert!(model
        .elements
        .iter()
        .all(|elem| elem.kind != ElementKind::Wall));
    let (expanded, _, report) = sepika_core::wall_expand::expand_wall_elements(&model);
    assert_eq!(report.generated, 1);
    for model in [&model, &expanded] {
        assert_qd1_shear_ratio(
            model,
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N / reference::COLUMN_ALLOWABLE_SHEAR_N,
        );
    }
}

#[test]
fn qd1_excludes_foot_girder_yield_when_a_brace_has_offset_at_the_column_foot() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
    let section = SectionId(model.sections.len() as u32);
    let mut brace_section = SectionShape::SteelH {
        height: 250.0,
        width: 125.0,
        web_thick: 6.0,
        flange_thick: 9.0,
        root_r: Some(8.0),
    }
    .to_section(section, "柱脚のブレース".into());
    brace_section.frame_use = Some(FrameSectionUse::Brace);
    brace_section.material = Some(fixture::WRONG_REBAR);
    model.sections.push(brace_section);

    let mut brace = model.elements[COLUMN.index()].clone();
    brace.id = ElemId(model.elements.len() as u32);
    brace.kind = ElementKind::Brace {
        tension_only: false,
    };
    brace.nodes = [NodeId(9), NodeId(0)].into_iter().collect();
    brace.end_cond = [EndCondition::Pinned; 2];
    brace.section = Some(section);
    model.elements.push(brace);

    for (offsets, expected_n) in [
        ([[0.0; 3]; 2], reference::REVERSIBLE_ONE_BAR_FOOT_QD1_N),
        (
            [[0.0, 0.0, 100.0], [0.0; 3]],
            reference::REVERSIBLE_ONE_BAR_FOOT_QD1_N,
        ),
        (
            [[0.0; 3], [0.0, 0.0, 100.0]],
            reference::TWO_HEAD_GIRDERS_HALF_QD1_N,
        ),
    ] {
        model.elements.last_mut().unwrap().end_offsets_mm = offsets;
        assert_qd1_shear_ratio(&model, expected_n / reference::COLUMN_ALLOWABLE_SHEAR_N);
    }
}

#[test]
fn qd1_keeps_existing_foot_girder_yield_when_the_head_has_no_direct_upper_column() {
    let mut model = fixture::column_with_reversible_girders(column::column_model(false, 0.0), 4, 1);
    model.elements[3].nodes[0] = NodeId(2);
    model.nodes[4].coord[0] = model.nodes[2].coord[0];
    let assert_foot_yield = |model: &Model| {
        model.validate().unwrap();
        assert_near(
            column_qd1_by_direction(model, [-100_000.0; 2], 1500.0)[0],
            reference::REVERSIBLE_ONE_BAR_FOOT_QD1_N,
            1e-6,
        );
    };
    assert_foot_yield(&model);
    model.constraints.push(Constraint::RigidLink {
        master: NodeId(1),
        slaves: vec![NodeId(2)],
        dofs: Dof6Mask::FIXED,
    });
    assert_foot_yield(&model);
}

#[test]
fn qd1_sloped_fiber_girder_contributes_without_counting_as_upper_column() {
    let mut model = yielding_head_model();
    fixture::add_column_above(&mut model, NodeId(1), ElementKind::Fiber);
    let top = model.nodes.last_mut().unwrap();
    top.coord[0] = 3000.0;
    let mut section = model.sections[GIRDER_SECTION.index()].clone();
    section.id = SectionId(2);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!();
    };
    rebar.top = vec![1];
    rebar.bottom = vec![1];
    let sloped_girder = model.elements.last_mut().unwrap();
    sloped_girder.section = Some(section.id);
    sloped_girder.local_axis.ref_vector = [0.0, 0.0, 1.0];
    model.sections.push(section);
    // 柱頭は終端で、斜めの大梁は主筋が半数なので平梁の半分を寄与する。
    assert_qd1_shear_ratio(&model, 0.7729675715693718);
}

fn assert_analyzed_column_qd1(mut model: Model, expected_qd1_n: f64, expected_pass: bool) {
    apply_auto_rigid_zones(&mut model, &RigidZoneRule::default());
    assert_eq!(
        model.elements[COLUMN.index()]
            .rigid_zone
            .clear_span_from(1700.0),
        Some(1400.0)
    );
    let [dead, live, seismic] =
        [0, 1, 2].map(|id| linear_static_once(&model, LoadCaseId(id)).unwrap());
    let gravity = superpose_static(&[(&dead, 1.0), (&live, 1.0)]).unwrap();
    let short = superpose_static(&[(&gravity, 1.0), (&seismic, 1.0)]).unwrap();
    let column_forces = short
        .member_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    for (_, force) in &column_forces.1.at {
        assert!((force[0] + 200_000.0).abs() < 1e-6, "{force:?}");
        assert!((force[1].abs() - 1000.0).abs() < 1e-6, "{force:?}");
    }
    let report = run_member_design_checks(
        &model,
        std::slice::from_ref(column_forces),
        &short.panel_moments,
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: QdMethod::Qd1,
            long_member_forces: Some(&gravity.member_forces),
            girder_column_face_forces: &short.girder_column_face_forces,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.member_checks.len(), 3);
    for (id, _, row) in &report.member_checks {
        assert_eq!(*id, COLUMN);
        let CheckOutcome::Checked(result) = row else {
            panic!("{row:?}");
        };
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        assert!(shear.detail.contains("QAy=270029.5 N"), "{shear:?}");
        let expected_ratio = expected_qd1_n / reference::COLUMN_ALLOWABLE_SHEAR_N;
        let qd1_n = shear.ratio * reference::COLUMN_ALLOWABLE_SHEAR_N;
        assert!(
            (qd1_n - expected_qd1_n).abs() < 1e-6,
            "QD1={qd1_n} N, 期待値={expected_qd1_n} N"
        );
        assert!((shear.ratio - expected_ratio).abs() < 1e-12, "{shear:?}");
        assert_eq!(result.ok(), expected_pass, "{result:?}");
    }
}

#[test]
fn qd1_counts_both_opposing_girders_for_all_elastic_representations() {
    for (positive, negative) in [
        (ElementKind::Beam, ElementKind::Beam),
        (ElementKind::Fiber, ElementKind::Beam),
        (ElementKind::Beam, ElementKind::MultiSpring),
        (ElementKind::MultiSpring, ElementKind::Fiber),
    ] {
        let mut model =
            fixture::column_with_opposing_head_girders(column::column_model(false, 0.0), negative);
        model.elements[fixture::GIRDER.index()].kind = positive;
        model.validate().unwrap();
        assert_analyzed_column_qd1(model.clone(), reference::TWO_GIRDER_QD1_N, false);

        fixture::add_column_above(&mut model, NodeId(1), ElementKind::Beam);
        model.validate().unwrap();
        assert_analyzed_column_qd1(model, reference::CONTINUOUS_TWO_GIRDER_QD1_N, true);
    }
}

fn short_column_with_single_girder() -> Model {
    let mut model = yielding_head_model();
    model.elements.pop();
    model.nodes.pop();
    for node in &mut model.nodes[1..] {
        node.coord[2] = 1200.0;
    }
    model.stories[1].elevation = Some(1200.0);
    model.nodes[2].restraint = Dof6Mask::FIXED;
    model.girder_torsion = GirderTorsionMode::Keep;
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [1_000.0, 0.0, -100_000.0, 0.0, 0.0, 0.0],
    )];
    model
}

fn assert_near(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "結果={actual}, 期待値={expected}"
    );
}

fn column_qd1_by_direction(model: &Model, n: [f64; 2], clear_height_mm: f64) -> [f64; 2] {
    let sum = compute_column_mechanism_sum_my(
        model,
        &ColumnMechanismTopology::new(model),
        &model.elements[COLUMN.index()],
        n[0],
        n[1],
        n[0],
        n[1],
        1.0,
    )
    .unwrap();
    [sum.qy, sum.qz].map(|moment| moment.unwrap().unwrap() / clear_height_mm)
}

#[test]
fn qd1_short_column_with_pinned_girder_fails_while_rigid_connection_passes() {
    // 節点平衡と曲げ・せん断たわみから求めた軸力・梁端曲げを使う独立計算。
    for (condition, n, m, qd1, ratio, pass) in [
        (
            EndCondition::Pinned,
            -99_875.610_048,
            0.0,
            [283_833.701_898, 189_222.467_932],
            1.051_121_033_179,
            false,
        ),
        (
            EndCondition::Fixed,
            -99_603.727_165,
            737_683.966,
            [221_156.702_790, 189_134.978_905],
            0.880_509_590_310,
            true,
        ),
    ] {
        for connected_end in [0, 1] {
            let mut model = short_column_with_single_girder();
            let girder = &mut model.elements[fixture::GIRDER.index()];
            if connected_end == 1 {
                girder.nodes.swap(0, 1);
            }
            girder.end_cond[connected_end] = condition;
            model.validate().unwrap();
            let solved = linear_static_once(&model, LoadCaseId(0)).unwrap();
            let forces_at = |id| {
                &solved
                    .member_forces
                    .iter()
                    .find(|(elem, _)| *elem == id)
                    .unwrap()
                    .1
                    .at
            };
            let girder_at = forces_at(fixture::GIRDER);
            let (xi, girder_force) = if connected_end == 0 {
                girder_at.first().unwrap()
            } else {
                girder_at.last().unwrap()
            };
            assert_eq!(*xi, connected_end as f64);
            assert_near(girder_force[5].abs(), m, 1e-3);
            let column_at = forces_at(COLUMN);
            let axial = [column_at[0].1[0], column_at.last().unwrap().1[0]];
            for axial_n in axial {
                assert_near(axial_n, n, 1e-6);
            }
            for (actual, expected) in column_qd1_by_direction(&model, axial, 1200.0)
                .into_iter()
                .zip(qd1)
            {
                assert_near(actual, expected, 1e-6);
            }
            let report = run_member_design_checks(
                &model,
                &solved.member_forces,
                &solved.panel_moments,
                &MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    qd_method: QdMethod::Qd1,
                    long_member_forces: Some(&solved.member_forces),
                    ..Default::default()
                },
            )
            .unwrap();
            let column_checks: Vec<_> = report
                .member_checks
                .iter()
                .filter(|(elem, _, _)| *elem == COLUMN)
                .collect();
            assert_eq!(column_checks.len(), 3);
            for (_, _, outcome) in column_checks {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}");
                };
                let shear = result
                    .components
                    .iter()
                    .find(|c| c.kind == CheckKind::Shear)
                    .unwrap();
                assert_near(shear.ratio, ratio, 1e-12);
                assert_eq!(result.ok(), pass, "{result:?}");
            }
        }
    }
}

#[test]
fn qd1_uses_only_opposing_girders_that_transfer_bending_at_the_column() {
    use reference::{BOTH_COLUMN_ENDS_QD1_N, ONE_GIRDER_QD1_N, TWO_GIRDER_QD1_N};
    let kinds = [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ];
    for (positive, negative) in kinds.into_iter().zip(kinds.into_iter().cycle().skip(1)) {
        let mut model =
            fixture::column_with_opposing_head_girders(column::column_model(false, 0.0), negative);
        model.elements[fixture::GIRDER.index()].kind = positive;
        let i = fixture::GIRDER.index();
        let j = ElemId(3).index();
        for (i_ends, j_ends, expected) in [
            (
                [EndCondition::Fixed; 2],
                [EndCondition::Fixed; 2],
                TWO_GIRDER_QD1_N,
            ),
            (
                [EndCondition::Fixed, EndCondition::Pinned],
                [EndCondition::Pinned, EndCondition::Fixed],
                TWO_GIRDER_QD1_N,
            ),
            (
                [EndCondition::Pinned, EndCondition::Fixed],
                [EndCondition::Fixed; 2],
                ONE_GIRDER_QD1_N,
            ),
            (
                [EndCondition::Fixed; 2],
                [EndCondition::Fixed, EndCondition::Pinned],
                ONE_GIRDER_QD1_N,
            ),
            (
                [
                    EndCondition::SemiRigid { k_theta: 0.0 },
                    EndCondition::Fixed,
                ],
                [EndCondition::Fixed, EndCondition::Pinned],
                BOTH_COLUMN_ENDS_QD1_N,
            ),
            (
                [EndCondition::Fixed; 2],
                [
                    EndCondition::Fixed,
                    EndCondition::SemiRigid { k_theta: 1e8 },
                ],
                BOTH_COLUMN_ENDS_QD1_N,
            ),
        ] {
            model.elements[i].end_cond = i_ends;
            model.elements[j].end_cond = j_ends;
            model.validate().unwrap();
            let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
            assert_near(strong, expected, 1e-6);
            assert_near(weak, 162_630.279_382_081, 1e-6);
        }
    }
}

#[test]
fn qd1_accounts_for_the_analytical_rigid_zone_at_the_connected_end() {
    use reference::{BOTH_COLUMN_ENDS_QD1_N, ONE_GIRDER_QD1_N, TWO_GIRDER_QD1_N};
    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    for (kind, id, end) in [
        (ElementKind::Beam, fixture::GIRDER, 0),
        (ElementKind::Fiber, fixture::GIRDER, 0),
        (ElementKind::MultiSpring, ElemId(3), 1),
    ] {
        for (condition, lengths_mm, enabled, expected) in [
            (EndCondition::Fixed, [100.0, 0.0], true, TWO_GIRDER_QD1_N),
            (
                EndCondition::Pinned,
                [100.0, 0.0],
                true,
                BOTH_COLUMN_ENDS_QD1_N,
            ),
            (EndCondition::Pinned, [100.0, 0.0], false, ONE_GIRDER_QD1_N),
            (EndCondition::Pinned, [0.0, 100.0], true, ONE_GIRDER_QD1_N),
            (
                EndCondition::Pinned,
                [2000.0, 2000.0],
                true,
                ONE_GIRDER_QD1_N,
            ),
        ] {
            let girder = &mut model.elements[id.index()];
            girder.kind = kind;
            girder.end_cond[end] = condition;
            girder.rigid_zone.length_i = lengths_mm[end];
            girder.rigid_zone.length_j = lengths_mm[1 - end];
            model.stress_cfg.consider_rigid_zones = enabled;
            model.validate().unwrap();
            let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
            assert_near(strong, expected, 1e-6);
            assert_near(weak, 162_630.279_382_081, 1e-6);
        }
        let girder = &mut model.elements[id.index()];
        girder.end_cond[end] = EndCondition::Fixed;
        girder.rigid_zone = Default::default();
    }
}

#[test]
fn qd1_accounts_for_rigid_arm_moment_beyond_the_plan_direction_of_a_girder() {
    use reference::{BOTH_COLUMN_ENDS_QD1_N, ONE_GIRDER_QD1_N};

    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    let girder = fixture::GIRDER.index();
    let far = model.elements[girder].nodes[1].index();
    let head_z_mm = model.nodes[NodeId(1).index()].coord[2];
    let weak_beam_qd1_n = 162_630.279_382_081;
    model.elements[girder].rigid_zone.length_i = 100.0;
    for (condition, unarmed_strong_qd1_n) in [
        (EndCondition::Pinned, ONE_GIRDER_QD1_N),
        (
            EndCondition::SemiRigid { k_theta: 1e8 },
            BOTH_COLUMN_ENDS_QD1_N,
        ),
    ] {
        model.elements[girder].end_cond[0] = condition;
        for (rise_mm, enabled, expected) in [
            (0.0, true, [BOTH_COLUMN_ENDS_QD1_N, weak_beam_qd1_n]),
            (1000.0, true, [BOTH_COLUMN_ENDS_QD1_N, 189_308.326_858_618]),
            (1000.0, false, [unarmed_strong_qd1_n, weak_beam_qd1_n]),
        ] {
            model.nodes[far].coord[2] = head_z_mm + rise_mm;
            model.stress_cfg.consider_rigid_zones = enabled;
            model.validate().unwrap();
            let qd1 = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
            for (actual, expected) in qd1.into_iter().zip(expected) {
                assert_near(actual, expected, 1e-6);
            }
        }
    }
}

#[test]
fn qd1_keeps_the_orthogonal_girder_only_for_an_exactly_parallel_rigid_arm() {
    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    model.elements[COLUMN.index()].local_axis.ref_vector = [3500.0, 2300.0, 0.0];
    model.nodes[3].coord[..2].copy_from_slice(&[-2300.0, 3500.0]);
    model.nodes[4].coord[..2].copy_from_slice(&[-3500.0, -2300.0]);
    let girder = &mut model.elements[fixture::GIRDER.index()];
    girder.end_cond[0] = EndCondition::Pinned;
    girder.rigid_zone.length_i = 100.0;
    model.stress_cfg.consider_rigid_zones = true;
    for (xy_mm, weak_qd1_n) in [
        ([3500.0, 2300.0], 162_630.279_382_081),
        ([5250.0, 3450.0], 162_630.279_382_081),
        (
            // 積を f64 で丸めると外積が 0 になる斜交例。
            [
                f64::from_bits(5250.0_f64.to_bits() - 4),
                f64::from_bits(3450.0_f64.to_bits() - 5),
            ],
            189_308.326_858_618,
        ),
    ] {
        model.nodes[2].coord[..2].copy_from_slice(&xy_mm);
        model.validate().unwrap();
        let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
        assert_near(strong, reference::BOTH_COLUMN_ENDS_QD1_N, 1e-6);
        assert_near(weak, weak_qd1_n, 1e-6);
    }

    model.nodes[2].coord[..2].copy_from_slice(&[3500.0, 2300.0]);
    for (column_x_mm, far_offset_x_mm, weak_qd1_n) in [
        (1e-14, 0.0, 189_308.326_858_618),
        (0.0, 1e-14, 189_308.326_858_618),
        (1e-14, 1e-14, 162_630.279_382_081),
    ] {
        model.nodes[0].coord[0] = column_x_mm;
        model.nodes[1].coord[0] = column_x_mm;
        model.elements[fixture::GIRDER.index()].end_offsets_mm[1][0] = far_offset_x_mm;
        model.validate().unwrap();
        let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
        assert_near(strong, reference::BOTH_COLUMN_ENDS_QD1_N, 1e-6);
        assert_near(weak, weak_qd1_n, 1e-6);
    }
}

#[test]
fn qd1_uses_both_column_ends_for_an_inclined_column_with_a_pinned_girder_and_rigid_arm() {
    use reference::{BOTH_COLUMN_ENDS_QD1_N, ONE_GIRDER_QD1_N};

    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    model.elements[COLUMN.index()].local_axis.ref_vector = [3500.0, 2300.0, 0.0];
    model.nodes[0].coord = [-1000.0, 0.0, -3000.0];
    model.nodes[1].coord = [0.0; 3];
    // 単位化済みの柱の局所z軸を4096倍した座標。
    model.nodes[2].coord = [
        -2_212.664_151_288_898,
        3_367.097_621_526_583_7,
        737.554_717_096_299_4,
    ];
    model.nodes[3].coord = [-2300.0, 3500.0, 0.0];
    model.nodes[4].coord = [-3500.0, -2300.0, 0.0];
    let girder = &mut model.elements[fixture::GIRDER.index()];
    girder.end_cond[0] = EndCondition::Pinned;
    girder.rigid_zone.length_i = 100.0;
    model.validate().unwrap();
    for (enabled, expected) in [
        (false, [ONE_GIRDER_QD1_N, 162_630.279_382_081]),
        (true, [BOTH_COLUMN_ENDS_QD1_N, 189_308.326_858_618]),
    ] {
        model.stress_cfg.consider_rigid_zones = enabled;
        let qd1 = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
        for (actual, expected) in qd1.into_iter().zip(expected) {
            assert_near(actual, expected, 1e-6);
        }
    }
}

#[test]
fn qd1_uses_column_ends_when_xyz_offsets_make_the_girder_mechanism_uncertain() {
    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    let i = fixture::GIRDER.index();
    for (condition, offsets, expected_strong, expected_weak) in [
        (
            EndCondition::Pinned,
            [[0.0, 200.0, 0.0]; 2],
            reference::BOTH_COLUMN_ENDS_QD1_N,
            189_308.326_858_618,
        ),
        (
            EndCondition::Fixed,
            [[0.0, 0.0, 200.0], [0.0; 3]],
            reference::BOTH_COLUMN_ENDS_QD1_N,
            189_308.326_858_618,
        ),
        (
            EndCondition::Fixed,
            [[0.0; 3], [-4000.0, 5000.0, 0.0]],
            reference::BOTH_COLUMN_ENDS_QD1_N,
            189_308.326_858_618,
        ),
        (
            EndCondition::Pinned,
            [[0.0; 3], [-4000.0, 5000.0, 0.0]],
            reference::ONE_GIRDER_QD1_N,
            162_630.279_382_081,
        ),
    ] {
        model.elements[i].end_cond[0] = condition;
        model.elements[i].end_offsets_mm = offsets;
        model.validate().unwrap();
        let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
        assert_near(strong, expected_strong, 1e-6);
        assert_near(weak, expected_weak, 1e-6);
    }
    model.elements[i].end_offsets_mm = [[0.0; 3]; 2];
    model.elements[i].end_cond[0] = EndCondition::Fixed;
    for end in [0, 1] {
        model.elements[COLUMN.index()].end_offsets_mm = [[0.0; 3]; 2];
        model.elements[COLUMN.index()].end_offsets_mm[end] = [0.0, 100.0, 0.0];
        model.validate().unwrap();
        let [strong, weak] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
        assert_near(strong, reference::BOTH_COLUMN_ENDS_QD1_N, 1e-6);
        assert_near(weak, 189_308.326_858_618, 1e-6);
    }
}

#[test]
fn pinned_flexible_end_can_transfer_nodal_moment_through_a_rigid_arm_or_xyz_offset() {
    let mut model = short_column_with_single_girder();
    let girder = fixture::GIRDER.index();
    model.elements[girder].end_cond[0] = EndCondition::Pinned;
    model.elements[girder].rigid_zone.length_i = 100.0;
    model.stress_cfg.consider_rigid_zones = true;
    let response_for = |model: &Model, dof, disp_mm| {
        let mut displacement = [0.0; 12];
        displacement[dof] = disp_mm;
        build_behavior(&model.elements[girder], model)
            .frame_load_response(&displacement, &[])
            .unwrap()
            .unwrap()
    };
    let response = response_for(&model, 2, 1.0);
    assert_near(response.flex_face_force_local[5], 0.0, 1e-6);
    assert_near(response.nodal_residual.data[2], 6_176.044_109_486, 1e-6);
    assert_near(response.nodal_residual.data[4], -617_604.410_948_605, 1e-6);
    model.stress_cfg.consider_rigid_zones = false;
    let response = response_for(&model, 2, 1.0);
    assert_near(response.flex_face_force_local[5], 0.0, 1e-6);
    assert_near(response.nodal_residual.data[4], 0.0, 1e-6);
    model.elements[girder].end_offsets_mm = [[0.0, 0.0, 200.0]; 2];
    let response = response_for(&model, 0, 0.01);
    assert_near(response.flex_face_force_local[5], 0.0, 1e-6);
    assert_near(response.nodal_residual.data[0], 10_350.0, 1e-6);
    assert_near(response.nodal_residual.data[4], 2_070_000.0, 1e-6);

    model.elements[girder].end_offsets_mm = [[0.0, 200.0, 0.0]; 2];
    let response = response_for(&model, 2, 1.0);
    assert_near(response.flex_face_force_local[5], 0.0, 1e-6);
    assert_near(response.nodal_residual.data[2], 5_729.064_160_598, 1e-6);
    assert_near(response.nodal_residual.data[3], 1_145_812.832_119_661, 1e-6);

    model.elements[girder].end_offsets_mm[0] = [0.0; 3];
    let response = response_for(&model, 2, 1.0);
    for moment in &response.nodal_residual.data[3..6] {
        assert_near(*moment, 0.0, 1e-6);
    }

    model.elements[girder].end_offsets_mm = [[0.0; 3]; 2];
    model.nodes[2].coord[2] += 1000.0;
    model.stress_cfg.consider_rigid_zones = true;
    let response = response_for(&model, 1, 1.0);
    assert_near(response.flex_face_force_local[4], 0.0, 1e-6);
    assert_near(response.nodal_residual.data[1], 1_424.830_746_940, 1e-6);
    assert_near(response.nodal_residual.data[3], -34_557.221_578_003, 1e-6);
}

#[test]
fn qd1_uses_both_column_ends_if_an_opposing_girder_lacks_yield_strength() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model =
            fixture::column_with_opposing_head_girders(column::column_model(false, 0.0), kind);
        fixture::remove_opposite_girder_yield_strength(&mut model);
        model.validate().unwrap();

        assert_analyzed_column_qd1(model, reference::BOTH_COLUMN_ENDS_QD1_N, false);
    }
}

#[test]
fn qd1_uses_both_column_ends_only_in_the_direction_of_unclassified_frame_members() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for (unclassified, expected_qd1_n) in [
            (ElemId(3), reference::BOTH_COLUMN_ENDS_QD1_N),
            (ElemId(2), reference::TWO_GIRDER_QD1_N),
        ] {
            let mut model = fixture::column_with_opposing_head_girders(
                column::column_model(false, 0.0),
                ElementKind::Beam,
            );
            let mut section = model.sections[GIRDER_SECTION.index()].clone();
            section.id = SectionId(2);
            section.name = "断面用途未設定の汎用断面".into();
            section.shape = None;
            section.property_basis = Default::default();
            section.frame_use = None;
            let member = &mut model.elements[unclassified.index()];
            member.kind = kind;
            member.section = Some(section.id);
            model.sections.push(section);

            assert_analyzed_column_qd1(model, expected_qd1_n, false);
        }
    }
}

#[test]
fn qd1_head_with_unconfirmed_column_connection_uses_column_moment() {
    let mut model = yielding_head_model();
    let mut base_girder = model.elements[fixture::GIRDER.index()].clone();
    let mut far = model.nodes[2].clone();
    far.id = NodeId(4);
    far.coord[2] = 0.0;
    model.nodes.push(far);
    base_girder.id = ElemId(3);
    base_girder.nodes = [NodeId(0), NodeId(4)].into_iter().collect();
    model.elements.push(base_girder);

    let mut offset_base = model.nodes[1].clone();
    offset_base.id = NodeId(5);
    offset_base.coord[0] += 100.0;
    model.nodes.push(offset_base);
    fixture::add_column_above(&mut model, NodeId(5), ElementKind::Beam);
    assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);

    for dofs in [Dof6Mask::FREE, Dof6Mask(0b1100_0000)] {
        model.constraints = vec![Constraint::RigidLink {
            master: NodeId(1),
            slaves: vec![NodeId(5)],
            dofs,
        }];
        assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);
    }

    let mut rotation_only = Dof6Mask::FREE;
    rotation_only.set_fixed(Dof::Rz);
    for constraint in [
        Constraint::RigidLink {
            master: NodeId(1),
            slaves: vec![NodeId(5)],
            dofs: Dof6Mask::FIXED,
        },
        Constraint::RigidLink {
            master: NodeId(1),
            slaves: vec![NodeId(5)],
            dofs: rotation_only,
        },
        Constraint::Mpc {
            master: NodeId(5),
            terms: vec![(NodeId(1), Dof::Rx, 1.0)],
        },
    ] {
        model.constraints = vec![constraint];
        // 両端柱降伏の QD1=227.114991 kN（強軸）。
        assert_qd1_shear_ratio(&model, 0.8410746938296013);
    }

    for (head_fixed, upper_fixed) in [(true, true), (true, false), (false, true)] {
        model.nodes[1].restraint.set(Dof::Rz, head_fixed);
        model.nodes[5].restraint.set(Dof::Rz, upper_fixed);
        for constraint in [
            Constraint::RigidLink {
                master: NodeId(1),
                slaves: vec![NodeId(5)],
                dofs: rotation_only,
            },
            Constraint::Mpc {
                master: NodeId(5),
                terms: vec![(NodeId(1), Dof::Rz, 1.0)],
            },
        ] {
            model.constraints = vec![constraint];
            assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);
        }
    }
}

#[test]
fn qd1_uses_the_explicit_constraint_that_survives_reduction() {
    let mut model = yielding_head_model();
    let head = NodeId(1);
    let girder_end = NodeId(2);
    let upper_base = NodeId(4);
    let mut offset_base = model.nodes[head.index()].clone();
    offset_base.id = upper_base;
    offset_base.coord[0] += 100.0;
    model.nodes.push(offset_base);
    fixture::add_column_above(&mut model, upper_base, ElementKind::Beam);

    let mut dofs = Dof6Mask::FREE;
    dofs.set_fixed(Dof::Ux);
    let link_to_upper_base = |master| Constraint::RigidLink {
        master,
        slaves: vec![upper_base],
        dofs,
    };
    let head_link = link_to_upper_base(head);
    let far_link = link_to_upper_base(girder_end);
    let head_mpc = Constraint::Mpc {
        master: upper_base,
        terms: vec![(head, Dof::Ux, 1.0)],
    };
    for constraints in [
        vec![far_link.clone()],
        vec![head_link.clone(), far_link.clone()],
        vec![far_link.clone(), head_mpc],
        vec![
            Constraint::rigid_diaphragm(StoryId(1), girder_end, vec![upper_base]),
            head_link.clone(),
        ],
    ] {
        model.constraints = constraints;
        assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);
    }

    for constraints in [
        vec![far_link, head_link.clone()],
        vec![
            Constraint::rigid_diaphragm(StoryId(1), girder_end, vec![head]),
            head_link,
        ],
    ] {
        model.constraints = constraints;
        assert_qd1_shear_ratio(&model, 0.8410746938296013);
    }
}

#[test]
fn invalid_girder_rebar_category_blocks_only_its_assigned_column_shear_direction() {
    for (kind, girder_xy, axis) in [
        (ElementKind::Beam, [4000.0, 0.0], "Qy"),
        (ElementKind::Fiber, [0.0, 4000.0], "Qz"),
        (ElementKind::MultiSpring, [4000.0, 4000.0], "Qy"),
    ] {
        let mut model = fixture::column_with_girder(column::column_model(false, 0.0));
        model.elements[fixture::GIRDER.index()].kind = kind;
        model.nodes[2].coord[..2].copy_from_slice(&girder_xy);
        let demand = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let rows = check(&model, method, Some(demand[0]), demand);
            let [CheckOutcome::Skipped { reason }] = &rows[..] else {
                panic!("{rows:?}");
            };
            for expected in [
                &format!("RC柱 ID {} の {axis} 方向", COLUMN.0),
                &format!("大梁 ID {}", fixture::GIRDER.0),
                "断面「接続大梁」",
                "主筋材料「SD345」",
                "材料区分は「鋼材」",
                "断面タブで主筋に「鉄筋」区分の材料を割り当て",
            ] {
                assert!(reason.contains(expected), "{reason}");
            }
            assert_eq!(reason.matches("方向のせん断検定").count(), 1, "{reason}");
        }

        let qd2 = check(&model, QdMethod::Qd2, Some(demand[0]), demand);
        let non_seismic = check(&model, QdMethod::Qd1, None, demand);
        model.sections[GIRDER_SECTION.index()].rebar_material = Some(REBAR);
        for (method, gravity) in [
            (QdMethod::Qd1, Some(demand[0])),
            (QdMethod::Min, Some(demand[0])),
            (QdMethod::Qd2, Some(demand[0])),
            (QdMethod::Qd1, None),
        ] {
            let rows = check(&model, method, gravity, demand);
            assert!(
                matches!(&rows[..], [CheckOutcome::Checked(result)] if result.ok()
                    && result.components.iter().any(|c| c.kind == CheckKind::Shear && c.ratio > 0.0)),
                "{rows:?}"
            );
            match (method, gravity) {
                (QdMethod::Qd2, _) => assert_eq!(rows, qd2),
                (_, None) => assert_eq!(rows, non_seismic),
                _ => {}
            }
        }
    }
}

#[test]
fn a_pinned_girder_needs_no_yield_strength_and_keeps_other_girder_diagnostics() {
    let mut model = fixture::column_with_opposing_head_girders(
        column::column_model(false, 0.0),
        ElementKind::MultiSpring,
    );
    let opposite = ElemId(3).index();
    fixture::remove_opposite_girder_yield_strength(&mut model);
    model.elements[opposite].end_cond[1] = EndCondition::Pinned;
    let [strong, _] = column_qd1_by_direction(&model, [-200_000.0; 2], 1400.0);
    assert_near(strong, reference::ONE_GIRDER_QD1_N, 1e-6);

    let invalid_section = model.elements[opposite].section.unwrap();
    model.sections[invalid_section.index()].rebar_material = Some(fixture::WRONG_REBAR);
    model.materials[fixture::WRONG_REBAR.index()].category =
        sepika_core::model::MaterialCategory::Steel;
    let demand = [-200_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
    for method in [QdMethod::Qd1, QdMethod::Min] {
        let good = check(&model, method, Some(demand[0]), demand);
        assert!(matches!(&good[..], [CheckOutcome::Checked(_)]), "{good:?}");
    }

    model.elements[fixture::GIRDER.index()].section = Some(invalid_section);
    for method in [QdMethod::Qd1, QdMethod::Min] {
        let rows = check(&model, method, Some(demand[0]), demand);
        let [CheckOutcome::Skipped { reason }] = &rows[..] else {
            panic!("{rows:?}");
        };
        assert!(
            reason.contains("Qy 方向") && reason.contains("大梁 ID 1"),
            "{reason}"
        );
        assert!(!reason.contains("大梁 ID 3"), "{reason}");
    }
}

#[test]
fn invalid_girder_rebar_category_preserves_independent_column_failures() {
    let model = fixture::column_with_girder(column::column_model(false, 0.0));
    let mut repaired = model.clone();
    repaired.sections[GIRDER_SECTION.index()].rebar_material = Some(REBAR);
    for (gravity, demand, failed_kind) in [
        (
            -100_000.0,
            [-100_000.0, 0.0, 0.0, 0.0, 0.0, 600.0e6],
            CheckKind::AxialBending,
        ),
        (
            -2_000_000.0,
            [-2_000_000.0, 0.0, 1_000_000.0, 0.0, 0.0, 0.0],
            CheckKind::Shear,
        ),
    ] {
        let rows = check(&model, QdMethod::Min, Some(gravity), demand);
        let [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }] = &rows[..] else {
            panic!("{rows:?}");
        };
        assert!(reason.contains("Qy 方向"), "{reason}");
        let [failed] = &result.components[..] else {
            panic!("{rows:?}");
        };
        assert_eq!(failed.kind, failed_kind);
        assert!(failed.ratio > 1.0);
        let rows = check(&repaired, QdMethod::Min, Some(gravity), demand);
        let [CheckOutcome::Checked(result)] = &rows[..] else {
            panic!("{rows:?}");
        };
        let baseline = result
            .components
            .iter()
            .find(|c| c.kind == failed_kind)
            .unwrap();
        assert_eq!(failed.ratio, baseline.ratio);
    }
}
