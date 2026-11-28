use super::shear_area_acceptance_tests::h_column_input as combined_column;
use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckKind, CheckOutcome};

#[path = "../../../sepika-design-jp/tests/fixtures/steel_beam_span_model.rs"]
pub(crate) mod input;

fn assert_short_bending_failure(app: &App) {
    let result = app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|member| member.elem == input::BEAM)
        .unwrap();
    let peak = result
        .positions
        .iter()
        .find(|position| position.xi == 0.25)
        .unwrap();
    let CheckOutcome::Checked(checked) = &peak.outcome else {
        panic!("{:?}", peak.outcome);
    };
    let bending = checked
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Bending)
        .unwrap();
    assert!((bending.ratio - 1.195_894_596).abs() < 1e-8, "{bending:?}");
    assert!(!checked.ok());
}

#[test]
fn steel_beam_span_short_ng_survives_save_and_old_design_recheck_without_reanalysis() {
    let mut app = App::default();
    assert!(app.load_model(input::short_point_combination()));
    let combo_index = app
        .core
        .model
        .combinations
        .iter()
        .position(|combo| combo.name == "G+P+S")
        .unwrap();
    app.run_combination(combo_index);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_short_bending_failure(&app);

    for version in [113, DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-beam-span-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        let analysis = &loaded.core.scoped.results.as_ref().unwrap().member_forces;
        assert_eq!(
            *analysis,
            app.core.scoped.results.as_ref().unwrap().member_forces
        );
        if version != DESIGN_CONTRACT_VERSION {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
            assert!(
                loaded.core.scoped.last_error.is_none(),
                "{:?}",
                loaded.core.scoped.last_error
            );
        }
        assert_short_bending_failure(&loaded);
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
    }
}

#[test]
fn steel_beam_middle_brace_preserves_distinct_checks_at_the_same_position_after_old_design_recheck()
{
    let mut app = App::default();
    assert!(app.load_model(input::simply_supported_with_middle_brace(
        2_000.0, 150_000.0
    )));
    app.run_linear_static(input::DEAD);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let assert_bending = |app: &App| {
        let member = app
            .current_design_results()
            .unwrap()
            .member_checks
            .iter()
            .find(|member| member.elem == input::BEAM)
            .unwrap();
        let ratios = |xi: f64| {
            let mut ratios: Vec<_> = member
                .positions
                .iter()
                .filter(|position| position.xi == xi)
                .filter_map(|p| match &p.outcome {
                    CheckOutcome::Checked(check) => check
                        .components
                        .iter()
                        .find(|c| c.kind == CheckKind::Bending)
                        .map(|c| c.ratio),
                    _ => None,
                })
                .collect();
            ratios.sort_by(f64::total_cmp);
            ratios
        };
        let at_point = ratios(0.25);
        let at_brace = ratios(0.5);
        assert_eq!(at_point.len(), 2, "{member:?}");
        assert_eq!(at_brace.len(), 2, "{member:?}");
        assert!((at_point[0] - 1.743_795_729).abs() < 1e-8, "{at_point:?}");
        assert!((at_brace[0] - 0.903_524_681).abs() < 1e-8, "{at_brace:?}");
        assert!((at_brace[1] - 1.162_530_486).abs() < 1e-8, "{at_brace:?}");
    };
    assert_bending(&app);

    for version in [DESIGN_CONTRACT_VERSION - 1, DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-beam-middle-brace-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            loaded.core.scoped.results.as_ref().unwrap().member_forces,
            app.core.scoped.results.as_ref().unwrap().member_forces
        );
        if version != DESIGN_CONTRACT_VERSION {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
            assert!(
                loaded.core.scoped.last_error.is_none(),
                "{:?}",
                loaded.core.scoped.last_error
            );
        }
        assert_bending(&loaded);
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
    }
}

#[test]
fn steel_beam_span_without_brace_positions_is_not_shown_as_passing() {
    let mut model = input::simply_supported_with_middle_brace(2_000.0, 150_000.0);
    model.steel_design_attrs[0].lateral_brace_count = None;
    model.steel_design_attrs[0].lb_direct = Some((4_000.0, 4_000.0, 4_000.0));
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_linear_static(input::DEAD);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.current_design_results().unwrap();
    let rows = &results
        .member_checks
        .iter()
        .find(|member| member.elem == input::BEAM)
        .unwrap()
        .positions;
    assert!(
        rows.iter().any(
            |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
        if reason.contains("補剛端の位置を確認できません"))
        ),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| row.xi != 0.5 || matches!(row.outcome, CheckOutcome::Skipped { .. })),
        "{rows:?}"
    );
}

#[test]
fn steel_beam_span_missing_saved_end_leaves_bending_unchecked() {
    let mut app = App::default();
    assert!(app.load_model(input::simply_supported_with_middle_brace(2000.0, 150_000.0)));
    app.run_linear_static(input::DEAD);
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let remove_end = |forces: &mut [(ElemId, sepika_element::frame::beam::MemberForces)]| {
        let (_, member) = forces
            .iter_mut()
            .find(|(id, _)| *id == input::BEAM)
            .unwrap();
        member.at.retain(|(xi, _)| *xi != 1.0);
    };
    remove_end(&mut bundle.member_forces);
    remove_end(&mut bundle.statics[0].1.member_forces);
    app.run_design_check();
    let rows = &app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|row| row.elem == input::BEAM)
        .unwrap()
        .positions;
    assert!(
        rows.iter().any(
            |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
        if reason.contains("j端の解析断面力がありません"))
        ),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| matches!(&row.outcome, CheckOutcome::Skipped { .. })),
        "{rows:?}"
    );
}

#[test]
fn saved_beam_does_not_check_the_rounded_right_cut_at_a_nonmaximum_j_face() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::ids::{NodeId, SectionId};
    use sepika_core::model::FrameSectionUse;
    use sepika_core::section_shape::SectionShape;

    let mut model = input::cantilever_with_axial_point_at_j_face();
    let mut section = SectionShape::SteelBox {
        height: 996.6,
        width: 200.0,
        thick: 10.0,
        horizontal_thick: 10.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(1), "j端の直交材".into());
    section.material = model.sections[0].material;
    section.frame_use = Some(FrameSectionUse::Column);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    for dof in [Dof::Ux, Dof::Uz, Dof::Ry] {
        model.nodes[1].restraint.set_free(dof);
    }
    let mut node = model.nodes[1].clone();
    node.id = NodeId(2);
    node.coord[2] = 3_000.0;
    let mut column = model.elements[0].clone();
    column.id = ElemId(1);
    column.nodes = [NodeId(1), node.id].into_iter().collect();
    column.local_axis.ref_vector = [1.0, 0.0, 0.0];
    column.section = Some(section.id);
    column.rigid_zone = Default::default();
    model.nodes.push(node);
    model.sections.push(section);
    model.elements.push(column);

    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_linear_static(input::DEAD);
    assert_eq!(app.core.model.elements[0].rigid_zone.face_j, Some(498.3));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let bundle = app.current_design_results().expect("静的検定の結果");
    let rows = &bundle
        .member_checks
        .iter()
        .find(|row| row.elem == input::BEAM)
        .unwrap()
        .positions;
    let face_xi = 1.0 - 498.3 / 8_395.6;
    let saved_face = bundle.member_forces[0]
        .1
        .at
        .iter()
        .find(|(xi, _)| *xi == face_xi)
        .unwrap();
    assert!(saved_face.1[0].abs() < 1e-6, "{saved_face:?}");
    assert!(
        matches!(&rows.iter().find(|row| row.xi == face_xi).unwrap().outcome,
        CheckOutcome::Skipped { reason } if reason.contains("切断側を確定できません"))
    );
    for xi in [0.0, 0.5] {
        assert!(
            rows.iter().any(|row| row.xi == xi
                && matches!(&row.outcome, CheckOutcome::Checked(check)
                if check.components.iter().any(|c| c.kind == CheckKind::Bending && c.ratio > 0.0)))
        );
    }
}

fn beam_with_independent_column() -> sepika_core::model::Model {
    use sepika_core::ids::{NodeId, SectionId};

    let mut model = input::nodal_only_cases();
    let mut column = combined_column::cantilever(0.0, 120e6, 0.0);
    for node in &mut column.nodes {
        node.id = NodeId(node.id.0 + 2);
        node.coord[1] = 5_000.0;
    }
    let mut section = column.sections.pop().unwrap();
    section.id = SectionId(1);
    let elem = &mut column.elements[0];
    elem.id = ElemId(1);
    elem.section = Some(section.id);
    elem.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    let load = &mut column.load_cases[0].nodal[0];
    load.node = NodeId(3);
    model.nodes.extend(column.nodes);
    model.sections.push(section);
    model.elements.extend(column.elements);
    model.load_cases[0]
        .nodal
        .append(&mut column.load_cases[0].nodal);
    model
}

#[test]
fn duplicated_beam_source_preserves_the_steel_column_bending_distribution() {
    let mut app = App::default();
    assert!(app.load_model(beam_with_independent_column()));
    let model = &mut app.core.model;
    model
        .steel_design_attrs
        .push(sepika_core::model::SteelDesignAttr {
            elem: ElemId(1),
            column_lateral_buckling_end_supports: model.column_end_supports(&model.elements[1]),
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
        });
    app.run_linear_static(input::DEAD);
    let column_rows = |app: &App| {
        app.current_design_results()
            .unwrap()
            .member_checks
            .iter()
            .find(|row| row.elem == ElemId(1))
            .unwrap()
            .positions
            .iter()
            .map(|row| (row.xi, row.outcome.clone()))
            .collect::<Vec<_>>()
    };
    let original = column_rows(&app);
    assert!(
        original.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if result.detail.contains("C="))
        ),
        "{original:?}"
    );
    let bundle = app.core.scoped.results.as_mut().unwrap();
    bundle.statics.push(bundle.statics[0].clone());
    app.run_design_check();
    assert_eq!(column_rows(&app), original);
    assert!(app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|row| row.elem == input::BEAM)
        .unwrap()
        .positions
        .iter()
        .any(
            |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
            if reason.contains("保存解析結果が重複しています"))
        ));
}

fn zero_span_beam(mut model: sepika_core::model::Model) -> App {
    model
        .steel_design_attrs
        .push(sepika_core::model::SteelDesignAttr {
            elem: input::BEAM,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: Some((0.0, 0.0, 0.0)),
            lateral_brace_count: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
            column_lateral_buckling_end_supports: None,
        });
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_linear_static(input::DEAD);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

fn beam_outcomes(app: &App) -> Vec<(f64, CheckOutcome)> {
    app.current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|row| row.elem == input::BEAM)
        .unwrap()
        .positions
        .iter()
        .map(|row| (row.xi, row.outcome.clone()))
        .collect()
}

#[test]
fn zero_span_beam_survives_a_missing_display_row_for_an_independent_column() {
    let mut app = zero_span_beam(beam_with_independent_column());
    let expected = beam_outcomes(&app);
    assert!(expected.iter().any(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(check)
        if check.components.iter().any(|component| component.kind == CheckKind::Bending && component.ratio > 0.0))));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    assert!(bundle.member_forces.iter().any(|(id, _)| *id == ElemId(1)));
    bundle.member_forces.retain(|(id, _)| *id != ElemId(1));

    let (restored, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "steel-beam-missing-independent-column",
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(beam_outcomes(&restored), expected);
    app.run_design_check();
    assert_eq!(beam_outcomes(&app), expected);
}

#[test]
fn zero_span_beam_reports_its_missing_saved_force_row() {
    let mut app = zero_span_beam(input::nodal_only_cases());
    assert!(beam_outcomes(&app)
        .iter()
        .any(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(_))));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    bundle.member_forces.retain(|(id, _)| *id != input::BEAM);
    bundle.statics[0]
        .1
        .member_forces
        .retain(|(id, _)| *id != input::BEAM);
    app.run_design_check();
    let rows = beam_outcomes(&app);
    assert!(
        matches!(rows.as_slice(),
        [(0.5, CheckOutcome::Skipped { reason })] if reason.contains("鋼梁の解析断面力がありません")),
        "{rows:?}"
    );
}

#[test]
fn unverified_split_girder_keeps_independent_bending_failure_on_recheck_and_restore() {
    for missing in [true, false] {
        let mut app = App::default();
        assert!(app.load_model(input::split_cantilever(
            input::cantilever(50_000.0, 0.0, None),
            input::SPLIT_MM,
        )));
        app.run_linear_static(input::DEAD);
        let outcomes = |app: &App, id| {
            app.current_design_results()
                .unwrap()
                .member_checks
                .iter()
                .find(|member| member.elem == id)
                .unwrap()
                .positions
                .clone()
        };
        assert!(outcomes(&app, input::SECOND_BEAM)
            .iter()
            .any(|row| matches!(&row.outcome, CheckOutcome::Checked(check) if check.ok())));

        let bundle = app.core.scoped.results.as_mut().unwrap();
        let (interval_reason, own_reason) = if missing {
            bundle
                .member_forces
                .retain(|(id, _)| *id != input::SECOND_BEAM);
            (
                "一本部材の一意な解析断面力がありません",
                "表示側に鋼梁の解析断面力がありません",
            )
        } else {
            let rear = bundle
                .member_forces
                .iter_mut()
                .find(|(id, _)| *id == input::SECOND_BEAM)
                .unwrap();
            rear.1.at.iter_mut().find(|(xi, _)| *xi == 0.5).unwrap().1[2] += 1.0;
            (
                "一本部材の参照先の解析断面力を照合できません",
                "保存解析結果と一致しません",
            )
        };
        let (restored, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("steel-beam-split-missing-{missing}"),
        );
        std::fs::remove_file(path).unwrap();
        app.run_design_check();
        for (path, app) in [("再検定", &app), ("復元", &restored)] {
            let front = outcomes(app, input::BEAM);
            assert!(
                front.iter().any(|row| row.xi == 0.0
                    && matches!(&row.outcome, CheckOutcome::Checked(check) if !check.ok()
                    && check.components.iter().any(|c| c.kind == CheckKind::Bending
                        && (c.ratio - 2.223_562_930).abs() < 1e-8
                        && c.detail.contains("fb上限=156.6667")))),
                "{path}: {front:?}"
            );
            assert!(front.iter().any(|row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
                if reason.contains("単一補剛区間の強軸曲げ・横座屈は未算定") && reason.contains(interval_reason))),
                "{path}: {front:?}");
            assert!(front
                .iter()
                .all(|row| !matches!(&row.outcome, CheckOutcome::Checked(check) if check.ok())));
            let rear = outcomes(app, input::SECOND_BEAM);
            assert!(rear
                .iter()
                .all(|row| matches!(&row.outcome, CheckOutcome::Skipped { .. })));
            assert!(
                rear.iter().any(
                    |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
                if reason.contains(own_reason))
                ),
                "{path}: {rear:?}"
            );
        }
    }
}

#[test]
fn steel_beam_check_does_not_use_the_unloaded_case_for_a_loaded_selection() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::ids::{MaterialId, SectionId};

    let mut model = super::result_contract_tests::direct_secondary_beam_app()
        .core
        .model;
    let mut nodal = input::nodal_only_cases();
    let mut material = nodal.materials.pop().unwrap();
    material.id = MaterialId(model.materials.len() as u32);
    let mut section = nodal.sections.pop().unwrap();
    section.id = SectionId(model.sections.len() as u32);
    section.material = Some(material.id);
    model.elements[0].section = Some(section.id);
    model.materials.push(material);
    model.sections.push(section);
    model.load_cases = nodal.load_cases;
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FIXED;
    }
    model.nodes[1].restraint.set_free(Dof::Uz);
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_linear_static(input::UNLOADED);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let unloaded = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_forces
        .clone();
    assert!(!unloaded.is_empty());
    assert!(unloaded
        .iter()
        .flat_map(|(_, mf)| &mf.at)
        .all(|(_, section)| *section == [0.0; 6]));
    app.run_linear_static(input::DEAD);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let beam_rows = |app: &App| {
        app.current_design_results()
            .unwrap()
            .member_checks
            .iter()
            .find(|row| row.elem == input::BEAM)
            .unwrap()
            .positions
            .clone()
    };
    let loaded = beam_rows(&app);
    assert!(loaded.iter().any(|row| matches!(&row.outcome, CheckOutcome::Checked(check)
        if check.components.iter().any(|component| component.kind == CheckKind::Bending && component.ratio > 0.0))));

    let original = app.current_design_results().unwrap();
    let (small_beams, slabs) = (original.beam_checks.clone(), original.slab_checks.clone());
    assert!(small_beams.iter().any(|(_, _, beam)| beam.m_max > 0.0));
    assert!(slabs.iter().any(|(_, slab)| slab.moment > 0.0));
    app.core.scoped.results.as_mut().unwrap().member_forces = unloaded;
    let (mut restored, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "steel-beam-wrong-display-case",
    );
    std::fs::remove_file(path).unwrap();
    let current = restored.current_design_results().unwrap();
    assert_eq!(current.beam_checks, small_beams);
    assert_eq!(current.slab_checks, slabs);
    assert!(beam_rows(&restored).iter().all(|row| matches!(&row.outcome,
        CheckOutcome::Skipped { reason } if reason.contains("表示断面力が選択した荷重状態の保存解析結果と一致しません")
    )));
    assert_eq!(
        restored.core.scoped.last_static,
        app.core.scoped.last_static
    );
    restored.run_design_check();
    assert!(beam_rows(&restored).iter().any(|row| matches!(&row.outcome,
        CheckOutcome::Skipped { reason } if reason.contains("表示断面力が選択した荷重状態の保存解析結果と一致しません")
    )));

    app.run_design_check();
    let rows = beam_rows(&app);
    assert!(
        rows.iter().all(
            |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
        if reason.contains("表示断面力が選択した荷重状態の保存解析結果と一致しません"))
        ),
        "{rows:?}"
    );
    let case = StaticCaseKey::User(input::DEAD);
    assert_eq!(app.core.scoped.last_static, Some(StaticKey::Case(case)));
    app.select_displayed_result(StaticKey::Case(case));
    assert!(beam_rows(&app)
        .iter()
        .any(|row| matches!(row.outcome, CheckOutcome::Checked(_))));

    let bundle = app.core.scoped.results.as_mut().unwrap();
    let duplicate = bundle
        .statics
        .iter()
        .find(|row| row.0 == case)
        .unwrap()
        .clone();
    bundle.statics.push(duplicate);
    app.run_design_check();
    let rows = beam_rows(&app);
    assert!(
        rows.iter().all(
            |row| matches!(&row.outcome, CheckOutcome::Skipped { reason }
        if reason.contains("保存解析結果が重複しています"))
        ),
        "{rows:?}"
    );
}
