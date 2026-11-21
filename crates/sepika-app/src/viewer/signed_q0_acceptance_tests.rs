use super::*;
use crate::app::signed_q0_acceptance_tests::fixture;

fn labels(app: &App, rec: &ThRecording, applied_long_term: bool) -> String {
    member_labels(app, rec, 0, applied_long_term)
}

fn member_labels(app: &App, rec: &ThRecording, index: usize, applied_long_term: bool) -> String {
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
            |ui| draw_peak_check(ui, app, elem, index, rec, true, applied_long_term, &axis),
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
        peak_member_forces: vec![Some(moments(250e6))],
        ..Default::default()
    };
    let text = labels(&app, &rec, true);
    assert!(text.contains("検定比") && text.contains("(NG)"), "{text}");
    assert!(text.contains("採用横座屈長さ lb=3000 mm"), "{text}");
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
    let (_, shear_only_ng) = ordinary
        .split_once("引張側 t=0.750000 s  ξ=1.00:")
        .unwrap_or_else(|| panic!("{ordinary}"));
    assert!(shear_only_ng.contains("(NG)"), "{ordinary}");
    assert!(shear_only_ng.contains("軸+曲げ 0.00"), "{ordinary}");
    assert!(shear_only_ng.contains("せん断 "), "{ordinary}");
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
        assert!(!row.contains("検定比 "), "{text}");
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
        for message in [
            "ξ=0.50:",
            "(NG)",
            "曲げ ",
            member,
            "せん断補強筋材料「SD345」",
            "材料区分は「鋼材」",
            "断面タブでせん断補強筋に「鉄筋」区分の材料を割り当て",
            "重力参照内力が不足",
        ] {
            assert!(text.contains(message), "{text}");
        }
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
