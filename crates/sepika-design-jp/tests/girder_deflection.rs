use approx::assert_abs_diff_eq;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    FrameSectionUse, MaterialCategory, MemberLoad, Model, NodalLoad, SteelDesignAttr,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
    MemberDesignCheckReport,
};
use sepika_solver::statics::linear::linear_static_once;

#[path = "fixtures/signed_beam_gravity_model.rs"]
mod fixture;

const TERMS: [(LoadCaseId, f64); 1] = [(LoadCaseId(0), 1.0)];

fn girder(steel: bool, fixed: bool, m_i_knm: f64, m_j_knm: f64) -> Model {
    let mut model = fixture::model();
    model.load_cases.truncate(1);
    model.combinations.clear();
    model.load_cases[0].member = vec![MemberLoad::full_length_uniform(
        ElemId(0),
        [0.0, 0.0, -1.0],
        6000.0,
        15.0,
    )];
    model.load_cases[0].nodal = vec![
        NodalLoad::manual(NodeId(0), [0.0, 0.0, 0.0, 0.0, m_i_knm * 1e6, 0.0]),
        NodalLoad::manual(NodeId(1), [0.0, 0.0, 0.0, 0.0, -m_j_knm * 1e6, 0.0]),
    ];
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FIXED;
        if !fixed {
            node.restraint.set_free(Dof::Ry);
        }
    }
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);

    if steel {
        let material = &mut model.materials[0];
        material.name = "SN400B".into();
        material.category = MaterialCategory::Steel;
        material.young = 205_000.0;
        material.poisson = 0.3;
        material.fc = None;
        material.fy = Some(235.0);
        let mut section = SectionShape::SteelH {
            height: 500.0,
            width: 300.0,
            web_thick: 12.0,
            flange_thick: 20.0,
            root_r: Some(0.0),
        }
        .to_section(SectionId(0), "鋼梁".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        section.material = Some(MaterialId(0));
        model.sections[0] = section;
        model.steel_design_attrs.push(SteelDesignAttr {
            elem: ElemId(0),
            lateral_brace_count: None,
            column_lateral_buckling_end_supports: None,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: Some((0.0, 0.0, 0.0)),
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
        });
    } else if let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape {
        rebar.top = vec![4];
        rebar.bottom = vec![4];
    }
    model
}

fn analyze(
    model: &Model,
    term: LoadTerm,
    terms: Option<&[(LoadCaseId, f64)]>,
) -> MemberDesignCheckReport {
    let solved = linear_static_once(model, LoadCaseId(0)).unwrap();
    run_member_design_checks(
        model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions {
            term,
            load_terms: terms,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn supported_rc_girder_uses_the_absolute_maximum_of_the_signed_curve() {
    // 独立曲率積分による値。端部−60 kN·mでは、同じ絶対値の極大が左右対称に現れる。
    for (fixed, m_i, m_j, delta, position, sign) in [
        (false, 0.0, 0.0, 2.038043478261, 3000.0, '+'),
        (true, 0.0, 0.0, 0.407608695652, 3000.0, '+'),
        (false, 32.0, 32.0, 3.197463768116, 3000.0, '+'),
        (false, -45.0, 0.0, 1.234014260984, 3245.791, '+'),
        (false, -60.0, -60.0, 0.181159420290, 1267.949, '-'),
    ] {
        let report = analyze(
            &girder(false, fixed, m_i, m_j),
            LoadTerm::Long,
            Some(&TERMS),
        );
        assert_eq!(report.member_checks.len(), 3);
        let (_, actual_pos, outcome) = &report.member_checks[1];
        assert_eq!(*actual_pos, 0.5);
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        let deflection = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Deflection)
            .unwrap();
        assert_abs_diff_eq!(deflection.ratio, delta / 3.0, epsilon = 1e-10);
        assert!(
            deflection
                .detail
                .contains(&format!("δ={delta:.6} mm, v={sign}{delta:.6}")),
            "{}",
            deflection.detail
        );
        assert!(
            deflection.detail.contains(&format!("x={position:.3} mm"))
                || (m_i == -60.0 && deflection.detail.contains("x=4732.051 mm")),
            "{}",
            deflection.detail
        );
        if m_i == 32.0 {
            assert!(!result.ok());
            assert!(result
                .components
                .iter()
                .filter(|c| c.kind != CheckKind::Deflection)
                .all(|c| c.ratio <= 1.0));
        }
    }
}

#[test]
fn steel_girder_reports_signed_reference_without_a_deflection_ratio() {
    let model = girder(true, false, 32.0, 32.0);
    let report = analyze(&model, LoadTerm::Long, Some(&TERMS));
    let control = analyze(&model, LoadTerm::Long, None);
    assert_eq!(report.steel_deflection_references.len(), 1);
    let (id, detail) = &report.steel_deflection_references[0];
    assert_eq!(*id, ElemId(0));
    assert!(detail.contains("δ=2.455453 mm, v=+2.455453 mm"), "{detail}");
    assert!(detail.contains("x=3000.000 mm"), "{detail}");
    assert!(control.steel_deflection_references[0]
        .1
        .contains("荷重ケース"));
    for ((_, pos, actual), (_, _, baseline)) in
        report.member_checks.iter().zip(control.member_checks)
    {
        let (CheckOutcome::Checked(actual), CheckOutcome::Checked(baseline)) = (actual, baseline)
        else {
            panic!("鋼の強度検定が未検定になりました");
        };
        assert_eq!(actual.components, baseline.components);
        assert!(actual
            .components
            .iter()
            .all(|component| component.kind != CheckKind::Deflection));
        assert_eq!(actual.detail.contains(detail), *pos == 0.5);
    }
}

#[test]
fn steel_girder_without_a_center_force_explains_unavailable_deflection_on_a_retained_station() {
    let model = girder(true, false, 32.0, 32.0);
    let mut solved = linear_static_once(&model, LoadCaseId(0)).unwrap();
    solved.member_forces[0].1.at.retain(|(pos, _)| *pos != 0.5);
    let report = run_member_design_checks(
        &model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions {
            load_terms: Some(&TERMS),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.member_checks.len(), 2);
    let detail = &report.steel_deflection_references[0].1;
    assert!(
        detail.contains("中央の解析曲げが不足しています"),
        "{detail}"
    );
    let mut displayed_references = 0;
    for (_, _, outcome) in &report.member_checks {
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        assert!(result
            .components
            .iter()
            .all(|c| c.kind != CheckKind::Deflection));
        displayed_references += usize::from(result.detail.contains(detail));
    }
    assert_eq!(displayed_references, 1);
}

#[test]
fn unsupported_conditions_keep_the_strength_results_and_explain_missing_deflection() {
    for steel in [false, true] {
        let mut point = girder(steel, false, 0.0, 0.0);
        point.load_cases[0].member = vec![fixture::point(3000.0, 45_000.0)];
        let mut cantilever = girder(steel, false, 0.0, 0.0);
        cantilever.nodes[0].restraint = Dof6Mask::FIXED;
        cantilever.nodes[1].restraint = Dof6Mask::FREE;
        let mut face = girder(steel, false, 0.0, 0.0);
        face.elements[0].rigid_zone.face_i = Some(500.0);
        face.elements[0].rigid_zone.face_j = Some(1000.0);
        let mut group = girder(steel, false, 0.0, 0.0);
        group.girder_groups = vec![vec![ElemId(0)]];
        for (model, terms, reason) in [
            (point, Some(&TERMS[..]), "集中荷重"),
            (cantilever, Some(&TERMS[..]), "鉛直拘束"),
            (face, Some(&TERMS[..]), "柱フェース"),
            (group, Some(&TERMS[..]), "一本部材"),
            (girder(steel, false, 0.0, 0.0), None, "荷重ケース"),
        ] {
            let report = analyze(&model, LoadTerm::Long, terms);
            let strength = report
                .member_checks
                .iter()
                .filter_map(|(_, _, outcome)| match outcome {
                    CheckOutcome::Checked(result) => Some(result),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(strength.len(), 3);
            assert!(strength.iter().all(|result| result
                .components
                .iter()
                .all(|c| c.kind != CheckKind::Deflection)));
            if steel {
                assert_eq!(report.member_checks.len(), 3);
                assert!(report.steel_deflection_references[0].1.contains(reason));
                assert!(strength[1].detail.contains(reason));
            } else {
                assert!(report.member_checks.iter().any(|(_, pos, outcome)| {
                    *pos == 0.5 && matches!(outcome, CheckOutcome::Skipped { reason: why } if why.contains(reason))
                }), "{:?}", report.member_checks);
            }
        }
    }
}

#[test]
fn short_term_has_no_deflection_component_or_unavailable_reference() {
    for steel in [false, true] {
        let report = analyze(&girder(steel, false, 0.0, 0.0), LoadTerm::Short, None);
        assert!(report.steel_deflection_references.is_empty());
        assert!(report.member_checks.iter().all(|(_, _, outcome)| {
            matches!(outcome, CheckOutcome::Checked(result)
                if !result.detail.contains("たわみ") && result.components.iter().all(|c| c.kind != CheckKind::Deflection))
        }));
    }
}

#[test]
fn rc_girder_with_wrong_shear_rebar_keeps_a_deflection_failure_without_showing_a_pass() {
    for (applied_moment_knm, expected_ng) in [(0.0, false), (32.0, true)] {
        let mut model = girder(false, false, applied_moment_knm, applied_moment_knm);
        let mut shear_material = model.materials[1].clone();
        shear_material.id = MaterialId(2);
        shear_material.category = MaterialCategory::Steel;
        model.sections[0].shear_rebar_material = Some(shear_material.id);
        model.materials.push(shear_material);

        let report = analyze(&model, LoadTerm::Long, Some(&TERMS));
        let outcomes: Vec<_> = report
            .member_checks
            .iter()
            .filter(|(_, pos, _)| *pos == 0.5)
            .map(|(_, _, outcome)| outcome)
            .collect();
        assert!(outcomes.iter().any(|outcome| matches!(
            outcome,
            CheckOutcome::Skipped { reason } if reason.contains("せん断補強筋材料")
        )));
        let components: Vec<_> = outcomes
            .into_iter()
            .filter_map(|outcome| match outcome {
                CheckOutcome::Checked(result) => Some(result.components.as_slice()),
                CheckOutcome::Skipped { .. } => None,
            })
            .flatten()
            .collect();
        assert_eq!(components.len(), usize::from(expected_ng), "{components:?}");
        assert!(components
            .iter()
            .all(|component| { component.kind == CheckKind::Deflection && component.ratio > 1.0 }));
    }
}
