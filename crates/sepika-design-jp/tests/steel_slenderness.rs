use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
    SteelDesign,
};

fn section() -> Section {
    Section {
        area: 10000.0,
        iy: 400000000.0,
        iz: 100000000.0,
        depth: 400.0,
        width: 200.0,
        material: Some(MaterialId(0)),
        frame_use: Some(FrameSectionUse::Girder),
        ..Section::zero(SectionId(0), "直接性能".into())
    }
}
fn material() -> Material {
    Material {
        id: MaterialId(0),
        name: "直接鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    }
}
fn forces(n: f64) -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.5,
        n,
        qy: 0.0,
        qz: 0.0,
        my: 0.0,
        mz: 0.0,
    }
}
fn result(outcome: CheckOutcome) -> sepika_design_jp::CheckResult {
    match outcome {
        CheckOutcome::Checked(r) => r,
        other => panic!("{other:?}"),
    }
}

#[test]
fn issue901_statutory_boundaries_both_axes_and_terms_ignore_tension_and_zero_axial_force() {
    for (kind, limit) in [
        (MemberKind::Column, 200.0),
        (MemberKind::Girder, 250.0),
        (MemberKind::Brace, 250.0),
    ] {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for axis in [0, 1] {
                for delta in [0.0, 0.0001] {
                    let lambda = limit + delta;
                    let ctx = DesignCtx {
                        kind,
                        term,
                        length: 3000.0,
                        lk_y: Some(if axis == 0 { 200.0 * lambda } else { 1000.0 }),
                        lk_z: Some(if axis == 1 { 100.0 * lambda } else { 1000.0 }),
                        ..Default::default()
                    };
                    for n in [-100000.0, 100000.0, 0.0] {
                        let mut f = forces(n);
                        if n == 0.0 {
                            f.my = 1000.0;
                            f.mz = 1000.0;
                        }
                        let r = result(SteelDesign.check(&f, &section(), &material(), &ctx));
                        let expected_failure = delta > 0.0 && n < 0.0;
                        assert_eq!(
                            !r.ok(),
                            expected_failure,
                            "{kind:?} {term:?} axis={axis} lambda={lambda} n={n}: {r:?}"
                        );
                        let provision =
                            r.components.iter().find(|c| c.kind == CheckKind::Provision);
                        assert_eq!(provision.is_some(), expected_failure);
                        if let Some(p) = provision {
                            assert!(
                                (p.ratio
                                    - if kind == MemberKind::Column {
                                        1.0000005
                                    } else {
                                        1.0000004
                                    })
                                .abs()
                                    < 1e-12
                            );
                            assert!(
                                p.detail.contains("法定上限")
                                    && p.detail.contains("応力度比ではありません")
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn issue901_invalid_compression_inputs_cannot_become_passing_zero_slenderness() {
    for invalid in [f64::NAN, f64::INFINITY, -1.0] {
        for field in 0..5 {
            let mut sec = section();
            let mut ctx = DesignCtx {
                length: 3000.0,
                ..Default::default()
            };
            match field {
                0 => sec.area = invalid,
                1 => sec.iy = invalid,
                2 => sec.iz = invalid,
                3 => ctx.lk_y = Some(invalid),
                _ => ctx.lk_z = Some(invalid),
            }
            assert!(
                matches!(SteelDesign.check(&forces(-1.0), &sec, &material(), &ctx), CheckOutcome::Skipped { reason } if reason.contains("有効細長比"))
            );
        }
    }
}

fn model() -> Model {
    Model {
        sections: vec![section()],
        materials: vec![material()],
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: [f64::from(i) * 3000.0, 0.0, 0.0],
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        steel_design_attrs: vec![SteelDesignAttr {
            elem: ElemId(0),
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: Some(1000.0),
            lk_z_direct: Some(25000.01),
            c_direct: None,
        }],
        ..Default::default()
    }
}

#[test]
fn issue901_public_batch_retains_compression_failure_among_tension_and_zero_positions() {
    use sepika_element::frame::beam::MemberForces;
    for term in [LoadTerm::Long, LoadTerm::Short] {
        let model = model();
        let mf = MemberForces {
            at: vec![
                (0.0, [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                (0.5, [-1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                (1.0, [0.0; 6]),
            ],
        };
        let report = sepika_design_jp::run_member_design_checks(
            &model,
            &[(ElemId(0), mf)],
            &[],
            &sepika_design_jp::MemberDesignCheckOptions {
                term,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.member_checks.len(), 3);
        for (_, pos, outcome) in report.member_checks {
            let r = result(outcome);
            assert_eq!(r.ok(), pos != 0.5);
        }
    }
}

#[test]
fn issue901_similar_sections_have_no_artificial_slenderness_cutoff() {
    for scale in [1e-12_f64, 1e-6, 1.0, 1e6] {
        let mut sec = section();
        sec.area *= scale.powi(2);
        sec.iy *= scale.powi(4);
        sec.iz *= scale.powi(4);
        let ctx = DesignCtx {
            kind: MemberKind::Column,
            length: 3000.0 * scale,
            lk_y: Some(40000.02 * scale),
            lk_z: Some(1000.0 * scale),
            ..Default::default()
        };
        let r = result(SteelDesign.check(&forces(-1.0), &sec, &material(), &ctx));
        assert!(!r.ok(), "scale={scale}");
        assert!((r.ratio() - 1.0000005).abs() < 1e-12);
    }
    for (area, iy) in [(1e-300, 1e300), (1e300, 1e-300)] {
        let mut sec = section();
        sec.area = area;
        sec.iy = iy;
        assert!(matches!(
            SteelDesign.check(
                &forces(-1.0),
                &sec,
                &material(),
                &DesignCtx {
                    length: 3000.0,
                    ..Default::default()
                }
            ),
            CheckOutcome::Skipped { .. }
        ));
    }
}
#[test]
fn issue901_composite_checkers_do_not_apply_ordinary_steel_limit() {
    use sepika_core::section_shape::SectionShape;
    for shape in [
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
        SectionShape::SrcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: sepika_core::section_shape::RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![4],
                y: vec![4],
                cover: 40.0,
                hoop: sepika_core::section_shape::RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
            steel_height: 400.0,
            steel_width: 200.0,
            steel_web_thick: 12.0,
            steel_flange_thick: 12.0,
        },
    ] {
        let sec = shape.to_section(SectionId(0), "合成柱".into());
        let concrete = Material {
            category: MaterialCategory::Concrete,
            fc: Some(30.0),
            young: 25000.0,
            ..material()
        };
        let kind =
            sepika_core::structure_kind::structure_kind_of(Some(&sec), Some(concrete.category));
        assert!(matches!(
            kind,
            sepika_core::structure_kind::StructureKind::Src
                | sepika_core::structure_kind::StructureKind::Cft
        ));
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let outcome = sepika_design_jp::checker_for(kind).check(
                &forces(-1.0),
                &sec,
                &concrete,
                &DesignCtx {
                    kind: MemberKind::Column,
                    term,
                    length: 3000.0,
                    lk_y: Some(1e6),
                    lk_z: Some(1e6),
                    steel_material: Some(material()),
                    ..Default::default()
                },
            );
            match outcome {
                CheckOutcome::Checked(r) => {
                    assert!(r.components.iter().all(|c| c.kind != CheckKind::Provision))
                }
                CheckOutcome::Skipped { reason } => assert!(!reason.contains("有効細長比")),
            }
        }
    }
}

#[test]
fn issue901_public_batch_uses_face_positions_without_node_center_axial_substitution() {
    use sepika_element::frame::beam::MemberForces;
    let mut model = model();
    model.elements[0].rigid_zone.face_i = Some(300.0);
    model.elements[0].rigid_zone.face_j = Some(300.0);
    let mf = MemberForces {
        at: vec![
            (0.0, [-1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            (0.1, [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            (0.5, [-1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            (0.9, [0.0; 6]),
            (1.0, [-1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ],
    };
    let report = sepika_design_jp::run_member_design_checks(
        &model,
        &[(ElemId(0), mf)],
        &[],
        &Default::default(),
    )
    .unwrap();
    assert_eq!(report.member_checks.len(), 3);
    for (_, pos, outcome) in report.member_checks {
        assert!([0.1, 0.5, 0.9].contains(&pos));
        assert_eq!(result(outcome).ok(), pos != 0.5);
    }
}
