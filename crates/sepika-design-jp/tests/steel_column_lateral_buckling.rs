use sepika_core::model::Model;
use sepika_design_jp::member_design_check::{run_member_design_checks, MemberDesignCheckOptions};
use sepika_design_jp::{CheckKind, CheckOutcome, LoadTerm};

#[path = "fixtures/steel_column_lateral_buckling_model.rs"]
mod input;

fn checks(model: &Model, mz_nmm: f64) -> Result<Vec<CheckOutcome>, String> {
    run_member_design_checks(
        model,
        &[(model.elements[0].id, input::moments(mz_nmm))],
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            ..Default::default()
        },
    )
    .map(|report| {
        report
            .member_checks
            .into_iter()
            .map(|(_, _, outcome)| outcome)
            .collect()
    })
}

#[test]
fn confirmed_end_supports_use_the_eccentric_physical_length() {
    for (offset_mm, lb_mm, expected, ok) in [
        (0.0, 3000, 0.973_509_006_066, true),
        (500.0, 3500, 1.039_363_326_692, false),
    ] {
        let mut model = input::column(offset_mm, true);
        model.steel_design_attrs[0].lb_direct = Some((450.0, 350.0, 250.0));
        let outcomes = checks(&model, 215e6).unwrap();
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            assert!(result.detail.contains(&format!("横座屈長さ lb={lb_mm} mm")));
            assert!(result.detail.contains("中間横補剛なし"));
            assert!(result.detail.contains("梁用のlb直接入力は不使用"));
            let axial_bending = result
                .components
                .iter()
                .find(|component| component.kind == CheckKind::AxialBending)
                .unwrap();
            assert!((axial_bending.ratio - expected).abs() < 1e-11, "{result:?}");
            assert_eq!(result.ok(), ok, "{result:?}");
        }
    }
}

#[test]
fn unconfirmed_or_intermediate_supports_do_not_provide_a_column_buckling_length() {
    for (confirmed, count, direct, reason) in [
        (false, None, None, "両実材端での横移動・ねじれの拘束"),
        (false, Some(0), None, "中間横補剛なしが未確認"),
        (
            false,
            None,
            Some((1000.0, 1500.0, 1000.0)),
            "実材端以外の支点",
        ),
        (true, Some(1), None, "中間横補剛が入力されています"),
    ] {
        let mut model = input::column(500.0, confirmed);
        model.steel_design_attrs[0].lateral_brace_count = count;
        model.steel_design_attrs[0].lb_direct = direct;
        let outcomes = checks(&model, 215e6).unwrap();
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason: actual } if actual.contains(reason)),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn failure_lower_bound_and_missing_support_are_both_reported() {
    let model = input::column(500.0, false);
    let outcomes = checks(&model, 300e6).unwrap();
    assert_eq!(outcomes.len(), 6);
    let (pairs, _) = outcomes.as_chunks::<2>();
    for pair in pairs {
        let [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }] = pair else {
            panic!("{pair:?}");
        };
        assert!(!result.ok());
        let check = &result.components[0];
        assert_eq!(check.kind, CheckKind::AxialBending);
        assert!(check.detail.contains("軸曲げ比の下限"));
        assert!(reason.contains("中間横補剛なしが未確認"));
    }
    let outcomes = checks(&model, 0.0).unwrap();
    assert_eq!(outcomes.len(), 3);
    assert!(outcomes
        .iter()
        .all(|outcome| matches!(outcome, CheckOutcome::Checked(result) if result.ok())));
}

#[test]
fn changed_end_conditions_cannot_reuse_the_prior_confirmation() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::model::{EndCondition, GirderTorsionMode};

    let original = input::column(500.0, true);
    let mut model = original.clone();
    for edit in [
        (|model: &mut Model| model.elements[0].end_offsets_mm[1][2] = 0.0) as fn(&mut Model),
        |model| model.nodes[1].coord[2] -= 250.0,
        |model| model.nodes[1].restraint = Dof6Mask::FREE,
        |model| model.nodes[1].support_spring = Some([1.0; 6]),
        |model| model.elements[0].end_cond[1] = EndCondition::Pinned,
        |model| {
            model.girder_torsion = match model.girder_torsion {
                GirderTorsionMode::ReleaseIEnd => GirderTorsionMode::Keep,
                GirderTorsionMode::Keep => GirderTorsionMode::ReleaseIEnd,
            }
        },
    ] {
        edit(&mut model);
        let outcomes = checks(&model, 215e6).unwrap();
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains("実材端の条件が確認時から変わりました")),
                "{outcome:?}"
            );
        }
        model = original.clone();
    }
}

#[test]
fn removing_the_restraint_of_a_direct_rigid_link_partner_requires_reconfirmation() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::NodeId;
    use sepika_core::model::Constraint;

    let mut model = input::column(500.0, false);
    let mut support = model.nodes[1].clone();
    support.id = NodeId(2);
    support.coord[1] += 1000.0;
    support.restraint = Dof6Mask::FIXED;
    model.nodes.push(support);
    model.nodes[1].restraint = Dof6Mask::FREE;
    model.constraints.push(Constraint::RigidLink {
        master: NodeId(2),
        slaves: vec![NodeId(1)],
        dofs: Dof6Mask::FIXED,
    });
    model.steel_design_attrs[0].column_lateral_buckling_end_supports =
        Some(model.column_end_supports(&model.elements[0]).unwrap());
    let outcomes = checks(&model, 215e6).unwrap();
    assert_eq!(outcomes.len(), 3);
    assert!(outcomes
        .iter()
        .all(|outcome| matches!(outcome, CheckOutcome::Checked(result) if !result.ok())));

    model.nodes[2].restraint = Dof6Mask::FREE;
    let outcomes = checks(&model, 215e6).unwrap();
    assert_eq!(outcomes.len(), 3);
    for outcome in outcomes {
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains("実材端の条件が確認時から変わりました")),
            "{outcome:?}"
        );
    }
}

#[test]
fn changing_the_stiffness_of_a_directly_connected_girder_requires_reconfirmation() {
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
    use sepika_core::model::{FrameSectionUse, PropertyBasis};
    use sepika_design_jp::steel::column_lateral_buckling_length;

    let mut model = input::column(500.0, false);
    let mut other_node = model.nodes[1].clone();
    other_node.id = NodeId(2);
    other_node.coord[1] += 3000.0;
    model.nodes.push(other_node);
    let mut girder = model.elements[0].clone();
    girder.id = ElemId(1);
    girder.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    girder.section = Some(SectionId(1));
    girder.end_offsets_mm = [[0.0; 3]; 2];
    model.elements.push(girder);
    let mut section = model.sections[0].clone();
    section.id = SectionId(1);
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(1));
    section.property_basis.j = PropertyBasis::Supplied;
    model.sections.push(section);
    let mut material = model.materials[0].clone();
    material.id = MaterialId(1);
    model.materials.push(material);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports =
        Some(model.column_end_supports(&model.elements[0]).unwrap());
    let length = |model: &Model| {
        column_lateral_buckling_length(model, &model.elements[0], model.steel_design_attrs.first())
    };
    assert_eq!(length(&model), Ok(3500.0));
    for edit in [
        (|model: &mut Model| model.sections[1].area /= 2.0) as fn(&mut Model),
        |model| model.sections[1].as_y /= 2.0,
        |model| model.sections[1].as_z /= 2.0,
        |model| model.sections[1].iy /= 2.0,
        |model| model.sections[1].iz /= 2.0,
        |model| model.sections[1].j /= 2.0,
        |model| model.materials[1].young /= 2.0,
        |model| model.materials[1].shear = Some(30_000.0),
    ] {
        let mut edited = model.clone();
        edit(&mut edited);
        assert!(length(&edited)
            .unwrap_err()
            .contains("確認時から変わりました"));
    }
}

#[test]
fn a_directly_connected_input_plate_requires_reconfirmation_after_an_edit() {
    use sepika_core::ids::{ElemId, NodeId, SectionId};
    use sepika_core::model::{ElementKind, Section};
    use sepika_design_jp::steel::column_lateral_buckling_length;

    for kind in [ElementKind::Shell, ElementKind::Wall] {
        let mut model = input::column(500.0, false);
        for (id, coord) in [
            (2, [3000.0, 0.0, 3000.0]),
            (3, [3000.0, 0.0, 1500.0]),
            (4, [0.0, 0.0, 1500.0]),
        ] {
            let mut node = model.nodes[1].clone();
            node.id = NodeId(id);
            node.coord = coord;
            model.nodes.push(node);
        }
        let mut plate = model.elements[0].clone();
        plate.id = ElemId(1);
        plate.kind = kind;
        plate.nodes = [1, 2, 3, 4].into_iter().map(NodeId).collect();
        plate.section = Some(SectionId(1));
        plate.end_offsets_mm = [[0.0; 3]; 2];
        model.elements.push(plate);
        let mut plate_section = Section::zero(SectionId(1), "板".into());
        plate_section.thickness = Some(150.0);
        model.sections.push(plate_section);
        model.steel_design_attrs[0].column_lateral_buckling_end_supports =
            Some(model.column_end_supports(&model.elements[0]).unwrap());

        let length = |model: &Model| {
            column_lateral_buckling_length(
                model,
                &model.elements[0],
                model.steel_design_attrs.first(),
            )
        };
        assert_eq!(length(&model), Ok(3500.0));
        model.sections[1].thickness = Some(100.0);
        assert!(length(&model)
            .unwrap_err()
            .contains("確認時から変わりました"));
        model.sections[1].thickness = Some(150.0);
        assert_eq!(length(&model), Ok(3500.0));
        model.elements.pop();
        assert!(length(&model)
            .unwrap_err()
            .contains("確認時から変わりました"));
    }
}

#[test]
fn changing_the_stiffness_of_a_directly_connected_isolator_requires_reconfirmation() {
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::{ElementKind, IsolatorAttr, IsolatorProps};
    use sepika_design_jp::steel::column_lateral_buckling_length;

    let mut model = input::column(500.0, false);
    let mut other_node = model.nodes[1].clone();
    other_node.id = NodeId(2);
    other_node.coord[1] += 1000.0;
    model.nodes.push(other_node);
    let mut isolator = model.elements[0].clone();
    isolator.id = ElemId(1);
    isolator.kind = ElementKind::Isolator;
    isolator.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    isolator.section = None;
    isolator.end_offsets_mm = [[0.0; 3]; 2];
    model.elements.push(isolator);
    model.isolator_attrs.push(IsolatorAttr {
        elem: ElemId(1),
        props: IsolatorProps::default(),
    });
    model.steel_design_attrs[0].column_lateral_buckling_end_supports =
        Some(model.column_end_supports(&model.elements[0]).unwrap());
    let length = |model: &Model| {
        column_lateral_buckling_length(model, &model.elements[0], model.steel_design_attrs.first())
    };
    assert_eq!(length(&model), Ok(3500.0));
    model.isolator_attrs[0].props.k1 *= 0.5;
    assert!(length(&model)
        .unwrap_err()
        .contains("実材端の条件が確認時から変わりました"));
}

#[test]
fn nonfinite_end_offset_is_rejected_even_with_confirmed_supports() {
    let mut model = input::column(500.0, true);
    model.elements[0].end_offsets_mm[1][2] = f64::NAN;
    let reason = checks(&model, 215e6).unwrap_err();
    assert!(reason.contains("j端のXYZ偏心が非有限"), "{reason}");
}
