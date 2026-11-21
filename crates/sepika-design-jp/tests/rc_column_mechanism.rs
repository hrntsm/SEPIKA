#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;
#[path = "fixtures/rc_column_mechanism_model.rs"]
mod fixture;
#[path = "fixtures/rc_column_mechanism_reference.rs"]
mod reference;

use fixture::{COLUMN, GIRDER_SECTION, REBAR};
use reference::{CONTINUOUS_SHEAR_RATIO, TERMINAL_SHEAR_RATIO};
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, NodeId, SectionId, StoryId};
use sepika_core::model::{Constraint, ElementKind, FrameSectionUse, Model, Story};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;

fn check(
    model: &Model,
    method: QdMethod,
    gravity_n: Option<f64>,
    forces: [f64; 6],
) -> Vec<CheckOutcome> {
    let dist = |force| {
        [(
            COLUMN,
            MemberForces {
                at: vec![(0.5, force)],
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
    .map(|(elem, pos, row)| {
        assert_eq!((elem, pos), (COLUMN, 0.5));
        row
    })
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
    let rows = check(
        model,
        QdMethod::Qd1,
        Some(-100_000.0),
        [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0],
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
fn qd1_head_girder_search_retains_existing_beam_scope() {
    let mut model = yielding_head_model();
    fixture::add_column_above(&mut model, NodeId(1), ElementKind::Fiber);
    let top = model.nodes.last_mut().unwrap();
    top.coord[0] = 3000.0;
    let sloped_girder = model.elements.last_mut().unwrap();
    sloped_girder.section = Some(GIRDER_SECTION);
    sloped_girder.local_axis.ref_vector = [0.0, 0.0, 1.0];
    assert_qd1_shear_ratio(&model, TERMINAL_SHEAR_RATIO);
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
fn invalid_girder_rebar_category_blocks_only_its_assigned_column_shear_direction() {
    for (girder_xy, axis) in [
        ([4000.0, 0.0], "Qy"),
        ([0.0, 4000.0], "Qz"),
        ([4000.0, 4000.0], "Qy"),
    ] {
        let mut model = fixture::column_with_girder(column::column_model(false, 0.0));
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
