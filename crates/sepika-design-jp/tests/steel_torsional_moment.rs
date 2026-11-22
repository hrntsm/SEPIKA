use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm,
    MemberDesignCheckOptions, MemberForcesAt, MemberKind, SteelDesign,
};
use sepika_solver::statics::linear::linear_static_once;

const COLUMN: ElemId = ElemId(0);
const POSITIONS: [f64; 3] = [0.0, 0.5, 1.0];
const PURE_PIPE_REASON: &str = "規準に基づくねじり耐力と適用条件を確定できない";
const COMBINED_REASON: &str = "断面内同一点での組合せ条件が未確定";

fn pipe_column(mx_nmm: f64) -> Model {
    let mut section = SectionShape::SteelPipe {
        outer_dia: 200.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "円形鋼管 D200×10".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Column);

    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "直接鋼材".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        sections: vec![section],
        elements: vec![ElementData {
            id: COLUMN,
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        load_cases: vec![LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "柱頭のねじり".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, 0.0, 0.0, 0.0, mx_nmm],
            )],
            member: vec![],
        }],
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    }
}

fn assert_skipped(outcome: &CheckOutcome, expected: &str) {
    assert!(
        matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("Mx") && reason.contains(expected)),
        "{outcome:?}"
    );
}

#[test]
fn analysis_torque_reaches_all_three_public_design_positions() {
    for mx_nmm in [-100e6_f64, 0.0, 100e6] {
        let model = pipe_column(mx_nmm);
        let solved = linear_static_once(&model, LoadCaseId(0)).unwrap();
        let (_, forces) = solved
            .member_forces
            .iter()
            .find(|(id, _)| *id == COLUMN)
            .unwrap();
        for position in POSITIONS {
            let (_, [n, qy, qz, mx, my, mz]) =
                forces.at.iter().find(|(pos, _)| *pos == position).unwrap();
            assert!((mx.abs() - mx_nmm.abs()).abs() <= 1e-6);
            assert_eq!([*n, *qy, *qz, *my, *mz], [0.0; 5]);
        }
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.member_checks.len(), POSITIONS.len());
        for ((elem, pos, outcome), position) in report.member_checks.iter().zip(POSITIONS) {
            assert_eq!((*elem, *pos), (COLUMN, position));
            if mx_nmm == 0.0 {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}");
                };
                assert_eq!(result.ratio(), 0.0);
                assert!(result.ok());
            } else {
                assert_skipped(outcome, PURE_PIPE_REASON);
            }
        }
    }
}

#[test]
fn steel_check_requires_shape_and_action_specific_torsion_rules() {
    let model = pipe_column(0.0);
    let pipe = &model.sections[0];
    let h = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(1), "H形鋼".into());
    let ctx = DesignCtx {
        term: LoadTerm::Short,
        kind: MemberKind::Column,
        length: 3000.0,
        ..Default::default()
    };
    let check = |section: &Section, forces, ctx: &DesignCtx| {
        SteelDesign.check(
            &MemberForcesAt::from_local(0.5, forces),
            section,
            &model.materials[0],
            ctx,
        )
    };

    let torque_only = [0.0, 0.0, 0.0, 100e6, 0.0, 0.0];
    assert_skipped(&check(pipe, torque_only, &ctx), PURE_PIPE_REASON);
    assert_skipped(&check(&h, torque_only, &ctx), COMBINED_REASON);
    for component in [0, 1, 2, 4, 5] {
        let mut forces = torque_only;
        forces[component] = 1.0;
        assert_skipped(&check(pipe, forces, &ctx), COMBINED_REASON);
    }
    for mx_nmm in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_skipped(
            &check(pipe, [0.0, 0.0, 0.0, mx_nmm, 0.0, 0.0], &ctx),
            "非有限",
        );
    }
    assert_skipped(
        &check(pipe, [0.0, 0.0, 0.0, f64::MIN_POSITIVE, 0.0, 0.0], &ctx),
        PURE_PIPE_REASON,
    );

    let long_column = DesignCtx {
        length: 30_000.0,
        lk_y: Some(30_000.0),
        lk_z: Some(30_000.0),
        ..ctx
    };
    for (mx_nmm, expected) in [(100e6, COMBINED_REASON), (f64::NAN, "非有限")] {
        let outcome = check(pipe, [-10_000.0, 0.0, 0.0, mx_nmm, 0.0, 0.0], &long_column);
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        assert!(!result.ok());
        let provision = &result.components[0];
        assert_eq!(provision.kind, CheckKind::Provision);
        assert!(provision.detail.contains("Mx"), "{provision:?}");
        assert!(provision.detail.contains(expected), "{provision:?}");
    }

    let invalid_column = DesignCtx {
        lk_y: Some(f64::NAN),
        ..long_column
    };
    let outcome = check(
        pipe,
        [-10_000.0, 0.0, 0.0, 100e6, 0.0, 0.0],
        &invalid_column,
    );
    assert_skipped(&outcome, "圧縮材の強軸・弱軸の座屈長さ");
    assert_skipped(&outcome, COMBINED_REASON);
}
