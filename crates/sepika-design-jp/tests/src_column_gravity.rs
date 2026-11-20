use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
    QdMethod, SeismicQd, SrcDesign,
};

fn material(name: &str, category: MaterialCategory) -> Material {
    Material {
        id: MaterialId(0),
        name: name.into(),
        category,
        young: 205_000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: Some(24.0),
        fy: Some(345.0),
        strength_factor: None,
        concrete_class: Default::default(),
    }
}

fn section() -> Section {
    SectionShape::SrcColumnRect {
        b: 500.0,
        d: 700.0,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 500.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 14.0,
    }
    .to_section(SectionId(0), "SRC柱".into())
}

fn context(method: QdMethod) -> DesignCtx {
    DesignCtx {
        kind: MemberKind::Column,
        term: LoadTerm::Short,
        rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        shear_rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        steel_material: Some(material("SN400B", MaterialCategory::Steel)),
        seismic_qd: Some(SeismicQd {
            long_at: vec![(0.0, [0.0, 100_000.0, 30_000.0, 0.0, 0.0, 0.0])],
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple: None,
            clear_length: 6000.0,
            method,
        }),
        ..Default::default()
    }
}

fn forces() -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.0,
        n: 0.0,
        qy: 110_000.0,
        qz: 50_000.0,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    }
}

#[test]
fn column_qd_rejects_missing_position_and_either_nonfinite_gravity_direction() {
    for method in [QdMethod::Qd1, QdMethod::Qd2, QdMethod::Min] {
        for (label, values) in [
            ("missing", None),
            ("nan_y", Some([f64::NAN, 30_000.0])),
            ("nan_z", Some([100_000.0, f64::NAN])),
            ("nan_both", Some([f64::NAN; 2])),
            ("inf_y", Some([f64::INFINITY, 30_000.0])),
            ("inf_z", Some([100_000.0, f64::NEG_INFINITY])),
        ] {
            let mut ctx = context(method);
            if let Some(values) = values {
                ctx.seismic_qd.as_mut().unwrap().long_at[0].1[1..3].copy_from_slice(&values);
            } else {
                ctx.seismic_qd.as_mut().unwrap().long_at.clear();
            }
            let out = SrcDesign.check(
                &forces(),
                &section(),
                &material("Fc24", MaterialCategory::Concrete),
                &ctx,
            );
            assert!(
                matches!(out, CheckOutcome::Skipped { reason } if reason.contains("QL（Qy・Qz）")),
                "{method:?} {label}"
            );
        }
        for pos in [0.5, 1.1e-6, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut ctx = context(method);
            ctx.seismic_qd.as_mut().unwrap().long_at[0].0 = pos;
            assert!(matches!(
                SrcDesign.check(
                    &forces(),
                    &section(),
                    &material("Fc24", MaterialCategory::Concrete),
                    &ctx
                ),
                CheckOutcome::Skipped { .. }
            ));
        }
        for pos in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut f = forces();
            f.pos = pos;
            assert!(matches!(
                SrcDesign.check(
                    &f,
                    &section(),
                    &material("Fc24", MaterialCategory::Concrete),
                    &context(method)
                ),
                CheckOutcome::Skipped { .. }
            ));
        }
    }
}

#[test]
fn normal_short_uses_elastic_share_and_long_uses_combined_allowable() {
    let mat = material("Fc24", MaterialCategory::Concrete);
    let sec = section();
    let mut ctx = context(QdMethod::Min);
    ctx.seismic_qd = None;
    let normal = checked(SrcDesign.check(&forces(), &sec, &mat, &ctx));
    let shear = normal
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap();
    assert!((shear.ratio - 0.1373893789326067).abs() < 1e-12);
    assert_eq!(shear.detail.matches("弾性分担").count(), 2);
    ctx.term = LoadTerm::Long;
    let long = checked(SrcDesign.check(&forces(), &sec, &mat, &ctx));
    assert!(long.components.iter().all(|c| c.ratio.is_finite()));
    assert!(
        long.components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap()
            .ratio
            > 0.0
    );
}

#[test]
fn public_batch_distinguishes_requested_missing_member_from_normal_short() {
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::*;
    use sepika_element::frame::beam::MemberForces;
    let mut sec = section();
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(1));
    sec.steel_material = Some(MaterialId(2));
    sec.frame_use = Some(FrameSectionUse::Column);
    let mut mats = vec![
        material("Fc24", MaterialCategory::Concrete),
        material("SD345", MaterialCategory::Rebar),
        material("SN400B", MaterialCategory::Steel),
    ];
    for (i, mat) in mats.iter_mut().enumerate() {
        mat.id = MaterialId(i as u32);
    }
    let model = Model {
        sections: vec![sec],
        materials: mats,
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: [0.0, 0.0, f64::from(i) * 6000.0],
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
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        ..Default::default()
    };
    let mf = MemberForces {
        at: vec![(0.0, [0.0, 110_000.0, 50_000.0, 0.0, 0.0, 0.0])],
    };
    let current = [(ElemId(0), mf.clone())];
    let empty = vec![];
    let other = vec![(ElemId(99), mf.clone())];
    let no_position = vec![(ElemId(0), MemberForces { at: vec![] })];
    for (reference, expected_reason) in [
        (None, None),
        (Some(empty.as_slice()), Some("重力参照内力")),
        (Some(other.as_slice()), Some("重力参照内力")),
        (Some(no_position.as_slice()), Some("QL（Qy・Qz）")),
        (Some(current.as_slice()), Some("一次設計用地震力")),
    ] {
        let options = sepika_design_jp::MemberDesignCheckOptions {
            term: LoadTerm::Short,
            long_member_forces: reference,
            ..Default::default()
        };
        let run = |forces: &[(ElemId, MemberForces)]| {
            sepika_design_jp::run_member_design_checks(&model, forces, &[], &options)
                .unwrap()
                .member_checks
        };
        let checks = run(&current);
        assert_eq!(checks.len(), 1);
        let Some(reason) = expected_reason else {
            assert!(matches!(&checks[0].2, CheckOutcome::Checked(result) if result.ok()));
            continue;
        };
        assert!(
            matches!(&checks[0].2, CheckOutcome::Skipped { reason: actual } if actual.contains(reason)),
            "{checks:?}"
        );
        for bad_force_index in [0, 5] {
            let mut overloaded = current.clone();
            overloaded[0].1.at[0].1[bad_force_index] = 100_000_000_000.0;
            let checks = run(&overloaded);
            assert_eq!(checks.len(), 2, "{checks:?}");
            assert!(matches!(&checks[0].2, CheckOutcome::Checked(result)
                if !result.ok() && result.components.len() == 1
                   && result.components[0].kind == CheckKind::AxialBending
            ));
            assert_eq!(checks[1].2, run(&current)[0].2);
        }
    }
}

fn checked(outcome: CheckOutcome) -> sepika_design_jp::CheckResult {
    match outcome {
        CheckOutcome::Checked(result) => result,
        other => panic!("{other:?}"),
    }
}
