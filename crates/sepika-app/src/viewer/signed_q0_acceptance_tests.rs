use super::*;
use crate::app::rc_beam_asymmetric_tests::input as asymmetric;
use crate::app::signed_q0_acceptance_tests::fixture;

#[path = "../../../sepika-design-jp/tests/fixtures/rc_beam_qd1_model.rs"]
mod rc_beam_qd1_model;

fn labels(app: &App, rec: &ThRecording, applied_long_term: bool) -> String {
    member_labels(app, rec, 0, applied_long_term)
}

fn member_labels(app: &App, rec: &ThRecording, index: usize, applied_long_term: bool) -> String {
    peak_labels(app, rec, index, true, applied_long_term)
}

fn peak_labels(
    app: &App,
    rec: &ThRecording,
    index: usize,
    nonlinear: bool,
    applied_long_term: bool,
) -> String {
    peak_labels_with_confirmed_long_term(
        app,
        rec,
        index,
        nonlinear,
        applied_long_term,
        applied_long_term,
    )
}

fn peak_labels_with_confirmed_long_term(
    app: &App,
    rec: &ThRecording,
    index: usize,
    nonlinear: bool,
    applied_long_term: bool,
    confirmed_long_term: bool,
) -> String {
    let elem = &app.core.model.elements[index];
    let axis = super::super::member_axis_endpoints(elem, &app.core.model).unwrap();
    egui::Context::default()
        .run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(3000.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw_peak_check(
                    ui,
                    app,
                    elem,
                    index,
                    rec,
                    rec.frame_time.len(),
                    nonlinear,
                    applied_long_term,
                    || (!confirmed_long_term).then_some(UNVERIFIED_DAMAGE_CONTROL_GRAVITY),
                    || {
                        Ok(app
                            .core
                            .scoped
                            .results
                            .as_ref()
                            .and_then(|bundle| {
                                bundle.combos.iter().find(|(name, _)| {
                                    sepika_core::load_combo::is_gravity_combination(
                                        name,
                                        &app.core.model,
                                    )
                                })
                            })
                            .map(|(_, result)| result.member_forces.as_slice())
                            .unwrap_or_default())
                    },
                    &axis,
                )
            },
        )
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn signed_q0_th_detail_missing_repaired_missing_does_not_render_ordinary_check() {
    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    app.load_model(fixture::model());
    app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Qd1;
    app.run_combination(0);
    let peak = app.core.scoped.results.as_ref().unwrap().member_forces[0]
        .1
        .clone();
    app.run_combination(1);
    let rec = ThRecording {
        peak_member_forces: vec![Some(peak)],
        ..Default::default()
    };
    for valid in [false, true, false] {
        app.core.model.load_cases[0].member[0].dir =
            if valid { [0.0, 0.0, -1.0] } else { [0.0; 3] };
        let text = labels(&app, &rec, true);
        if valid {
            assert!(text.contains("検定比"), "{text}");
            assert!(!text.contains("検定不能"), "{text}");
        } else {
            assert!(text.contains("Q0"), "{text}");
            assert!(!text.contains("検定比"), "{text}");
        }
    }
    app.core.model.load_cases[0].member[0].dir = [0.0, 0.0, -1.0];
    let original = app.core.scoped.results.as_ref().unwrap().combos.clone();
    for valid in [false, true, false] {
        app.core.scoped.results.as_mut().unwrap().combos =
            if valid { original.clone() } else { vec![] };
        let text = labels(&app, &rec, true);
        if valid {
            assert!(text.contains("検定比"), "{text}");
        } else {
            assert!(text.contains("重力参照内力"), "{text}");
            assert!(!text.contains("検定比"), "{text}");
        }
    }
}

#[test]
fn signed_q0_th_detail_local_axis_error_affects_only_beam_that_needs_q0() {
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_design_jp::QdMethod;

    let mut model = fixture::model();
    for mut node in model.nodes.clone() {
        node.id = NodeId(node.id.0 + 2);
        node.coord[1] += 4000.0;
        model.nodes.push(node);
    }
    let mut beam = model.elements[0].clone();
    beam.id = ElemId(1);
    beam.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    model.elements.push(beam);
    for case in &mut model.load_cases[..2] {
        let mut load = case.member[0].clone();
        load.elem = ElemId(1);
        case.member.push(load);
    }

    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    assert!(app.load_model(model));
    app.run_combination(0);
    let peak = &app.current_static().unwrap().member_forces;
    let rec = ThRecording {
        peak_member_forces: [ElemId(0), ElemId(1)]
            .map(|id| Some(peak.iter().find(|(elem, _)| *elem == id).unwrap().1.clone()))
            .to_vec(),
        ..Default::default()
    };
    app.run_combination(1);

    for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
        app.core.analysis_cfg.qd_method = method;
        app.core.model.elements[1].local_axis.ref_vector = [0.0, 0.0, 1.0];
        let expected_healthy = labels(&app, &rec, true);
        let expected_other = member_labels(&app, &rec, 1, true);
        assert!(expected_healthy.contains("検定比"), "{expected_healthy}");
        assert!(expected_other.contains("検定比"), "{expected_other}");

        app.core.model.elements[1].local_axis.ref_vector = [f64::NAN, 0.0, 1.0];
        let reason = sepika_job::simple_beam_q0_by_gravity_cases(&app.core.model).unwrap_err();
        assert!(reason.contains("部材 ID 1"), "{reason}");
        assert!(
            reason.contains("断面参照方向に非有限値があります"),
            "{reason}"
        );
        assert_eq!(labels(&app, &rec, true), expected_healthy, "{method:?}");
        let other = member_labels(&app, &rec, 1, true);
        if method == QdMethod::Qd2 {
            assert_eq!(other, expected_other);
        } else {
            assert!(
                other.contains("検定不能:") && other.contains(&reason),
                "{other}"
            );
            assert!(!other.contains("検定比"), "{other}");
        }
    }
}

#[test]
fn signed_q0_th_detail_steel_still_checks_without_rc_gravity_input() {
    use super::steel_column_lateral_buckling_input::{column, moments};
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
    use sepika_design_jp::QdMethod;

    let mut app = App::default();
    assert!(app.load_model(column(0.0, true)));
    app.core.analysis_cfg.qd_method = QdMethod::Qd1;
    let rec = ThRecording {
        peak_member_forces: vec![Some(moments(300e6))],
        ..Default::default()
    };
    let text = labels(&app, &rec, true);
    assert!(text.contains("検定比") && text.contains("(NG)"), "{text}");
    assert!(
        text.contains("確定時刻ごとの補剛区間の照査記録がありません"),
        "{text}"
    );
    assert!(!text.contains("重力参照内力"), "{text}");

    let mut rc_model = fixture::model();
    let mut concrete = rc_model.materials.remove(0);
    concrete.id = MaterialId(1);
    let mut section = rc_model.sections.remove(0);
    section.id = SectionId(1);
    section.material = Some(concrete.id);
    let mut beam = rc_model.elements.remove(0);
    beam.id = ElemId(1);
    beam.section = Some(section.id);
    beam.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    for mut node in rc_model.nodes {
        node.id = NodeId(node.id.0 + 2);
        node.coord[1] += 4000.0;
        app.core.model.nodes.push(node);
    }
    app.core.model.materials.push(concrete);
    app.core.model.sections.push(section);
    app.core.model.elements.push(beam);
    let mut dead_loads = rc_model.load_cases.remove(0);
    dead_loads.member[0].elem = ElemId(1);
    app.core.model.load_cases.push(dead_loads);
    assert_eq!(labels(&app, &rec, true), text);

    app.core.model.elements[1].local_axis.ref_vector = [f64::NAN, 0.0, 1.0];
    assert!(sepika_job::simple_beam_q0_by_gravity_cases(&app.core.model).is_err());
    assert_eq!(labels(&app, &rec, true), text);
}

#[test]
fn src_column_th_detail_displays_why_seismic_bending_shear_is_unchecked() {
    use sepika_design_jp::QdMethod;

    let mut app = crate::app::src_ql_acceptance_tests::src_column_app(QdMethod::Qd1);
    app.run_combination(0);
    let peak = app.current_static().unwrap().member_forces[0].1.clone();
    app.run_combination(1);
    let rec = ThRecording {
        peak_member_forces: vec![Some(peak)],
        ..Default::default()
    };
    let mut overloaded = rec.clone();
    overloaded.peak_member_forces[0].as_mut().unwrap().at =
        vec![(0.0, [-40.0e6, 0.0, 0.0, 0.0, 0.0, 0.0])];
    for (method, reason) in [
        (QdMethod::Qd1, "柱頭・柱脚の軸力"),
        (QdMethod::Min, "一次設計用地震力"),
    ] {
        app.core.analysis_cfg.qd_method = method;
        let text = labels(&app, &rec, true);
        assert!(text.contains("検定不能: SRC柱検定"), "{text}");
        assert!(text.contains(reason), "{text}");
        assert!(!text.contains("検定比"), "{text}");

        let text = labels(&app, &overloaded, true);
        assert_eq!(text.matches("ξ=0.00:").count(), 2, "{text}");
        assert!(text.contains("(NG)"), "{text}");
        assert!(
            text.contains("検定不能: SRC柱検定") && text.contains(reason),
            "{text}"
        );
        assert!(!text.contains("(OK)"), "{text}");

        let mut with_torsion = overloaded.clone();
        with_torsion.peak_member_forces[0].as_mut().unwrap().at[0].1[3] = 1.0e6;
        let text = labels(&app, &with_torsion, true);
        assert_eq!(text.matches("検定不能:").count(), 1, "{text}");
        assert!(
            text.contains(reason) && text.contains("ねじりモーメントMx"),
            "{text}"
        );
        assert!(text.contains("(NG)") && !text.contains("(OK)"), "{text}");
    }
    let ordinary = labels(&app, &rec, false);
    assert!(ordinary.contains("検定比"), "{ordinary}");
    assert!(ordinary.contains("(OK)"), "{ordinary}");
    assert!(!ordinary.contains("検定不能"), "{ordinary}");
}

#[test]
fn src_column_th_detail_keeps_axial_bending_ng_when_gravity_reference_is_missing() {
    let mut app =
        crate::app::src_ql_acceptance_tests::src_column_app(sepika_design_jp::QdMethod::Qd1);
    app.run_combination(1);
    let forces = MemberForces {
        at: vec![
            (0.0, [-40.0e6, 0.0, 0.0, 1.0e6, 0.0, 0.0]),
            (0.5, [10_000.0, 0.0, 0.0, 0.0, 0.0, 4.0e9]),
            (1.0, [10_000.0, 10.0e6, 0.0, 0.0, 0.0, 0.0]),
        ],
    };
    let rec = ThRecording {
        peak_member_forces: vec![Some(forces.clone())],
        axial_force_peaks: vec![AxialForcePeaks {
            snapshots: [0.25, 0.5, 0.75]
                .into_iter()
                .enumerate()
                .map(|(index, time)| {
                    let mut at_step = forces.clone();
                    for (i, (_, f)) in at_step.at.iter_mut().enumerate() {
                        if i != index {
                            *f = [0.0; 6];
                        }
                    }
                    ForcePeakSnapshot {
                        time,
                        forces: at_step,
                        positions: vec![index],
                    }
                })
                .collect(),
        }],
        ..Default::default()
    };
    let assert_rows = |text: &str, reasons: [&str; 3]| {
        let (envelope, snapshots) = text
            .split_once("軸力極値が生じた確定ステップ\n")
            .unwrap_or_else(|| panic!("{text}"));
        for (rows, positions) in [
            (envelope, ["ξ=0.00:", "ξ=0.50:", "ξ=1.00:"]),
            (
                snapshots,
                [
                    "圧縮側 t=0.250000 s  ξ=0.00:",
                    "引張側 t=0.500000 s  ξ=0.50:",
                    "引張側 t=0.750000 s  ξ=1.00:",
                ],
            ),
        ] {
            assert!(!rows.contains("(OK)"), "{rows}");
            assert!(!rows.contains("せん断 "), "{rows}");
            assert_eq!(rows.matches("検定比 ").count(), 2, "{rows}");
            assert_eq!(rows.matches("軸+曲げ ").count(), 2, "{rows}");
            for (index, position) in positions.into_iter().enumerate() {
                let (_, at_position) = rows
                    .split_once(position)
                    .unwrap_or_else(|| panic!("{rows}"));
                let at_position = positions
                    .get(index + 1)
                    .and_then(|next| at_position.split_once(next).map(|(row, _)| row))
                    .unwrap_or(at_position);
                assert_eq!(
                    at_position.matches("(NG)").count(),
                    usize::from(index < 2),
                    "{rows}"
                );
                assert_eq!(at_position.matches("検定不能:").count(), 1, "{rows}");
                assert!(at_position.contains(reasons[index]), "{rows}");
                assert_eq!(
                    at_position.contains("ねじりモーメントMx"),
                    index == 0,
                    "{rows}"
                );
            }
        }
    };

    let qd1_reason = "柱頭・柱脚の軸力";
    assert_rows(&labels(&app, &rec, true), [qd1_reason; 3]);

    let stored = &mut app.core.scoped.results.as_mut().unwrap().combos;
    assert_eq!(stored.len(), 1);
    let long_at = &mut stored[0].1.member_forces[0].1.at;
    long_at.retain(|(pos, _)| *pos != 1.0);
    long_at.iter_mut().find(|(pos, _)| *pos == 0.5).unwrap().1[2] = f64::NAN;
    assert_rows(
        &labels(&app, &rec, true),
        [
            qd1_reason,
            "同位置の重力参照せん断力QL（Qy・Qz）が不足または非有限",
            "同位置の重力参照せん断力QL（Qy・Qz）が不足または非有限",
        ],
    );

    let stored = &mut app.core.scoped.results.as_mut().unwrap().combos;
    stored[0].1.member_forces.clear();
    let reason = "設計用せん断力に必要な重力参照内力が不足しています";
    assert_rows(&labels(&app, &rec, true), [reason; 3]);
    app.core.scoped.results.as_mut().unwrap().combos.clear();
    assert_rows(&labels(&app, &rec, true), [reason; 3]);

    let ordinary = labels(&app, &rec, false);
    assert!(!ordinary.contains(reason), "{ordinary}");
    let (_, ordinary_tip) = ordinary
        .split_once("引張側 t=0.750000 s  ξ=1.00:")
        .unwrap_or_else(|| panic!("{ordinary}"));
    assert!(
        ordinary_tip
            .contains("時刻歴の地震内力から設計用せん断力と材料別の負担曲げを確定できません"),
        "{ordinary}"
    );
    assert!(!ordinary_tip.contains("(OK)"), "{ordinary}");
}

#[test]
fn src_column_th_detail_checks_without_q0_for_another_beam() {
    use sepika_core::ids::{ElemId, SectionId};

    let mut app =
        crate::app::src_ql_acceptance_tests::src_column_app(sepika_design_jp::QdMethod::Qd1);
    let mut beam_fixture = fixture::model();
    let mut section = beam_fixture.sections.remove(0);
    section.id = SectionId(1);
    let mut beam = beam_fixture.elements.remove(0);
    beam.id = ElemId(1);
    beam.section = Some(section.id);
    beam.nodes[1] = beam.nodes[0];
    app.core.model.sections.push(section);
    app.core.model.elements.push(beam);
    assert!(sepika_job::simple_beam_q0_by_gravity_cases(&app.core.model).is_err());

    let rec = ThRecording {
        peak_member_forces: vec![Some(MemberForces {
            at: vec![(0.0, [-40.0e6, 0.0, 0.0, 0.0, 0.0, 0.0])],
        })],
        ..Default::default()
    };
    let text = labels(&app, &rec, true);
    assert!(text.contains("重力参照内力が不足しています"), "{text}");
    assert!(text.contains("(NG)") && text.contains("軸+曲げ "), "{text}");
    assert!(!text.contains("せん断 "), "{text}");
}

#[test]
fn src_column_th_detail_reports_gravity_reference_even_if_other_inputs_prevent_checks() {
    let mut app =
        crate::app::src_ql_acceptance_tests::src_column_app(sepika_design_jp::QdMethod::Qd1);
    app.run_combination(1);
    let long_at = &mut app.core.scoped.results.as_mut().unwrap().combos[0]
        .1
        .member_forces[0]
        .1
        .at;
    long_at.retain(|(pos, _)| *pos != 0.5);

    let assert_reasons = |app: &App, n, input_reason, gravity_reason| {
        let rec = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![(0.5, [n, 0.0, 0.0, 1.0e6, 0.0, 0.0])],
            })],
            ..Default::default()
        };
        let text = labels(app, &rec, true);
        let (_, row) = text
            .split_once("ξ=0.50:")
            .unwrap_or_else(|| panic!("{text}"));
        for reason in [
            "検定不能:",
            input_reason,
            gravity_reason,
            "ねじりモーメントMx",
        ] {
            assert_eq!(row.matches(reason).count(), 1, "{text}");
        }
        assert_eq!(row.contains("(NG)"), n.is_finite(), "{text}");
    };

    let material = app.core.model.sections[0].shear_rebar_material.take();
    assert!(material.is_some());
    let missing_material = "せん断補強筋の材料が未割当";
    assert_reasons(
        &app,
        -40.0e6,
        missing_material,
        "同位置の重力参照せん断力QL（Qy・Qz）が不足または非有限です",
    );
    app.core.scoped.results.as_mut().unwrap().combos.clear();
    let missing_gravity = "設計用せん断力に必要な重力参照内力が不足しています";
    assert_reasons(&app, -40.0e6, missing_material, missing_gravity);

    app.core.model.sections[0].shear_rebar_material = material;
    assert_reasons(&app, f64::NAN, "組合せの軸力Nが非有限です", missing_gravity);
}

#[test]
fn concrete_beam_th_detail_keeps_bending_ng_and_shear_rebar_diagnostic_without_gravity() {
    use sepika_core::ids::MaterialId;
    use sepika_core::model::{MaterialCategory, Model};

    let src_model: Model =
        serde_json::from_str(include_str!("../../tests/fixtures/src_ql_contract.json")).unwrap();
    for (model, member) in [(fixture::model(), "RC梁"), (src_model, "SRC梁")] {
        let mut app = App::default();
        app.core.analysis_cfg.rc_damage_control = false;
        assert!(app.load_model(model));
        app.core.analysis_cfg.qd_method = QdMethod::Qd1;
        let model = &mut app.core.model;
        let original = model.sections[0].shear_rebar_material.unwrap();
        let mut wrong = model.materials[original.index()].clone();
        wrong.id = MaterialId(model.materials.len() as u32);
        wrong.category = MaterialCategory::Steel;
        model.sections[0].shear_rebar_material = Some(wrong.id);
        model.materials.push(wrong);
        let recording = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![(0.5, [0.0, 20_000.0, 0.0, 0.0, 0.0, 2e9])],
            })],
            ..Default::default()
        };

        let text = labels(&app, &recording, true);
        for message in ["ξ=0.50:", "(NG)", "曲げ ", member] {
            assert!(text.contains(message), "{text}");
        }
        if member == "RC梁" {
            for message in ["せん断補強筋材料「SD345」", "材料区分は「鋼材」"] {
                assert!(text.contains(message), "{text}");
            }
        } else {
            assert!(text.contains("付1.4-4"), "{text}");
            assert!(!text.contains("せん断補強筋材料"), "{text}");
        }
        assert_eq!(
            text.contains("重力参照内力が不足"),
            member == "RC梁",
            "{text}"
        );
        assert!(
            !text.contains("(OK)") && !text.contains("せん断 "),
            "{text}"
        );

        if member == "RC梁" {
            app.core.model.elements[0].local_axis.ref_vector = [f64::NAN, 0.0, 1.0];
            let text = labels(&app, &recording, true);
            assert!(text.contains("Q0"), "{text}");
            assert!(
                text.contains("(NG)") && text.contains("せん断補強筋材料"),
                "{text}"
            );
        }
    }
}

#[test]
fn rc_beam_th_detail_uses_available_gravity_to_keep_an_independent_bond_failure() {
    use sepika_core::ids::MaterialId;
    use sepika_core::model::MaterialCategory;

    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    assert!(app.load_model(fixture::model()));
    app.run_combination(1);
    app.core.analysis_cfg.qd_method = QdMethod::Qd2;
    app.core.analysis_cfg.bond_method = sepika_design_jp::BondMethod::Rc1991;
    let long = &mut app.core.scoped.results.as_mut().unwrap().combos[0]
        .1
        .member_forces[0]
        .1
        .at;
    *long = vec![(0.5, [0.0; 6])];

    let model = &mut app.core.model;
    let mut wrong = model.materials[1].clone();
    wrong.id = MaterialId(2);
    wrong.category = MaterialCategory::Steel;
    model.sections[0].shear_rebar_material = Some(wrong.id);
    model.materials.push(wrong);
    let recording = ThRecording {
        peak_member_forces: vec![Some(MemberForces {
            at: [0.5, 1.0]
                .map(|pos| (pos, [0.0, 10e6, 0.0, 0.0, 0.0, 1e6]))
                .to_vec(),
        })],
        ..Default::default()
    };

    let text = labels(&app, &recording, true);
    let (_, at_middle) = text.split_once("ξ=0.50:").unwrap();
    let (at_middle, at_end) = at_middle.split_once("ξ=1.00:").unwrap();
    assert!(
        at_middle.contains("付着 ") && at_middle.contains("(NG)"),
        "{text}"
    );
    assert!(!at_middle.contains("重力参照内力が不足"), "{text}");
    assert!(at_end.contains("重力参照内力が不足"), "{text}");
    assert!(
        !at_end.contains("付着 ") && !at_end.contains("(NG)"),
        "{text}"
    );
    for row in [at_middle, at_end] {
        assert!(
            row.contains("せん断補強筋材料") && !row.contains("(OK)"),
            "{text}"
        );
    }
}

fn set_gravity_forces(app: &mut App, at: Option<Vec<(f64, [f64; 6])>>) {
    use sepika_solver::statics::linear::StaticOnce;

    let gravity = app
        .core
        .model
        .combinations
        .iter()
        .find(|combo| sepika_core::load_combo::is_gravity_combination(&combo.name, &app.core.model))
        .expect("常時重力組合せ");
    app.core.scoped.results = Some(crate::app::ResultsBundle {
        combos: vec![(
            gravity.name.clone(),
            StaticOnce {
                story_shears: None,
                stiffness_ratio_disp: None,
                story_rigidity: Vec::new(),
                disp: Vec::new(),
                member_forces: at
                    .map(|at| (app.core.model.elements[0].id, MemberForces { at }))
                    .into_iter()
                    .collect(),
                column_end_horizontal_forces: Vec::new(),
                column_end_axial_forces: Vec::new(),
                panel_moments: Vec::new(),
                girder_column_face_forces: Vec::new(),
                steel_panel_column_face_forces: Vec::new(),
            },
        )],
        ..Default::default()
    });
}

fn gravity_test_record(envelope: [f64; 6], snapshot: [f64; 6]) -> ThRecording {
    ThRecording {
        peak_member_forces: vec![Some(MemberForces {
            at: [0.0, 0.5, 1.0].map(|xi| (xi, envelope)).to_vec(),
        })],
        axial_force_peaks: vec![AxialForcePeaks {
            snapshots: vec![ForcePeakSnapshot {
                time: 0.25,
                forces: MemberForces {
                    at: vec![(0.0, snapshot), (0.75, snapshot), (1.0, snapshot)],
                },
                positions: vec![1],
            }],
        }],
        ..Default::default()
    }
}

#[test]
fn time_history_gravity_gaps_are_scoped_to_each_position_and_required_component() {
    use crate::app::{
        cft_column_buckling_tests::input as cft, rc_column_boundary_tests::input as rc,
    };
    use sepika_core::model::Model;
    use sepika_design_jp::QdMethod;

    let load = |model: Model| {
        let mut app = App::default();
        app.core.analysis_cfg.rc_damage_control = false;
        assert!(app.load_model(model));
        app.core.analysis_cfg.qd_method = QdMethod::Qd2;
        app
    };
    let rc_app = |circle| {
        load(super::rc_column_mechanism_input::column_with_girder(
            rc::column_model(circle, 0.0),
        ))
    };
    let cft_app = |circle| {
        let mut model = cft::cantilever(circle, 0.0);
        model.steel_design_attrs = vec![cft::direct_lengths(ElemId(0), Some(3000.0), Some(3000.0))];
        load(model)
    };
    let response = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
    let rec = gravity_test_record(response, response);

    for (member, mut app) in [
        ("RC矩形", rc_app(false)),
        ("RC円形", rc_app(true)),
        ("CFT角形", cft_app(false)),
        ("CFT円形", cft_app(true)),
    ] {
        for missing_combo in [true, false] {
            set_gravity_forces(&mut app, None);
            if missing_combo {
                app.core.scoped.results.as_mut().unwrap().combos.clear();
            }
            let all_missing = labels(&app, &rec, true);
            assert_eq!(
                all_missing.matches("検定不能:").count(),
                4,
                "{member}: {all_missing}"
            );
            assert!(all_missing.contains("重力参照"), "{member}: {all_missing}");
            assert!(
                all_missing.contains("t=0.250000 s  ξ=0.75:"),
                "{member}: {all_missing}"
            );
            assert!(!all_missing.contains("検定比 "), "{member}: {all_missing}");
            for (nonlinear, with_gravity) in [(false, false), (false, true), (true, false)] {
                let ordinary = peak_labels(&app, &rec, 0, nonlinear, with_gravity);
                assert_eq!(ordinary.matches("(OK)").count(), 4, "{member}: {ordinary}");
                assert!(!ordinary.contains("検定不能"), "{member}: {ordinary}");
            }
        }

        let valid = [f64::NAN, 0.0, -0.0, f64::NAN, f64::INFINITY, f64::NAN];
        let mut gy_missing = valid;
        gy_missing[1] = f64::NAN;
        set_gravity_forces(
            &mut app,
            Some(vec![
                (0.0, valid),
                (0.5, gy_missing),
                (0.750_000_1, valid),
                (1.0, valid),
            ]),
        );
        let split = labels(&app, &rec, true);
        assert!(split.contains("ξ=0.50:\n検定不能:"), "{member}: {split}");
        assert!(
            split.contains("t=0.250000 s  ξ=0.75:\n検定不能:"),
            "{member}: {split}"
        );
        assert_eq!(split.matches("(OK)").count(), 2, "{member}: {split}");
        assert_eq!(split.matches("検定不能:").count(), 2, "{member}: {split}");
        assert!(split.contains("Qy方向"), "{member}: {split}");
        assert!(split.contains("Qz方向"), "{member}: {split}");

        set_gravity_forces(
            &mut app,
            Some([0.0, 0.5, 0.75, 1.0].map(|xi| (xi, valid)).to_vec()),
        );
        let complete = labels(&app, &rec, true);
        assert_eq!(complete.matches("(OK)").count(), 4, "{member}: {complete}");
        assert!(!complete.contains("検定不能"), "{member}: {complete}");

        set_gravity_forces(
            &mut app,
            Some([0.0, 0.5, 0.75, 1.0].map(|xi| (xi, gy_missing)).to_vec()),
        );
        let mut independent_ng = response;
        independent_ng[2] = 2.0e6;
        let rec = gravity_test_record(independent_ng, independent_ng);
        let text = labels(&app, &rec, true);
        assert_eq!(text.matches("(NG)").count(), 4, "{member}: {text}");
        assert_eq!(text.matches("検定不能:").count(), 4, "{member}: {text}");
        assert!(!text.contains("(OK)"), "{member}: {text}");
        assert!(text.contains("せん断 "), "{member}: {text}");
        assert_eq!(
            text.contains("検定比の下限"),
            member == "CFT円形",
            "{member}: {text}"
        );

        if member.starts_with("CFT") {
            let shear_area_mm2 = if member == "CFT円形" {
                std::f64::consts::PI * (200.0_f64.powi(2) - 188.0_f64.powi(2)) / 2.0
            } else {
                2.0 * 12.0 * 376.0
            };
            let allowable_shear_n = shear_area_mm2 * 235.0 / 3.0_f64.sqrt();
            let ordinary = peak_labels(&app, &rec, 0, true, false);
            let ordinary_ratio =
                2.0e6_f64.hypot(if member == "CFT円形" { 1000.0 } else { 0.0 }) / allowable_shear_n;
            assert!(
                ordinary.contains(&format!("せん断 {ordinary_ratio:.2}")),
                "{member}: {ordinary}"
            );

            let mut complete = valid;
            complete[2] = -100_000.0;
            set_gravity_forces(
                &mut app,
                Some([0.0, 0.5, 0.75, 1.0].map(|xi| (xi, complete)).to_vec()),
            );
            let qd2 = labels(&app, &rec, true);
            let qd_z = 100_000.0 + 1.5 * 2_100_000.0;
            let expected = f64::hypot(qd_z, if member == "CFT円形" { 1500.0 } else { 0.0 })
                / allowable_shear_n;
            assert!(
                qd2.contains(&format!("せん断 {expected:.2}")),
                "{member}: {qd2}"
            );
            assert!(!qd2.contains("検定不能"), "{member}: {qd2}");
        }

        let mut axial_ng = response;
        axial_ng[0] = -30.0e6;
        axial_ng[5] = 1.0;
        let rec = gravity_test_record(axial_ng, axial_ng);
        set_gravity_forces(&mut app, None);
        let text = labels(&app, &rec, true);
        assert_eq!(text.matches("(NG)").count(), 4, "{member}: {text}");
        assert!(text.contains("軸+曲げ "), "{member}: {text}");
        assert!(!text.contains("(OK)"), "{member}: {text}");
        if member.starts_with("RC") {
            assert!(text.contains("許容曲げが非正"), "{member}: {text}");
        }
    }
}

#[test]
fn time_history_rc_qd1_uses_end_axial_and_retains_connected_girder_diagnostics() {
    use super::rc_column_mechanism_input as input;
    use crate::app::rc_column_boundary_tests::input as rc;
    use sepika_design_jp::QdMethod;

    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    assert!(app.load_model(input::column_with_girder(rc::column_model(false, 0.0))));
    let response = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
    let rec = gravity_test_record(response, response);
    let end = [0.0, f64::NAN, f64::INFINITY, f64::NAN, f64::NAN, f64::NAN];
    let center = [0.0; 6];

    for method in [QdMethod::Qd1, QdMethod::Min] {
        app.core.analysis_cfg.qd_method = method;
        app.core.model.sections[input::GIRDER_SECTION.index()].rebar_material =
            Some(input::WRONG_REBAR);
        app.core.model.sections[0].shear_rebar_material = Some(input::WRONG_REBAR);
        for head in [None, Some([f64::NAN, 0.0, 0.0, 0.0, 0.0, 0.0])] {
            let forces = vec![(0.0, end), (0.5, center), (0.75, center), (0.98, center)];
            set_gravity_forces(
                &mut app,
                Some(
                    forces
                        .into_iter()
                        .chain(head.map(|head| (1.0, head)))
                        .collect(),
                ),
            );
            let text = labels(&app, &rec, true);
            for reason in [
                "柱頭の重力参照軸力N",
                "大梁 ID 1",
                "せん断補強筋",
                "Qy 方向",
                "Qz 方向",
            ] {
                assert!(text.contains(reason), "{method:?}: {text}");
            }
            assert!(!text.contains("(OK)"), "{method:?}: {text}");
        }

        app.core.model.sections[input::GIRDER_SECTION.index()].rebar_material = Some(input::REBAR);
        app.core.model.sections[0].shear_rebar_material = Some(input::REBAR);
        set_gravity_forces(&mut app, Some(vec![(0.0, end), (1.0, end)]));
        let text = labels(&app, &rec, true);
        if method == QdMethod::Qd1 {
            assert!(text.contains("せん断 "), "{text}");
            assert!(!text.contains("検定不能:"), "{text}");
        } else {
            assert_eq!(text.matches("検定不能:").count(), 4, "{text}");
            assert!(!text.contains("(OK)"), "{text}");
        }
    }
}

#[test]
fn time_history_cft_qd1_uses_only_necessary_gravity_components() {
    use crate::app::cft_column_buckling_tests::input as cft;
    use sepika_design_jp::QdMethod;

    for circular in [false, true] {
        let mut app = App::default();
        let mut model = cft::cantilever(circular, 0.0);
        model.steel_design_attrs = vec![cft::direct_lengths(ElemId(0), Some(3000.0), Some(3000.0))];
        assert!(app.load_model(model));
        let response = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
        let rec = gravity_test_record(response, response);
        app.core.analysis_cfg.qd_method = QdMethod::Qd1;
        set_gravity_forces(&mut app, None);
        let with_no_ql = labels(&app, &rec, true);
        assert_eq!(with_no_ql.matches("せん断 ").count(), 4, "{with_no_ql}");
        assert!(!with_no_ql.contains("検定不能"), "{with_no_ql}");
        set_gravity_forces(
            &mut app,
            Some([0.0, 0.5, 0.75, 1.0].map(|xi| (xi, [1.0e20; 6])).to_vec()),
        );
        assert_eq!(labels(&app, &rec, true), with_no_ql);
        set_gravity_forces(&mut app, None);
        app.core.analysis_cfg.qd_method = QdMethod::Min;
        let text = labels(&app, &rec, true);
        assert_eq!(text.matches("検定不能:").count(), 4, "{text}");
        assert!(!text.contains("(OK)"), "{text}");
    }
}

#[test]
fn time_history_cft_shows_buckling_and_required_gravity_reasons_at_each_position() {
    use crate::app::cft_column_buckling_tests::input as cft;
    use sepika_design_jp::QdMethod;

    for circular in [false, true] {
        let mut app = App::default();
        let mut model = cft::cantilever(circular, 0.0);
        model.steel_design_attrs = vec![cft::direct_lengths(ElemId(0), Some(3000.0), None)];
        assert!(app.load_model(model));
        let response = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
        let rec = gravity_test_record(response, response);
        let mut missing_qy = [0.0; 6];
        missing_qy[1] = f64::NAN;
        let mut missing_qz = [0.0; 6];
        missing_qz[2] = f64::INFINITY;
        set_gravity_forces(
            &mut app,
            Some(vec![(0.0, [0.0; 6]), (0.5, missing_qy), (0.75, missing_qz)]),
        );

        for weak_length in [None, Some(0.0), Some(f64::NAN)] {
            app.core.model.steel_design_attrs[0].lk_z_direct = weak_length;
            for (method, nonlinear, applied_long_term) in [
                (QdMethod::Qd2, true, true),
                (QdMethod::Min, true, true),
                (QdMethod::Qd1, true, true),
                (QdMethod::Qd2, true, false),
                (QdMethod::Qd2, false, false),
            ] {
                app.core.analysis_cfg.qd_method = method;
                let text = peak_labels(&app, &rec, 0, nonlinear, applied_long_term);
                assert!(!text.contains("検定比 "), "{text}");
                assert!(!text.contains("(OK)") && !text.contains("(NG)"), "{text}");
                let reason = "弱軸の座屈長さを有限な正値で直接指定";
                let shows_gravity = nonlinear && applied_long_term && method != QdMethod::Qd1;
                assert_eq!(
                    text.matches(reason).count(),
                    if shows_gravity { 4 } else { 3 },
                    "{text}"
                );
                if !shows_gravity {
                    assert!(
                        !text.contains("重力参照内力") && !text.contains("t=0.250000 s"),
                        "{text}"
                    );
                    continue;
                }

                for (label, qy_missing, qz_missing) in [
                    ("ξ=0.00:", false, false),
                    ("ξ=0.50:", true, false),
                    ("ξ=1.00:", true, true),
                    ("圧縮側 t=0.250000 s  ξ=0.75:", false, true),
                ] {
                    let (_, after_label) = text
                        .split_once(label)
                        .unwrap_or_else(|| panic!("{label}: {text}"));
                    let row = after_label.lines().nth(1).unwrap();
                    assert!(
                        row.starts_with("検定不能:") && row.contains(reason),
                        "{text}"
                    );
                    assert_eq!(row.contains("Qy方向の設計用せん断力"), qy_missing, "{text}");
                    assert_eq!(row.contains("Qz方向の設計用せん断力"), qz_missing, "{text}");
                }
            }
        }
    }
}

#[test]
fn time_history_rc_beam_qd1_does_not_require_a_support_beyond_the_connection() {
    use sepika_core::{dof::Dof6Mask, model::EndCondition};

    let mut model = rc_beam_qd1_model::model();
    rc_beam_qd1_model::add_columns_at_outer_nodes(&mut model, [1.0, 1.0, 0.0]);
    let direct = model.clone();
    rc_beam_qd1_model::extend_j_column(&mut model);
    let extended_fixed = model.clone();
    model.nodes[4].restraint = Dof6Mask::FREE;
    let mut pinned = model.clone();
    pinned.elements[0].end_cond[1] = EndCondition::Pinned;
    let rec = gravity_test_record(
        [0.0, 30_000.0, 0.0, 0.0, 0.0, 10e6],
        [0.0, 30_000.0, 0.0, 0.0, 0.0, 10e6],
    );
    let mut expected_shear = None;
    for (support, model, released) in [
        ("直接固定", direct, false),
        ("延長先固定", extended_fixed, false),
        ("延長先自由", model, false),
        ("延長先自由・梁端ピン", pinned, true),
    ] {
        let mut app = App::default();
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd1;
        assert!(app.load_model(model));
        set_gravity_forces(
            &mut app,
            Some(
                [0.0, 0.5, 0.75, 1.0]
                    .map(|xi| (xi, [0.0, -10_000.0, 0.0, 0.0, 0.0, 0.0]))
                    .to_vec(),
            ),
        );
        let text = labels(&app, &rec, true);
        if released {
            assert!(
                text.contains("QD1用の梁のj端で強軸曲げが解放"),
                "{support}: {text}"
            );
            continue;
        }
        let shear = text
            .lines()
            .find_map(|line| line.split_once("せん断 ").map(|(_, value)| value))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("{support}: {text}"));
        assert!(shear > 0.0, "{support}: {text}");
        assert!(!text.contains("検定不能"), "{support}: {text}");
        assert_eq!(
            shear,
            *expected_shear.get_or_insert(shear),
            "{support}: {text}"
        );
    }
}

#[test]
fn time_history_qd1_requires_qy_reaction_at_a_direct_support_and_keeps_bending_ng() {
    use sepika_core::dof::Dof;
    use sepika_design_jp::QdMethod;

    let mut app = App::default();
    let mut model = crate::app::signed_q0_acceptance_tests::grouped_rc_qd1_model(false);
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    assert!(app.load_model(model));
    app.core.analysis_cfg.rc_damage_control = false;
    let record = gravity_test_record(
        [0.0, 60_000.0, 0.0, 0.0, 0.0, 1e9],
        [0.0, 60_000.0, 0.0, 0.0, 0.0, 1e9],
    );
    set_gravity_forces(
        &mut app,
        Some(
            [0.0, 0.5, 0.75, 1.0]
                .map(|x| (x, [0.0, -10_000.0, 0.0, 0.0, 0.0, 0.0]))
                .to_vec(),
        ),
    );
    for supported in [true, false] {
        app.core.model.nodes[1].restraint.set(Dof::Uz, supported);
        assert_eq!(
            sepika_job::design_q0::rc_beam_q0_for_gravity_cases(
                &app.core.model,
                &app.core.model.elements[0]
            ),
            Ok(Some(20_000.0))
        );
        for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
            app.core.analysis_cfg.qd_method = method;
            let text = labels(&app, &record, true);
            let (envelope, snapshot) = text.split_once("軸力極値が生じた確定ステップ").unwrap();
            for part in [envelope, snapshot] {
                assert!(part.contains("(NG)"), "{method:?}: {text}");
                if supported || method == QdMethod::Qd2 {
                    assert!(
                        part.contains("せん断 ") && !part.contains("検定不能"),
                        "{method:?}: {text}"
                    );
                } else {
                    assert!(!part.contains("せん断 "), "{method:?}: {text}");
                    assert!(
                        part.contains("検定不能: QD1用の梁のj端で局所Qy方向の反力"),
                        "{method:?}: {text}"
                    );
                }
            }
        }
    }
    app.core.analysis_cfg.qd_method = QdMethod::Qd1;
    app.core.model.load_cases[0].member[0].dir = [0.0; 3];
    let text = labels(&app, &record, true);
    assert!(
        text.contains("部材荷重由来Q0が不足または不正") && text.contains("j端で局所Qy方向の反力"),
        "{text}"
    );
}

#[test]
fn signed_q0_th_detail_released_beam_keeps_independent_bending_failure() {
    use sepika_core::model::EndCondition;

    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    app.load_model(fixture::model());
    app.run_combination(0);
    let mut peak = app.core.scoped.results.as_ref().unwrap().member_forces[0]
        .1
        .clone();
    for (_, f) in &mut peak.at {
        f[5] = 1e9;
    }
    app.run_combination(1);
    let rec = ThRecording {
        peak_member_forces: vec![Some(peak)],
        ..Default::default()
    };
    app.core.model.elements[0].end_cond[0] = EndCondition::Pinned;

    for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
        app.core.analysis_cfg.qd_method = method;
        let text = labels(&app, &rec, true);
        assert!(text.contains("NG"), "{method:?}: {text}");
        if method == QdMethod::Qd2 {
            assert!(!text.contains("強軸曲げが解放"), "{text}");
            assert!(!text.contains("検定不能"), "{text}");
        } else {
            assert!(
                text.contains("検定不能: QD1用の梁のi端で強軸曲げが解放"),
                "{text}"
            );
        }
    }
}

#[test]
fn time_history_damage_control_uses_the_combined_committed_force_and_not_the_envelope() {
    use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
    use sepika_design_jp::QdMethod;
    let mut app = App::default();
    let mut model = fixture::model();
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D22);
    assert!(app.load_model(model));
    app.core.analysis_cfg.bond_method = sepika_design_jp::BondMethod::Rc1991;
    app.core.analysis_cfg.rc_damage_control = true;
    let rec = gravity_test_record(
        [0.0, 900_000.0, 0.0, 0.0, 0.0, 100e6],
        [-50_000.0, -200_000.0, 0.0, 0.0, 0.0, 30e6],
    );
    for method in [QdMethod::Qd1, QdMethod::Qd2, QdMethod::Min] {
        app.core.analysis_cfg.qd_method = method;
        let text = peak_labels_with_confirmed_long_term(&app, &rec, 0, true, true, true);
        // Fc24・SD345、b=300 mm、d=539 mm、α=2、2-φ10@100 の許容値は285.551 kN。
        assert_eq!(text.matches("せん断 ").count(), 1, "{text}");
        assert_eq!(text.matches("付着 ").count(), 1, "{text}");
        assert!(text.contains("せん断 0.70"), "{text}");
        assert!(
            text.contains("異なる時刻の内力包絡では算定しません"),
            "{text}"
        );
        assert!(text.contains("全時刻の適否は未判定"), "{text}");
        assert!(!text.contains("地震時 QD / 柱メカニズム"), "{text}");
    }
    for (nonlinear, apply) in [(false, false), (true, false), (true, true)] {
        let text = peak_labels_with_confirmed_long_term(&app, &rec, 0, nonlinear, apply, false);
        assert!(!text.contains("せん断 "), "{text}");
        assert!(!text.contains("付着 "), "{text}");
        assert!(
            text.contains("長期荷重を含む応答を確認できません"),
            "{text}"
        );
    }
}

#[test]
fn time_history_damage_control_requires_current_generation_input_and_nonempty_long_term_loads() {
    use sepika_core::model::LoadCaseKind;
    use sepika_solver::dynamic::timehistory::{GroundMotion, ResponseResult};
    let make_app = |kinds: [Option<LoadCaseKind>; 2]| {
        let mut app = App::default();
        let mut model = fixture::model();
        for (case, kind) in model.load_cases.iter_mut().zip(kinds) {
            if let Some(kind) = kind {
                case.kind = kind;
            } else {
                case.member.clear();
                case.nodal.clear();
            }
        }
        assert!(app.load_model(model));
        app.core.analysis_cfg.th_nonlinear = true;
        app.core.analysis_cfg.th_apply_long_term = true;
        let input = app
            .time_history_input(
                GroundMotion {
                    dt: 0.02,
                    accel_x: vec![0.0, 0.0],
                    accel_y: None,
                    accel_theta: None,
                },
                "損傷制御の試験波".into(),
            )
            .unwrap();
        app.apply_time_history_result(
            Ok(ResponseResult {
                wall_capacities: vec![],
                rc_wall_shear_method: Default::default(),
                time: vec![0.0, 0.02],
                peak_disp: vec![],
                story_drift_angle: vec![],
                history: Default::default(),
                recording: None,
                nonlinear: true,
                applied_long_term: true,
                non_converged_steps: 0,
            }),
            input.source,
        );
        app
    };
    let issue = |app: &App| {
        let view = &app.ui.view;
        let result = view.spatial_result(app).unwrap();
        damage_control_gravity_issue(app, view, &app.core.model, result)
    };
    let mut app = make_app([Some(LoadCaseKind::Dead), Some(LoadCaseKind::Live)]);
    assert_eq!(issue(&app), None);
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .input_records
        .clear();
    assert_eq!(issue(&app), Some(UNVERIFIED_DAMAGE_CONTROL_GRAVITY));
    assert!(issue(&make_app([None, None]))
        .unwrap()
        .contains("初期載荷がありません"));
    assert_eq!(issue(&make_app([Some(LoadCaseKind::Dead), None])), None);
    for kinds in [
        [Some(LoadCaseKind::Seismic), Some(LoadCaseKind::Live)],
        [Some(LoadCaseKind::Dead), Some(LoadCaseKind::Seismic)],
    ] {
        assert!(issue(&make_app(kinds))
            .unwrap()
            .contains("両荷重ケースを明示"));
    }
    for kinds in [
        [Some(LoadCaseKind::Other), None],
        [Some(LoadCaseKind::Dead), Some(LoadCaseKind::Other)],
        [Some(LoadCaseKind::Dead), Some(LoadCaseKind::Snow)],
    ] {
        assert!(issue(&make_app(kinds))
            .unwrap()
            .contains("固定・架構用積載以外"));
    }
}

#[test]
fn grouped_rc_qd1_time_history_stays_unchecked_instead_of_using_segment_reactions() {
    let mut app = App::default();
    app.core.analysis_cfg.rc_damage_control = false;
    assert!(app.load_model(crate::app::signed_q0_acceptance_tests::grouped_rc_qd1_model(true)));
    app.run_combination(0);
    let rec = ThRecording {
        peak_member_forces: app
            .current_static()
            .unwrap()
            .member_forces
            .iter()
            .map(|(_, mf)| Some(mf.clone()))
            .collect(),
        ..Default::default()
    };
    app.run_combination(1);
    for elem_index in 0..2 {
        for method in [
            sepika_design_jp::QdMethod::Qd1,
            sepika_design_jp::QdMethod::Min,
        ] {
            app.core.analysis_cfg.qd_method = method;
            let text = member_labels(&app, &rec, elem_index, true);
            assert!(text.contains("一本部材グループ"), "{text}");
        }
        app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Qd2;
        let text = member_labels(&app, &rec, elem_index, true);
        assert!(!text.contains("一本部材グループ"), "{text}");
        assert!(text.contains("検定比"), "{text}");
    }
}

#[test]
fn time_history_qd1_shows_slab_warning_and_uses_recorded_reinforcement_when_confirmed() {
    let model = rc_beam_qd1_model::beam_with_recorded_continuous_slab_rebar();
    let mut app = App::default();
    let rec = gravity_test_record(
        [0.0, 60_000.0, 0.0, 0.0, 0.0, 10e6],
        [0.0, 60_000.0, 0.0, 0.0, 0.0, 10e6],
    );
    let mut ratios = Vec::new();
    for confirmed in [false, true] {
        let mut input = model.clone();
        if !confirmed {
            for slab in &mut input.slabs {
                slab.plate.top_reinforcement = None;
            }
        }
        assert!(app.load_model(input));
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd1;
        set_gravity_forces(
            &mut app,
            Some(
                [0.0, 0.5, 0.75, 1.0]
                    .map(|x| (x, [0.0, -10_000.0, 0.0, 0.0, 0.0, 0.0]))
                    .to_vec(),
            ),
        );
        let text = labels(&app, &rec, true);
        assert_eq!(text.contains("QD1に算入していません"), !confirmed, "{text}");
        assert_eq!(
            text.contains("設計用せん断力を過小評価"),
            !confirmed,
            "{text}"
        );
        let shear = text
            .lines()
            .find_map(|line| line.strip_prefix("せん断 "))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("{text}"));
        ratios.push(shear);
    }
    assert!(ratios[0] > 0.0 && ratios[0] < ratios[1], "{ratios:?}");
}

#[test]
fn slab_qd1_inputs_and_checked_warning_round_trip_in_ovika_without_changing_analysis_identity() {
    use sepika_core::ids::ElemId;
    use sepika_design_jp::{CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod};
    use sepika_element::frame::beam::MemberForces;
    use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};

    let mut model = rc_beam_qd1_model::beam_with_recorded_continuous_slab_rebar();
    let original_analysis_identity = model.calculation_model();
    let slab = model
        .slabs
        .iter_mut()
        .find(|slab| {
            slab.plate
                .top_reinforcement
                .as_ref()
                .is_some_and(|layout| !layout.straight_bars.is_empty())
        })
        .unwrap();
    slab.plate.top_reinforcement.as_mut().unwrap().straight_bars[0].ends_mm[1][0] = 6501.0;
    assert!(original_analysis_identity.eq_ignoring_dofmap(&model.calculation_model()));

    let response = [(
        ElemId(0),
        MemberForces {
            at: vec![(0.5, [0.0, 60_000.0, 0.0, 0.0, 0.0, 10e6])],
        },
    )];
    let gravity = [(
        ElemId(0),
        MemberForces {
            at: vec![(0.5, [0.0, -10_000.0, 0.0, 0.0, 0.0, 0.0])],
        },
    )];
    let q0_by_elem = std::collections::HashMap::from([(ElemId(0), 0.0)]);
    let options = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        qd_method: QdMethod::Qd1,
        long_member_forces: Some(&gravity),
        q_simple_by_elem: Some(&q0_by_elem),
        ..Default::default()
    };
    let checks = sepika_design_jp::run_member_design_checks(&model, &response, &[], &options)
        .unwrap()
        .member_checks;
    let path = std::env::temp_dir().join(format!("sepika-qd1-slab-{}.ovika", uuid::Uuid::new_v4()));
    let results = rmp_serde::to_vec_named(&checks).unwrap();
    save_ovika(
        &path,
        &model,
        OvikaExtras {
            results: Some(&results),
            ..Default::default()
        },
    )
    .unwrap();
    let restored = load_ovika(&path);
    std::fs::remove_file(path).unwrap();
    let restored = restored.unwrap();
    assert_eq!(model.slabs, restored.model.slabs);
    let restored_checks: Vec<(ElemId, f64, CheckOutcome)> =
        rmp_serde::from_slice(&restored.results.unwrap()).unwrap();
    assert_eq!(checks, restored_checks);
    let CheckOutcome::Checked(checked) = &restored_checks[0].2 else {
        panic!("{restored_checks:?}");
    };
    let shear = checked
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap();
    assert!(shear.ratio > 0.0 && shear.ratio <= 1.0);
    assert!(shear
        .rc_beam_qd1_slab_warning()
        .unwrap()
        .contains("過小評価"));
}

#[test]
fn time_history_eccentric_beam_replaces_only_the_automatic_face() {
    use sepika_core::ids::ElemId;
    use sepika_core::model::{MemberLoad, MemberLoadKind};
    let mut model = rc_beam_qd1_model::model();
    rc_beam_qd1_model::add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1000.0], [500.0, 0.0, 1000.0]];
    model.load_cases[0].member = vec![MemberLoad::manual(
        ElemId(0),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Point {
            a: 1500.0,
            p: 1_000.0,
        },
    )];
    // L=5000 mmの実材軸で、左反力は1 kN × 3500/5000 = 0.7 kN。
    let q0 =
        sepika_job::design_q0::rc_beam_q0_for_gravity_cases(&model, &model.elements[0]).unwrap();
    assert_eq!(q0, Some(700.0));
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.rc_damage_control = false;
    app.core.analysis_cfg.qd_method = QdMethod::Qd1;
    set_gravity_forces(
        &mut app,
        Some(
            [0.0, 0.5, 1.0]
                .map(|x| (x, [0., -10_000., 0., 0., 0., 0.]))
                .to_vec(),
        ),
    );
    let mut record = gravity_test_record(
        [0., 30_000., 0., 0., 0., 10e6],
        [0., 30_000., 0., 0., 0., 10e6],
    );
    let forces = record.peak_member_forces[0].as_mut().unwrap();
    forces.at.insert(1, (0.05, [0., 30_000., 0., 0., 0., 1e10]));
    let text = labels(&app, &record, true);
    assert!(
        text.contains("ξ=0.00:") && text.contains("ξ=1.00:"),
        "{text}"
    );
    assert!(
        !text.contains("ξ=0.05:") && !text.contains("実柱面:") && text.contains("せん断 "),
        "{text}"
    );
    assert!(!text.contains("(NG)"), "{text}");
    let mut detail = sepika_core::model::MemberDetailAttr::new(ElemId(0));
    detail.joints.push(sepika_core::model::MemberJoint {
        distance: 250.0,
        kind: Default::default(),
    });
    app.core.model.member_detail_attrs.push(detail);
    let text = labels(&app, &record, true);
    assert!(text.contains("ξ=0.05:") && text.contains("(NG)"), "{text}");
    let forces = record.peak_member_forces[0].as_mut().unwrap();
    forces.at[0].0 = 1e-8;
    let text = labels(&app, &record, true);
    assert!(text.contains("保存解析断面力がありません"), "{text}");
}

#[test]
fn time_history_qd1_direct_support_failure_keeps_the_opposite_eccentric_material_end() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_design_jp::QdMethod;

    let mut model = rc_beam_qd1_model::model();
    rc_beam_qd1_model::add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements.pop().unwrap();
    model.nodes.pop().unwrap();
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Uz);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1_000.0]; 2];
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.rc_damage_control = false;
    app.core.analysis_cfg.qd_method = QdMethod::Min;
    set_gravity_forces(
        &mut app,
        Some(
            [0.0, 250.0 / 4500.0, 0.5, 1.0 - 250.0 / 4500.0]
                .map(|x| (x, [0.0, -10_000.0, 0.0, 0.0, 0.0, 0.0]))
                .to_vec(),
        ),
    );
    let mut record = gravity_test_record([0.0, 60_000.0, 0.0, 0.0, 0.0, 10e6], [0.0; 6]);
    let forces = &mut record.peak_member_forces[0].as_mut().unwrap().at;
    forces[0].1[5] = 1e9;
    forces.insert(1, (250.0 / 4500.0, [0.0, 60_000.0, 0.0, 0.0, 0.0, 10e6]));
    forces.last_mut().unwrap().0 = 1.0 - 250.0 / 4500.0;
    let text = labels(&app, &record, true);
    let at_material_end = text
        .split_once("ξ=0.00:")
        .unwrap()
        .1
        .split("ξ=0.50:")
        .next()
        .unwrap();
    assert!(
        at_material_end.contains("(NG)") && at_material_end.contains("j端で局所Qy方向の反力"),
        "{text}"
    );
    assert!(
        !text.contains("ξ=0.06:") && !text.contains("せん断 "),
        "{text}"
    );
}

#[test]
fn eccentric_beam_static_check_uses_saved_rotated_column_face_for_both_response_and_ql() {
    use sepika_core::ids::{ElemId, StoryId};
    use sepika_core::model::Story;
    use sepika_design_jp::{CheckKind, CheckOutcome};
    let mut model = rc_beam_qd1_model::model();
    rc_beam_qd1_model::add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1000.0], [0.0, 0.0, -500.0]];
    model.elements[2].local_axis.ref_vector = [1.0, 1.0, 0.0];
    model.sections[1].rebar_material = model.sections[0].rebar_material;
    model.sections[1].shear_rebar_material = model.sections[0].shear_rebar_material;
    for node in &mut model.nodes {
        node.coord[2] += 3000.0;
    }
    model.stories = vec![
        Story::new(StoryId(0), "1F".into(), Some(0.0), Some(0.0)),
        Story::new(StoryId(1), "2F".into(), Some(3000.0), Some(0.0)),
    ];
    for story in &mut model.stories {
        story.weight_override = Some(100_000.0);
    }
    let source = fixture::model();
    model.load_cases = source.load_cases;
    model.combinations = source.combinations;
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.rc_damage_control = false;
    app.core.analysis_cfg.qd_method = QdMethod::Qd1;
    app.run_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let result = app.current_static().unwrap();
    let (_, member) = result
        .member_forces
        .iter()
        .find(|(id, _)| *id == ElemId(0))
        .unwrap();
    let (_, faces) = result
        .girder_column_face_forces
        .iter()
        .find(|(id, _)| *id == ElemId(0))
        .unwrap();
    assert!(faces[0].is_err());
    let j = faces[1].as_ref().unwrap();
    let xi = 1.0 - 250.0 * std::f64::consts::SQRT_2 / 4500.0;
    let length_mm = f64::hypot(4500.0, 1500.0);
    assert!((j.face.position_mm - xi * length_mm).abs() < 1e-8);
    assert!(member.at.iter().all(|(pos, _)| (pos - xi).abs() > 1e-6));
    let checked = app.current_design_results().unwrap();
    let check = checked
        .member_checks
        .iter()
        .find(|m| m.elem == ElemId(0))
        .unwrap();
    let row = check
        .positions
        .iter()
        .find(|p| (p.xi - xi).abs() < 1e-12)
        .unwrap();
    let CheckOutcome::Checked(checked) = &row.outcome else {
        panic!("{:?}", check.positions);
    };
    let demand_n = 1200.0 + 293_562_225.0 / (xi * length_mm);
    let shear = checked
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap();
    assert!(
        shear.detail.contains(&format!("QD={demand_n:.1} N")),
        "{shear:?}"
    );
    assert!(checked
        .components
        .iter()
        .any(|c| c.kind == CheckKind::Bending
            && c.detail
                .contains(&format!("|mz|={:.1}", j.forces.as_ref().unwrap()[5].abs()))));
}

#[test]
fn history_eccentric_beam_does_not_substitute_a_nominal_face_for_an_unsaved_physical_face() {
    let mut model = rc_beam_qd1_model::model();
    rc_beam_qd1_model::add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1000.0], [0.0, 0.0, -500.0]];
    model.elements[2].local_axis.ref_vector = [1.0, 1.0, 0.0];
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.rc_damage_control = false;
    app.core.analysis_cfg.qd_method = QdMethod::Qd1;
    let rows = [0.0, 0.5, 0.95, 1.0];
    set_gravity_forces(
        &mut app,
        Some(rows.map(|xi| (xi, [0., -10_000., 0., 0., 0., 0.])).to_vec()),
    );
    let values = [0., 30_000., 0., 0., 0., 10e6];
    let mut record = gravity_test_record(values, values);
    record.peak_member_forces[0].as_mut().unwrap().at = rows.map(|xi| (xi, values)).to_vec();
    let text = labels(&app, &record, true);
    assert!(
        text.contains("ξ=0.92:") && text.contains("保存解析断面力がありません"),
        "{text}"
    );
    assert!(
        !text.contains("ξ=0.95:") && !text.contains("ξ=1.00:"),
        "{text}"
    );
}
