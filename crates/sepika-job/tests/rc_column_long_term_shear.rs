#[path = "../../sepika-design-jp/tests/fixtures/rc_column_long_term_shear_model.rs"]
mod input;

#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;

use input::{distributed, COLUMN, LENGTH_MM};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementKind, FrameSectionUse, JointKind, MaterialCategory, MemberDetailAttr, MemberJoint,
    MemberLoad, MemberLoadKind, Model, NodalLoad, Section,
};
use sepika_core::section_shape::{RebarSize, SectionShape};
use sepika_design_jp::{
    run_member_design_checks, CheckComponent, CheckKind, CheckOutcome, MemberDesignCheckOptions,
    MemberDesignCheckReport, QdMethod, RcColumnLongTermShearInput,
};
use sepika_element::frame::beam::MemberForces;
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::{RcColumnFaceForce, StaticOnce};

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];

fn model(circle: bool, inner_peak: bool) -> Model {
    input::model(
        column_input::column_model(circle, 0.0),
        beam_input::section(300.0, 600.0, 19.0, &[3], &[3]),
        inner_peak,
    )
}

fn alpha_model(circle: bool, tip_moment_nmm: f64) -> Model {
    let mut model = model(circle, false);
    let section = &mut model.sections[0];
    let mut shape = section.shape.take().unwrap();
    let main_dia = RebarSize::DiameterMm(29.0);
    match &mut shape {
        SectionShape::RcColumnRect { rebar, .. } => rebar.main_dia = main_dia,
        SectionShape::RcColumnCircle { rebar, .. } => {
            rebar.main_dia = main_dia;
            rebar.count = 16;
        }
        other => panic!("{other:?}"),
    }
    *section = Section {
        material: section.material,
        rebar_material: section.rebar_material,
        shear_rebar_material: section.shear_rebar_material,
        frame_use: section.frame_use,
        ..shape.to_section(section.id, section.name.clone())
    };
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [25_000.0, 0.0, 0.0, 0.0, tip_moment_nmm, 0.0],
    )];
    model.load_cases[0].member = vec![distributed(X, 0.0, LENGTH_MM, 60.0, 60.0)];
    model
}

fn analyze(model: &mut Model) -> StaticOnce {
    sepika_job::prepare_model_for_analysis(model, &Default::default(), None).unwrap();
    let combo = &model.combinations[0];
    Analysis::prepare(model)
        .unwrap()
        .linear_combination(combo)
        .unwrap()
}

fn check(
    model: &Model,
    solved: &StaticOnce,
    source: RcColumnLongTermShearInput<'_>,
) -> Vec<(f64, CheckOutcome)> {
    check_report(model, solved, source)
        .member_checks
        .into_iter()
        .filter(|(id, _, _)| *id == COLUMN)
        .map(|(_, xi, outcome)| (xi, outcome))
        .collect()
}

fn check_report(
    model: &Model,
    solved: &StaticOnce,
    source: RcColumnLongTermShearInput<'_>,
) -> MemberDesignCheckReport {
    run_member_design_checks(
        model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions {
            rc_column_long_term_shear: source,
            rc_column_face_forces: &solved.rc_column_face_forces,
            ..Default::default()
        },
    )
    .unwrap()
}

fn check_combination(model: &Model, solved: &StaticOnce) -> Vec<(f64, CheckOutcome)> {
    check(
        model,
        solved,
        RcColumnLongTermShearInput::SavedCombination(&model.combinations[0].name),
    )
}

fn shear(outcome: &CheckOutcome) -> &CheckComponent {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap()
}

fn point(a: f64, p: f64) -> MemberLoad {
    MemberLoad::manual(COLUMN, X, MemberLoadKind::Point { a, p })
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn 回転大梁の実面の符号付き断面力を長期と短期の検定に使う() {
    for circle in [false, true] {
        for (reference, section_mm) in [([0.0, 0.0, 1.0], 300.0), (Y, 150.0)] {
            let mut model = model(circle, false);
            model.elements[1].local_axis.ref_vector = reference;
            model.load_cases[0].nodal = vec![NodalLoad::manual(
                NodeId(1),
                [20_000.0, 5000.0, -40_000.0, 1e6, 2e6, 0.0],
            )];
            model.load_cases[0].member =
                vec![distributed([0.0, 0.0, 1.0], 0.0, LENGTH_MM, -10.0, -10.0)];
            let response = analyze(&mut model);
            let face = response
                .rc_column_face_forces
                .iter()
                .find(|(id, _)| *id == COLUMN)
                .unwrap()
                .1[0]
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!(face.face.position_mm, section_mm);
            let arm_mm = LENGTH_MM - section_mm;
            let area_mm2 = if circle {
                std::f64::consts::PI * 300.0_f64.powi(2)
            } else {
                400.0 * 600.0
            };
            let top_weight_n = 2.4e-9 * 9806.65 * area_mm2 * 1500.0;
            let expected = [
                -40_000.0 - 10.0 * arm_mm - top_weight_n,
                -20_000.0,
                -5000.0,
                0.0,
                1e6 - 5000.0 * arm_mm,
                2e6 + 20_000.0 * arm_mm,
            ];
            for (actual, expected) in face.forces.as_ref().unwrap().iter().zip(expected) {
                assert_close(*actual.as_ref().unwrap(), expected, 1e-4);
            }
            for term in [
                sepika_design_jp::LoadTerm::Long,
                sepika_design_jp::LoadTerm::Short,
            ] {
                let check = run_member_design_checks(
                    &model,
                    &response.member_forces,
                    &response.panel_moments,
                    &MemberDesignCheckOptions {
                        term,
                        rc_column_face_forces: &response.rc_column_face_forces,
                        rc_column_long_term_shear: RcColumnLongTermShearInput::SingleCase,
                        ..Default::default()
                    },
                )
                .unwrap();
                let station = section_mm / LENGTH_MM;
                assert!(check.member_checks.iter().any(|(id, xi, outcome)| *id == COLUMN && *xi == station
                    && matches!(outcome, CheckOutcome::Checked(result)
                        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending))),
                    "{check:?}");
                assert!(
                    section_mm == 300.0
                        || check
                            .member_checks
                            .iter()
                            .all(|(id, xi, _)| *id != COLUMN || *xi != 0.1)
                );
            }
        }
    }
}

#[test]
fn 回転大梁の実面で軸力ゼロの長期曲げ適否が変わる() {
    for (axis, station, is_ng) in [([0.0, 0.0, 1.0], 0.1, false), (Y, 0.05, true)] {
        let mut model = model(false, false);
        model.elements[1].local_axis.ref_vector = axis;
        model.load_cases[0].member.clear();
        model.load_cases[0].nodal = vec![NodalLoad::manual(
            NodeId(1),
            [36_100.0, 0.0, 8472.9456, 0.0, 0.0, 0.0],
        )];
        let response = analyze(&mut model);
        let rows = check_combination(&model, &response);
        let outcome = &rows.iter().find(|(xi, _)| *xi == station).unwrap().1;
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{rows:?}");
        };
        let nm = result
            .components
            .iter()
            .find(|part| part.kind == CheckKind::AxialBending)
            .unwrap();
        // 三角形コンクリート圧縮域と主筋合力の釣合いから、N=0の強軸許容曲げは約100.20 kN·m。
        let demand_knm = if is_ng { 102.885 } else { 97.470 };
        assert_close(nm.ratio, demand_knm / 100.20, 0.002);
        assert_eq!(nm.ratio > 1.0, is_ng, "{rows:?}");
    }
}

#[test]
fn 同じ位置の保存応答がない実面は未算定で中央の曲げ不適合は残す() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, 0.0, 0.0, 170e6, 0.0],
    ));
    let mut response = analyze(&mut model);
    response.rc_column_face_forces.clear();
    let rows = check_combination(&model, &response);
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.05
        && matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("実材端面の断面力がありません"))));
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.5 && matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0))));
    assert!(rows.iter().all(|(xi, _)| *xi != 0.1));
}

#[test]
fn 形状不明と非一意の柱脚を検定せず確認済みの柱頭を検定する() {
    let mut model = model(false, false);
    let response = analyze(&mut model);
    for (shape, axis, diagnostic) in [
        (None, [0.0, 0.0, 1.0], "実断面形状が未指定"),
        (
            model.sections[1].shape.clone(),
            [0.0, 1.0, 1.0],
            "接合境界を一つの断面位置で表せません",
        ),
    ] {
        model.sections[1].shape = shape;
        model.elements[1].local_axis.ref_vector = axis;
        let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
        assert!(rows.iter().any(|(xi, outcome)| *xi == 0.1
            && matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(diagnostic))), "{rows:?}");
        assert!(rows
            .iter()
            .all(|(xi, outcome)| *xi != 0.1 || !matches!(outcome, CheckOutcome::Checked(_))));
        assert!(rows
            .iter()
            .any(|(xi, outcome)| *xi == 1.0 && matches!(outcome, CheckOutcome::Checked(_))));
    }
}

#[test]
fn 実柱端面の集中荷重では柱区間側を採用する() {
    let mut model = model(false, false);
    let mut upper_end = model.nodes[2].clone();
    upper_end.id = NodeId(3);
    upper_end.coord[2] = LENGTH_MM;
    upper_end.restraint = sepika_core::dof::Dof6Mask::FREE;
    model.nodes.push(upper_end);
    let mut upper = model.elements[1].clone();
    upper.id = ElemId(2);
    upper.nodes = [NodeId(1), NodeId(3)].into_iter().collect();
    model.elements.push(upper);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [12_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    model.load_cases[0].member = vec![point(150.0, 50_000.0), point(2700.0, 30_000.0)];
    let response = analyze(&mut model);
    let ends = &response
        .rc_column_face_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .1;
    for (cut, (position, q)) in ends.iter().zip([(150.0, -42_000.0), (2700.0, -42_000.0)]) {
        let face = cut.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(face.face.position_mm, position);
        assert_close(*face.forces.as_ref().unwrap()[1].as_ref().unwrap(), q, 1e-5);
    }
    model.load_cases[0].member[0] = point(150.0 + 1e-11, 50_000.0);
    let response = analyze(&mut model);
    let ends = &response
        .rc_column_face_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .1;
    let i = ends[0]
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .forces
        .as_ref()
        .unwrap();
    assert!(matches!(&i[1], Err(reason) if reason.contains("丸め誤差")));
    assert!(matches!(&i[5], Err(reason) if reason.contains("丸め誤差")));
    assert!(i[0].is_ok() && i[2..5].iter().all(Result::is_ok));
    assert!(ends[1].as_ref().unwrap().as_ref().unwrap().forces.is_ok());
}

#[test]
fn 丸めで集中荷重と同じ座標になった実面は軸力だけ保留する() {
    for shift_mm in [0.0, 1000.0] {
        for (beam_offset_mm, expected_axial_n) in [
            (-0.01, Some(-38_472.946)),
            (-1e-14, None),
            (0.0, Some(-8_472.946)),
        ] {
            let mut model = model(false, false);
            translate_z(&mut model, shift_mm);
            model.elements[1].end_offsets_mm = [[0.0, 0.0, beam_offset_mm]; 2];
            model.load_cases[0].member = vec![MemberLoad::manual(
                COLUMN,
                [0.0, 0.0, 1.0],
                MemberLoadKind::Point {
                    a: 300.0,
                    p: -30_000.0,
                },
            )];
            let response = analyze(&mut model);
            let face = saved_column_face(&response, 0).as_ref().unwrap();
            let forces = face.forces.as_ref().unwrap();
            if let Some(expected) = expected_axial_n {
                assert_close(*forces[0].as_ref().unwrap(), expected, 0.2);
            } else {
                assert_eq!(face.face.position_mm, 300.0);
                assert!(matches!(&forces[0], Err(reason) if reason.contains("丸め誤差")));
                let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
                assert!(rows.iter().any(|(xi, outcome)| *xi == 0.1 && matches!(
                    outcome, CheckOutcome::Skipped { reason } if reason.contains("軸力N") && reason.contains("丸め誤差")
                )), "{rows:?}");
            }
            assert!(forces[1..].iter().all(Result::is_ok), "{forces:?}");
        }
    }
}

#[test]
fn 柱頭の実配置と解析材軸が一致しない場合は集中荷重の切断力を採用しない() {
    let mut model = model(false, false);
    model.nodes[2].coord[2] = LENGTH_MM;
    model.stories[1].elevation = Some(LENGTH_MM + 1.0);
    model.nodes[2].restraint = sepika_core::dof::Dof6Mask::FREE;
    model.elements[1].nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    model.elements[0].end_offsets_mm[1][2] = 0.00025;
    let load = |a| {
        MemberLoad::manual(
            COLUMN,
            [0.0, 0.0, 1.0],
            MemberLoadKind::Point { a, p: -30_000.0 },
        )
    };
    model.load_cases[0].member = vec![load(2699.999875)];
    let mut translated = model.clone();
    translate_z(&mut translated, 2_f64.powi(43));
    let response = analyze(&mut translated);
    let reason = "実配置で求めた柱の材軸と解析材軸の一致を確認できません";
    assert!(matches!(saved_column_face(&response, 1), Err(detail) if detail.contains(reason)));
    let rows = check(
        &translated,
        &response,
        RcColumnLongTermShearInput::SingleCase,
    );
    assert!(rows.iter().any(|(_, outcome)|
        matches!(outcome, CheckOutcome::Skipped { reason: detail } if detail.contains(reason))), "{rows:?}");
    assert!(rows
        .iter()
        .any(|(xi, outcome)| *xi == 0.5 && matches!(outcome, CheckOutcome::Checked(_))));

    let mut n = Vec::new();
    for a in [2699.999875, 2700.000125] {
        model.load_cases[0].member = vec![load(a)];
        let response = analyze(&mut model);
        let face = saved_column_face(&response, 1).as_ref().unwrap();
        assert_close(face.face.position_mm, 2700.0, 1e-9);
        n.push(*face.forces.as_ref().unwrap()[0].as_ref().unwrap());
    }
    assert_close(n[0] - n[1], 30_000.0, 0.2);

    model.elements[0].end_offsets_mm[1][2] = 0.0;
    model.load_cases[0].member = vec![load(2700.0)];
    for shift_mm in [0.0, 1000.0] {
        let mut model = model.clone();
        translate_z(&mut model, shift_mm);
        let mut known_axial = Vec::new();
        for beam_offset_mm in [0.0, 1e-14, 0.01] {
            model.elements[1].end_offsets_mm = [[0.0, 0.0, beam_offset_mm]; 2];
            let response = analyze(&mut model);
            let face = saved_column_face(&response, 1).as_ref().unwrap();
            let forces = face.forces.as_ref().unwrap();
            if beam_offset_mm == 1e-14 {
                assert_eq!(face.face.position_mm, 2700.0);
                assert!(matches!(&forces[0], Err(reason) if reason.contains("丸め誤差")));
                let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
                assert!(rows.iter().any(|(xi, outcome)| *xi == 0.9 && matches!(
                    outcome, CheckOutcome::Skipped { reason } if reason.contains("軸力N") && reason.contains("丸め誤差")
                )), "{rows:?}");
            } else {
                known_axial.push(*forces[0].as_ref().unwrap());
            }
            assert!(forces[1..].iter().all(Result::is_ok), "{forces:?}");
        }
        assert_close(known_axial[1] - known_axial[0], 30_000.0, 0.2);
    }
}

#[test]
fn 短期地震時の実面で同位置の重力せん断を使い別経路の同位置試料で代用しない() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [20_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    model.load_cases[0].member = vec![point(225.0, 12_000.0)];
    model.load_cases[3].name = "試験用水平力".into();
    model.load_cases[3].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [80_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    let combo = sepika_core::model::LoadCombination {
        name: "DL+EX".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(3), 1.0)],
    };
    sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
    let analysis = Analysis::prepare(&model).unwrap();
    let mut gravity = analysis.linear_static(LoadCaseId(0)).unwrap();
    let response = analysis.linear_combination(&combo).unwrap();
    gravity
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .1
        .at
        .push((0.05, [0.0, -20_000.0, 0.0, 0.0, 0.0, 0.0]));
    let run = |gravity_faces: &[_], gravity_error: Option<&str>, ordinary: bool| {
        run_member_design_checks(
            &model,
            &response.member_forces,
            &response.panel_moments,
            &MemberDesignCheckOptions {
                term: sepika_design_jp::LoadTerm::Short,
                qd_method: sepika_design_jp::QdMethod::Qd2,
                long_member_forces: gravity_error.is_none().then_some(if ordinary {
                    gravity.member_forces.as_slice()
                } else {
                    &[]
                }),
                rc_column_face_forces: &response.rc_column_face_forces,
                rc_column_gravity_face_forces: gravity_faces,
                rc_column_gravity_error: gravity_error,
                ..Default::default()
            },
        )
        .unwrap()
        .member_checks
        .into_iter()
        .filter(|(id, xi, _)| *id == COLUMN && *xi == 0.05)
        .map(|(_, _, outcome)| outcome)
        .collect::<Vec<_>>()
    };
    for ordinary in [true, false] {
        let rows = run(&gravity.rc_column_face_forces, None, ordinary);
        let component = shear(&rows[0]);
        assert_close(component.ratio * displayed_qay(component), 152_000.0, 0.2);
        let rows = run(&[], None, ordinary);
        assert!(
            rows.iter()
                .any(|outcome| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("同位置の重力参照内力が不足"))),
            "{rows:?}"
        );
        assert!(rows.iter().any(|outcome| matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0))));
    }
    let rows = run(&[], Some("重力再計算が失敗しました"), false);
    assert!(
        rows.iter()
            .any(|outcome| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("重力再計算が失敗しました"))),
        "{rows:?}"
    );
    assert!(rows.iter().any(|outcome| matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0)
        && result.components.iter().all(|part| part.kind != CheckKind::Shear))));
}

fn displayed_qay(component: &CheckComponent) -> f64 {
    component
        .detail
        .split("QAy=")
        .nth(1)
        .unwrap()
        .split(" N")
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn 丸め帯の集中荷重で不明な成分だけ保留して独立な短期qd2不適合を残す() {
    for (axis, ambiguous) in [([0.0, 0.0, 1.0], &[0][..]), (X, &[1, 5][..])] {
        let mut model = model(false, false);
        model.elements[1].local_axis.ref_vector = Y;
        model.load_cases[0].nodal = vec![NodalLoad::manual(
            NodeId(1),
            [20_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )];
        model.load_cases[0].member = vec![MemberLoad::manual(
            COLUMN,
            axis,
            MemberLoadKind::Point {
                a: 150.0 + 1e-11,
                p: 50_000.0,
            },
        )];
        model.load_cases[3].name = "試験用水平力".into();
        model.load_cases[3].nodal = vec![NodalLoad::manual(
            NodeId(1),
            [200_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )];
        let combo = sepika_core::model::LoadCombination {
            name: "DL+EX".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(3), 1.0)],
        };
        sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
        let analysis = Analysis::prepare(&model).unwrap();
        let gravity = analysis.linear_static(LoadCaseId(0)).unwrap();
        let response = analysis.linear_combination(&combo).unwrap();
        for solved in [&gravity, &response] {
            let forces = solved
                .rc_column_face_forces
                .iter()
                .find(|(id, _)| *id == COLUMN)
                .unwrap()
                .1[0]
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .forces
                .as_ref()
                .unwrap();
            for (index, value) in forces.iter().enumerate() {
                if ambiguous.contains(&index) {
                    assert!(
                        matches!(value, Err(reason) if reason.contains("丸め誤差")),
                        "{value:?}"
                    );
                } else {
                    assert!(matches!(value, Ok(value) if value.is_finite()), "{value:?}");
                }
            }
        }
        let report = run_member_design_checks(
            &model,
            &response.member_forces,
            &response.panel_moments,
            &MemberDesignCheckOptions {
                term: sepika_design_jp::LoadTerm::Short,
                qd_method: sepika_design_jp::QdMethod::Qd2,
                long_member_forces: Some(&[]),
                rc_column_face_forces: &response.rc_column_face_forces,
                rc_column_gravity_face_forces: &gravity.rc_column_face_forces,
                ..Default::default()
            },
        )
        .unwrap();
        let rows = report
            .member_checks
            .iter()
            .filter(|(id, xi, _)| *id == COLUMN && *xi == 0.05)
            .map(|(_, _, row)| row)
            .collect::<Vec<_>>();
        let component_label = if ambiguous.contains(&0) {
            "軸力N"
        } else {
            "せん断Qy"
        };
        assert!(rows.iter().any(|row| matches!(row, CheckOutcome::Skipped { reason } if reason.contains(component_label) && reason.contains("丸め誤差"))), "{rows:?}");
        let failure = rows.iter().find_map(|row| match row {
            CheckOutcome::Checked(result) => result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Shear && c.ratio > 1.0),
            _ => None,
        });
        if ambiguous.contains(&0) {
            let component = failure.unwrap_or_else(|| panic!("{rows:?}"));
            assert_close(component.ratio * displayed_qay(component), 320_000.0, 0.2);
        } else {
            assert!(failure.is_none(), "{rows:?}");
        }
    }
}

#[test]
fn 長期単独の実面で片方の曲げが未算定でも直交曲げの不適合を残す() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].member = vec![point(150.0 + 1e-11, 50_000.0)];
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, 0.0, 150_000_000.0, 0.0, 0.0],
    )];
    let response = analyze(&mut model);
    let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.05 && matches!(
        outcome, CheckOutcome::Skipped { reason } if reason.contains("曲げMz") && reason.contains("丸め誤差")
    )), "{rows:?}");
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.05 && matches!(
        outcome, CheckOutcome::Checked(result) if result.components.iter().any(|part|
            part.kind == CheckKind::AxialBending && part.ratio.is_finite() && part.ratio > 1.0)
    )), "{rows:?}");
}

#[test]
fn 実面でqd1またはminを選んだ場合は検証済みの汎用柱端軸力を要する() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].member.clear();
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    let solved = analyze(&mut model);
    let mut incomplete_gravity = solved.member_forces.clone();
    let (_, column) = incomplete_gravity
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    column.at.retain(|(pos, _)| *pos != 1.0);
    let mut incomplete_response = solved.member_forces.clone();
    let (_, column) = incomplete_response
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    column.at.iter_mut().find(|(pos, _)| *pos == 0.0).unwrap().1[0] = f64::NAN;
    for method in [QdMethod::Qd1, QdMethod::Min] {
        for (regular, gravity, reason) in [
            (
                solved.member_forces.as_slice(),
                &[][..],
                "柱端の重力参照軸力",
            ),
            (
                solved.member_forces.as_slice(),
                incomplete_gravity.as_slice(),
                "柱端の重力参照軸力",
            ),
            (
                incomplete_response.as_slice(),
                solved.member_forces.as_slice(),
                "柱端の応答軸力",
            ),
        ] {
            let rows = check_short_face(&model, &solved, method, regular, Some(gravity));
            assert!(
                rows.iter().any(
                    |outcome| matches!(outcome, CheckOutcome::Skipped { reason: detail }
                if detail.contains(reason))
                ),
                "{rows:?}"
            );
            assert!(rows.iter().any(|outcome| matches!(outcome, CheckOutcome::Checked(result)
                if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0)
                && result.components.iter().all(|part| part.kind != CheckKind::Shear))), "{rows:?}");
        }
        let rows = check_short_face(&model, &solved, method, &solved.member_forces, None);
        assert!(rows
            .iter()
            .any(|outcome| matches!(outcome, CheckOutcome::Checked(result)
            if result.components.iter().any(|part| part.kind == CheckKind::Shear))));
    }
}

fn check_short_face(
    model: &Model,
    solved: &StaticOnce,
    method: QdMethod,
    regular: &[(ElemId, MemberForces)],
    gravity: Option<&[(ElemId, MemberForces)]>,
) -> Vec<CheckOutcome> {
    run_member_design_checks(
        model,
        regular,
        &solved.panel_moments,
        &MemberDesignCheckOptions {
            term: sepika_design_jp::LoadTerm::Short,
            qd_method: method,
            long_member_forces: gravity,
            rc_column_face_forces: &solved.rc_column_face_forces,
            rc_column_gravity_face_forces: &solved.rc_column_face_forces,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .filter(|(id, xi, _)| *id == COLUMN && *xi == 0.05)
    .map(|(_, _, outcome)| outcome)
    .collect()
}

#[test]
fn 実面のqd1とminでは柱端軸力の重複順序によらず保留しqd2と曲げは判定する() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].member.clear();
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    let solved = analyze(&mut model);
    let original = &solved.member_forces;
    let column_index = original.iter().position(|(id, _)| *id == COLUMN).unwrap();
    for reverse in [false, true] {
        for end in [0.0, 1.0] {
            let mut repeated_end = original.clone();
            let stations = &mut repeated_end[column_index].1.at;
            let mut extra = *stations.iter().find(|(pos, _)| *pos == end).unwrap();
            extra.1[0] += 40_000.0;
            stations.push(extra);
            if reverse {
                stations.reverse();
            }
            let mut repeated_column = original.clone();
            let mut extra = original[column_index].clone();
            extra
                .1
                .at
                .iter_mut()
                .find(|(pos, _)| *pos == end)
                .unwrap()
                .1[0] += 40_000.0;
            repeated_column.push(extra);
            if reverse {
                repeated_column.reverse();
            }
            for (regular, gravity, source) in [
                (&repeated_end, original, "柱端の応答軸力"),
                (original, &repeated_end, "柱端の重力参照軸力"),
                (original, &repeated_column, "柱端の重力参照軸力"),
            ] {
                for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
                    let rows = check_short_face(&model, &solved, method, regular, Some(gravity));
                    assert!(rows.iter().any(|outcome| matches!(outcome, CheckOutcome::Checked(result)
                        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0)
                    )), "{rows:?}");
                    let shear = rows.iter().find_map(|outcome| match outcome {
                        CheckOutcome::Checked(result) => result
                            .components
                            .iter()
                            .find(|c| c.kind == CheckKind::Shear),
                        _ => None,
                    });
                    let repeated = rows.iter().any(|outcome| {
                        matches!(outcome, CheckOutcome::Skipped { reason }
                        if reason.contains(source) && reason.contains("重複"))
                    });
                    if method == QdMethod::Qd2 {
                        assert!(!repeated, "{rows:?}");
                        let component = shear.expect("同じ実材端面だけでQD2を算定できる");
                        assert_close(component.ratio * displayed_qay(component), 100_000.0, 0.2);
                    } else {
                        assert!(repeated && shear.is_none(), "{rows:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn 偏心で柱軸を外れても正面積の接触なら実面を定め線接触では定めない() {
    for (circle, tangent) in [(false, 350.0), (true, 450.0)] {
        let mut model = model(circle, false);
        for (offset, has_face) in [
            (tangent - 0.001, true),
            (tangent, false),
            (tangent + 0.001, false),
        ] {
            model.elements[1].end_offsets_mm = [[0.0, offset, 0.0]; 2];
            let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(&model);
            let geometry = sepika_core::face_distance::rc_column_girder_faces(
                &model,
                &model.elements[0],
                &adjacency,
            )
            .unwrap();
            let face = geometry.ends[0].as_ref().unwrap();
            if has_face {
                assert_eq!(face.as_ref().unwrap().position_mm, 300.0);
            } else {
                assert!(matches!(face, Err(reason) if reason.contains("正の面積")));
            }
        }
    }
}

#[test]
fn 回転した平面内で丸めにより正となる線接触を面接触にしない() {
    for (circle, offset) in [
        (false, [-148.0, -514.0, 0.0]),
        (true, [-177.0, -486.0, 0.0]),
    ] {
        let mut column = column_input::column_model(circle, 0.0);
        match column.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcColumnRect { b, d, .. } => {
                *b = 200.0;
                *d = 200.0;
            }
            SectionShape::RcColumnCircle { d, .. } => *d = 200.0,
            _ => unreachable!(),
        }
        let shape = SectionShape::RcBeamRect {
            b: 100.0,
            d: 600.0,
            rebar: sepika_core::section_shape::RcBeamRebar {
                main_dia: RebarSize::DiameterMm(19.0),
                top: Vec::new(),
                bottom: Vec::new(),
                cover: 40.0,
                stirrup: sepika_core::section_shape::BeamStirrup {
                    dia: RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let girder = Section {
            frame_use: Some(FrameSectionUse::Girder),
            material: Some(MaterialId(0)),
            ..shape.to_section(SectionId(1), "接触判定用の矩形大梁".into())
        };
        let mut model = input::model(column, girder, false);
        model.nodes[2].coord = [300.0, 400.0, 0.0];
        for (offset, contact) in [
            (offset, false),
            ([offset[0] - 0.008, offset[1] + 0.006, 0.0], true),
        ] {
            model.elements[1].end_offsets_mm = [offset; 2];
            let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(&model);
            let faces = sepika_core::face_distance::rc_column_girder_faces(
                &model,
                &model.elements[0],
                &adjacency,
            )
            .unwrap();
            let i = faces.ends[0].as_ref().unwrap();
            if contact {
                assert_eq!(i.as_ref().unwrap().position_mm, 300.0);
            } else {
                assert!(
                    matches!(i, Err(reason) if reason.contains("正の面積") && reason.contains("丸め誤差")),
                    "{i:?}"
                );
            }
        }
    }
}

#[test]
fn 平行移動しても柱軸方向の微小な隙間を真の接触と区別する() {
    for translate_mm in [0.0, 1000.0] {
        for (beam_z_mm, column_z_mm, expected_face_mm) in [
            (300.0, -1e-14, None),
            (300.0, 0.0, Some(600.0)),
            (-300.0, 1e-14, None),
            (-300.0, 0.0, Some(0.0)),
        ] {
            let mut model = model(false, false);
            translate_z(&mut model, translate_mm);
            model.elements[0].end_offsets_mm = [[0.0, 0.0, column_z_mm]; 2];
            model.elements[1].end_offsets_mm = [[0.0, 0.0, beam_z_mm]; 2];
            let solved = analyze(&mut model);
            assert_ordinary_foot(&model, &solved, expected_face_mm.ok_or("接触しません"));
        }
    }
}

#[test]
fn 接合側の大梁ハンチだけが実面の確定に影響する() {
    for reversed in [false, true] {
        let mut model = model(false, false);
        if reversed {
            model.elements[1].nodes.swap(0, 1);
        }
        for (side, increase, supported) in [
            (usize::from(reversed), 40.0, false),
            (usize::from(!reversed), 40.0, true),
            (usize::from(reversed), 0.0, true),
        ] {
            let mut detail = MemberDetailAttr::new(ElemId(1));
            let haunch = Some(sepika_core::model::Haunch {
                length: 750.0,
                depth_increase: 0.0,
                width_increase: increase,
                qd1_face: None,
            });
            if side == 0 {
                detail.haunch_i = haunch;
            } else {
                detail.haunch_j = haunch;
            }
            model.member_detail_attrs = vec![detail];
            let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(&model);
            let faces = sepika_core::face_distance::rc_column_girder_faces(
                &model,
                &model.elements[0],
                &adjacency,
            )
            .unwrap();
            let i = faces.ends[0].as_ref().unwrap();
            if supported {
                assert_eq!(i.as_ref().unwrap().position_mm, 300.0);
            } else {
                assert!(
                    matches!(i, Err(reason) if reason.contains("接合側にハンチ")),
                    "{i:?}"
                );
            }
        }
    }
}

#[test]
fn 汎用柱応答の重複で専用実面と構造規定の不適合を消さない() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    if let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape {
        rebar.hoop.pitch = 120.0;
    }
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [36_100.0, 0.0, 8472.9456, 0.0, 0.0, 0.0],
    )];
    model.load_cases[0].member.clear();
    let mut response = analyze(&mut model);
    let duplicate = response
        .member_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .clone();
    response.member_forces.push(duplicate);
    let rows = check_combination(&model, &response);
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.05 && matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::AxialBending && part.ratio > 1.0))), "{rows:?}");
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.5
        && matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("重複"))));
    assert_eq!(rows.iter().filter(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::Provision && part.ratio > 1.0))).count(), 1);
    assert!(rows.iter().all(
        |(_, outcome)| !matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|part| part.kind == CheckKind::Shear))
    ));
}

#[test]
fn 区間外の中央で構造規定が警告のみなら偽の未検定行を作らない() {
    let mut column = column_input::column_model(false, 0.0);
    if let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut column.sections[0].shape {
        rebar.main_dia = RebarSize::DiameterMm(13.0);
    }
    let mut model = input::model(
        column,
        beam_input::section(300.0, 3200.0, 19.0, &[3], &[3]),
        false,
    );
    model.load_cases[0].member.clear();
    model.load_cases[0].nodal.clear();
    let response = analyze(&mut model);
    let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
    assert_eq!(
        rows.iter().map(|(pos, _)| *pos).collect::<Vec<_>>(),
        [1600.0 / LENGTH_MM, 1.0]
    );
    assert!(
        rows.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if !result.components.is_empty() && result.ok())
        ),
        "{rows:?}"
    );
    assert_eq!(
        rows.iter()
            .filter(
                |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if result.detail.contains("主筋比 pg="))
            )
            .count(),
        1
    );
}

#[test]
fn 短期圧縮靱性は実面の軸力で比較し柱のハンチ範囲はその実面から測る() {
    use sepika_design_jp::RcColumnDuctilityComparison as Comparison;
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = Y;
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, -1_900_000.0 + 8472.9456, 0.0, 0.0, 0.0],
    )];
    model.load_cases[0].member = vec![MemberLoad::manual(
        COLUMN,
        [0.0, 0.0, 1.0],
        MemberLoadKind::Point {
            a: 225.0,
            p: -30_000.0,
        },
    )];
    let run = |model: &Model, response: &StaticOnce, use_faces: bool| {
        let report = run_member_design_checks(
            model,
            &response.member_forces,
            &response.panel_moments,
            &MemberDesignCheckOptions {
                term: sepika_design_jp::LoadTerm::Short,
                rc_column_face_forces: if use_faces {
                    &response.rc_column_face_forces
                } else {
                    &[]
                },
                ..Default::default()
            },
        )
        .unwrap();
        report
            .rc_column_ductility_advisories
            .into_iter()
            .find(|advisory| advisory.elem == COLUMN)
            .unwrap()
            .positions
    };
    let response = analyze(&mut model);
    let rows = run(&model, &response, true);
    assert!(
        matches!(rows[0], (0.05, Comparison::Compared { exceeds_standard: true, compression_n_per_mm2, .. })
        if (compression_n_per_mm2 - 1_930_000.0 / 240_000.0).abs() < 1e-10),
        "{rows:?}"
    );
    assert!(rows.iter().all(|(xi, _)| *xi != 0.1));
    assert_eq!(
        run(&model, &response, false)[0],
        (0.05, Comparison::Unavailable)
    );
    let mut detail = MemberDetailAttr::new(COLUMN);
    detail.haunch_i = Some(sepika_core::model::Haunch {
        length: 1300.0,
        depth_increase: 70.0,
        width_increase: 0.0,
        qd1_face: None,
    });
    model.member_detail_attrs.push(detail);
    let response = analyze(&mut model);
    let rows = run(&model, &response, true);
    assert_eq!(rows[0], (0.05, Comparison::Unavailable));
    assert!(
        rows.iter().any(|(xi, outcome)| *xi == 0.5
            && matches!(
                outcome,
                Comparison::Compared {
                    exceeds_standard: false,
                    ..
                }
            )),
        "{rows:?}"
    );
}

#[test]
fn 中央を越える柱脚面を切り詰めず構造規定の不適合を保持する() {
    let mut column = column_input::column_model(false, 0.0);
    if let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut column.sections[0].shape {
        rebar.hoop.pitch = 120.0;
    }
    let mut model = input::model(
        column,
        beam_input::section(300.0, 3200.0, 19.0, &[3], &[3]),
        false,
    );
    let response = analyze(&mut model);
    let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
    assert!(
        rows.iter().any(|(xi, outcome)| *xi == 1600.0 / LENGTH_MM
            && matches!(outcome, CheckOutcome::Checked(result)
            if result.components.iter().any(|part| part.kind == CheckKind::AxialBending))),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|(xi, outcome)| *xi == 0.5
            && matches!(outcome, CheckOutcome::Checked(result)
            if result.components.len() == 1 && result.components[0].kind == CheckKind::Provision
                && result.components[0].ratio > 1.0)),
        "{rows:?}"
    );
    assert!(rows.iter().all(|(xi, _)| *xi != 0.5 - 1e-9));
}

#[test]
fn 両端の大梁面が一致または逆転するときは柱区間を設けない() {
    for depth_mm in [3000.0, 3200.0] {
        let mut model = input::model(
            column_input::column_model(false, 0.0),
            beam_input::section(300.0, depth_mm, 19.0, &[3], &[3]),
            false,
        );
        let mut upper_end = model.nodes[2].clone();
        upper_end.id = NodeId(3);
        upper_end.coord[2] = LENGTH_MM;
        model.nodes.push(upper_end);
        let mut upper = model.elements[1].clone();
        upper.id = ElemId(2);
        upper.nodes = [NodeId(1), NodeId(3)].into_iter().collect();
        model.elements.push(upper);
        let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(&model);
        let geometry = sepika_core::face_distance::rc_column_girder_faces(
            &model,
            &model.elements[0],
            &adjacency,
        )
        .unwrap();
        assert!(!geometry.has_positive_span);
        assert!(geometry.ends.iter().all(|cut| matches!(
            cut, Some(Err(reason)) if reason.contains("正の長さの柱区間がありません")
        )));
    }
}

#[test]
fn 軸力曲げの検定位置を欠く場合も材端を復元してせん断を判定する() {
    for circle in [false, true] {
        for (w, demand_n) in [(0.0, 14_700.0), (3400.0, 183_900.0), (6000.0, 313_900.0)] {
            let mut model = model(circle, false);
            if w > 0.0 {
                model.load_cases[0].member.extend([
                    distributed(X, 1050.0, 1100.0, -w, -w),
                    distributed(X, 1100.0, 1150.0, w, w),
                ]);
            }
            let mut detail = MemberDetailAttr::new(COLUMN);
            detail.joints.push(MemberJoint {
                distance: 1200.0,
                kind: JointKind::Site,
            });
            model.member_detail_attrs.push(detail);
            let mut girder_tip = model.nodes[2].clone();
            girder_tip.id = NodeId(3);
            girder_tip.coord[2] = LENGTH_MM;
            girder_tip.restraint = sepika_core::dof::Dof6Mask::FREE;
            model.nodes.push(girder_tip);
            let mut upper_girder = model.elements[1].clone();
            upper_girder.id = ElemId(2);
            upper_girder.nodes = [NodeId(1), NodeId(3)].into_iter().collect();
            model.elements.push(upper_girder);
            let solved = analyze(&mut model);
            let zone = model.elements[0].rigid_zone;
            assert_eq!((zone.face_i, zone.face_j), (Some(300.0), Some(300.0)));
            let complete = check_combination(&model, &solved);
            let nominal = [0.1, 0.4, 0.5, 0.9];
            assert_eq!(
                complete.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
                nominal
            );
            for (_, outcome) in &complete {
                let component = shear(outcome);
                assert!(component
                    .detail
                    .contains(&format!("局所Qy最大 |Qy|={demand_n:.3} N")));
                assert_eq!(component.ratio > 1.0, w > 0.0);
            }
            for omitted in [nominal.as_slice(), &[0.5], &[0.4]] {
                let mut incomplete = solved.clone();
                let column = incomplete
                    .member_forces
                    .iter_mut()
                    .find(|(id, _)| *id == COLUMN)
                    .unwrap();
                assert_eq!(column.1.at.len(), 6);
                column.1.at.retain(|(xi, _)| !omitted.contains(xi));
                if let Some((_, ends)) = incomplete
                    .rc_column_face_forces
                    .iter_mut()
                    .find(|(id, _)| *id == COLUMN)
                {
                    for (side, pos) in [0.1, 0.9].into_iter().enumerate() {
                        if omitted.contains(&pos) {
                            ends[side] = None;
                        }
                    }
                }
                let rows = check_combination(&model, &incomplete);
                let y_peak_mm = if w == 0.0 { 300.0 } else { 1100.0 };
                let y_demand =
                    format!("局所Qy最大 |Qy|={demand_n:.3} N（i端から x={y_peak_mm:.6} mm");
                let z_demand = "局所Qz最大 |Qz|=0.000 N（i端から x=300.000000 mm";
                for target in omitted {
                    assert!(rows.iter().any(|(xi, outcome)| xi == target && matches!(
                        outcome, CheckOutcome::Skipped { reason } if
                            (reason.contains("軸力・曲げを検定する断面位置の解析内力がありません")
                                || reason.contains("実材端面の断面力がありません"))
                            && reason.contains(&y_demand)
                            && reason.contains(z_demand)
                    )), "省略位置={omitted:?} {rows:?}");
                }
                let expected = shear(&complete[0].1);
                let shear_rows = rows
                    .iter()
                    .filter_map(|(xi, outcome)| match outcome {
                        CheckOutcome::Checked(result) => result
                            .components
                            .iter()
                            .find(|component| component.kind == CheckKind::Shear)
                            .map(|component| (xi, component)),
                        CheckOutcome::Skipped { .. } => None,
                    })
                    .collect::<Vec<_>>();
                if omitted == nominal {
                    if w == 0.0 {
                        assert!(shear_rows.is_empty(), "{rows:?}");
                    } else {
                        let [(xi, component)] = shear_rows.as_slice() else {
                            panic!("{rows:?}");
                        };
                        assert_close(**xi * LENGTH_MM, 1100.0, 1e-8);
                        assert_eq!(*component, expected);
                    }
                } else {
                    assert_eq!(shear_rows.len(), nominal.len() - omitted.len());
                    assert!(shear_rows
                        .iter()
                        .all(|(_, component)| *component == expected));
                }
            }
        }
    }
}

#[test]
fn 片方向の材端が未確認なら他方向の確定検定と独立した曲げ不適合を保持する() {
    let mut model = model(false, true);
    model.load_cases[0]
        .member
        .push(distributed(Y, 800.0, 2800.0, 415.0, -415.0));
    model.load_cases[1]
        .member
        .push(distributed(Y, 800.0, 2800.0, -83.0, -83.0));
    let solved = analyze(&mut model);
    for (index, damage, direction, bound_x_mm, exact_alpha) in [
        (4, f64::INFINITY, "Qz", 1600.0, "αy=1.000, αz=未算定"),
        (5, 900_000_000.0, "Qy", 1100.0, "αy=未算定, αz=1.000"),
    ] {
        let mut incomplete = solved.clone();
        let (_, column) = incomplete
            .member_forces
            .iter_mut()
            .find(|(id, _)| *id == COLUMN)
            .unwrap();
        let (_, face) = column.at.iter_mut().find(|(xi, _)| *xi == 0.1).unwrap();
        face[index] = damage;
        let (_, mid) = column.at.iter_mut().find(|(xi, _)| *xi == 0.5).unwrap();
        mid[5] = f64::NAN;
        mid[4] = 520_000_000.0;
        let rows = check_combination(&model, &incomplete);
        let reason_part = if damage.is_finite() {
            "曲げ釣合い"
        } else {
            "曲げモーメントが非有限"
        };
        assert!(rows.iter().any(|(_, outcome)| matches!(outcome,
            CheckOutcome::Skipped { reason } if reason.contains(&format!("長期せん断（局所{direction}）"))
                && reason.contains(reason_part)
        )), "{rows:?}");
        let mut components = rows.iter().filter_map(|(xi, outcome)| match outcome {
            CheckOutcome::Checked(result) => Some((xi, result.components.as_slice())),
            CheckOutcome::Skipped { .. } => None,
        });
        assert!(
            components.clone().any(|(xi, checks)| {
                (*xi * LENGTH_MM - bound_x_mm).abs() < 1e-8
                    && checks.iter().any(|component| {
                        component.kind == CheckKind::Shear
                            && component.ratio > 1.0
                            && component.detail.contains("検定比の下限")
                            && component.detail.contains(&format!("局所{direction}最大"))
                    })
            }),
            "{rows:?}"
        );
        assert!(
            components
                .clone()
                .any(|(_, checks)| checks.iter().any(|component| {
                    component.kind == CheckKind::Shear
                        && component.ratio > 1.0
                        && component.detail.contains(exact_alpha)
                        && !component.detail.contains("検定比の下限")
                })),
            "{rows:?}"
        );
        assert!(
            components.any(|(xi, checks)| *xi == 0.5
                && checks.iter().any(|component| {
                    component.kind == CheckKind::AxialBending
                        && component.ratio > 1.0
                        && component.detail.contains("有限成分による検定比の下限")
                })),
            "{rows:?}"
        );
    }
}

#[test]
fn 矩形と円形で柱脚と柱頭の小さい方の割増係数を採用する() {
    for (circle, tip_moment, face_moment, expected_qa_n, alpha) in [
        (false, -145_000_000.0, 141_200_000.0, 136_820.250, "1.000"),
        (
            true,
            -145_000_000.0,
            141_200_000.0,
            158_695.110_485_7,
            "1.000",
        ),
        (
            false,
            -30_000_000.0,
            256_200_000.0,
            153_797.297_846,
            "1.124",
        ),
        (true, -30_000_000.0, 256_200_000.0, 161_429.296_698, "1.017"),
    ] {
        let mut model = alpha_model(circle, tip_moment);
        let solved = analyze(&mut model);
        let (_, column) = solved
            .member_forces
            .iter()
            .find(|(id, _)| *id == COLUMN)
            .unwrap();
        let face = |xi| column.at.iter().find(|(x, _)| *x == xi).unwrap().1;
        assert_close(face(0.1)[5], face_moment, 1e-2);
        assert_close(face(0.1)[1], -187_000.0, 1e-5);
        assert_close(face(1.0)[5], tip_moment, 1e-2);
        assert_close(face(1.0)[1], -25_000.0, 1e-5);
        let rows = check_combination(&model, &solved);
        assert_eq!(
            rows.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
            [0.1, 0.5, 1.0]
        );
        for (_, outcome) in rows {
            let CheckOutcome::Checked(ref result) = outcome else {
                panic!("{outcome:?}");
            };
            assert!(!result.ok(), "{outcome:?}");
            if tip_moment == -145_000_000.0 {
                assert!(result
                    .components
                    .iter()
                    .any(|c| c.kind == CheckKind::AxialBending && c.ratio < 1.0));
            }
            let component = shear(&outcome);
            assert_close(component.ratio, 187_000.0 / expected_qa_n, 1e-8);
            assert!(component
                .detail
                .contains(&format!("QAy={expected_qa_n:.1} N")));
            assert!(component.detail.contains(&format!("αy={alpha}, αz=1.000")));
            assert!(component
                .detail
                .contains("局所Qy最大 |Qy|=187000.000 N（i端から x=300.000000 mm"));
        }
    }
}

#[test]
fn 両方向が割増を要するとき対応する曲げと有効せいを使う() {
    let mut model = alpha_model(false, -30_000_000.0);
    let tip = &mut model.load_cases[0].nodal[0].values;
    tip[1] = 170_000.0;
    tip[3] = 225_000_000.0;
    let solved = analyze(&mut model);
    let (_, column) = solved
        .member_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    let face = |xi| column.at.iter().find(|(x, _)| *x == xi).unwrap().1;
    assert_close(face(0.1)[4], -234_000_000.0, 1e-2);
    assert_close(face(1.0)[4], 225_000_000.0, 1e-2);
    assert_close(face(0.1)[2], -170_000.0, 1e-5);
    assert_close(face(1.0)[2], -170_000.0, 1e-5);
    for (_, outcome) in check_combination(&model, &solved) {
        let component = shear(&outcome);
        assert_close(component.ratio, 170_000.0 / 128_580.375, 1e-8);
        assert!(component.detail.contains("QAy=153797.3 N, QAz=128580.4 N"));
        assert!(component.detail.contains("αy=1.124, αz=1.000"));
    }
}

#[test]
fn 回転大梁の未保存フェースと集中荷重の柱区間側の内力を使う() {
    for (rotated, expected_q_n, expected_qa_n, position) in [
        (false, 187_000.0, 153_797.297_846, "300.000000 mm（j端側）"),
        (true, 196_000.0, 147_330.145_705, "150.000000 mm"),
    ] {
        let mut model = alpha_model(false, -30_000_000.0);
        if rotated {
            model.elements[1].local_axis.ref_vector = Y;
        } else {
            model.load_cases[0].member.push(point(300.0, 50_000.0));
        }
        let solved = analyze(&mut model);
        let rows = check_combination(&model, &solved);
        assert_eq!(
            rows.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
            [if rotated { 0.05 } else { 0.1 }, 0.5, 1.0]
        );
        for (_, outcome) in rows {
            let component = shear(&outcome);
            assert_close(component.ratio, expected_q_n / expected_qa_n, 1e-8);
            assert!(
                component.detail.contains(&format!(
                    "局所Qy最大 |Qy|={expected_q_n:.3} N（i端から x={position}"
                )),
                "{}",
                component.detail
            );
        }
    }
}

#[test]
fn 割増係数が不足する方向を保留し他方向の検定比を理由に表示する() {
    let mut model = alpha_model(false, -145_000_000.0);
    model.load_cases[0]
        .member
        .push(distributed(Y, 800.0, 2800.0, 50.0, -50.0));
    model.load_cases[1]
        .member
        .push(distributed(Y, 800.0, 2800.0, -10.0, -10.0));
    let mut solved = analyze(&mut model);
    let (_, column) = solved
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    let (_, face) = column.at.iter_mut().find(|(xi, _)| *xi == 0.1).unwrap();
    face[5] = f64::NAN;
    face[4] = f64::NAN;
    let (_, faces) = solved
        .rc_column_face_forces
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    let saved = faces[0]
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .forces
        .as_mut()
        .unwrap();
    saved[4] = Err("曲げMyが非有限です".into());
    saved[5] = Err("曲げMzが非有限です".into());
    let rows = check_combination(&model, &solved);
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.1 && matches!(
        outcome, CheckOutcome::Skipped { reason } if reason.contains("曲げMzが非有限")
            && reason.contains("長期せん断（局所Qy）")
            && !reason.contains("長期せん断（局所Qz）")
            && reason.contains("αy=未算定, αz=1.000")
            && reason.contains("局所Qy最大 |Qy|=187000.000 N（i端から x=300.000000 mm）")
            && reason.contains("局所Qz最大 |Qz|=36000.000 N（i端から x=1600.000000 mm、検定比 ")
    )), "{rows:?}");
    assert!(!rows.iter().any(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if result.ok() || result.components.iter().any(|component| component.kind == CheckKind::Shear))));
}

#[test]
fn 割増なしで需要を満たせば両方向の材端曲げモーメントを要しない() {
    let mut model = model(false, false);
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(1),
        [0.0, 2000.0, 0.0, 0.0, 0.0, 0.0],
    ));
    let mut solved = analyze(&mut model);
    let (_, column) = solved
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    column.at[0].1[4] = f64::NAN;
    column.at[0].1[5] = f64::INFINITY;
    let rows = check_combination(&model, &solved);
    for (_, outcome) in rows {
        let component = shear(&outcome);
        assert!(component.detail.contains("αy=1.000, αz=1.000"));
        assert!(component.detail.contains("局所Qy最大 |Qy|=14700.000 N"));
        assert!(component.detail.contains("局所Qz最大 |Qz|=2000.000 N"));
        assert!(component.ratio < 1.0);
    }
}

#[test]
fn 材中央の非有限な軸力や曲げは実フェースのせん断判定に影響しない() {
    let mut model = alpha_model(false, -145_000_000.0);
    let solved = analyze(&mut model);
    for index in [0, 4, 5] {
        let mut incomplete = solved.clone();
        let (_, column) = incomplete
            .member_forces
            .iter_mut()
            .find(|(id, _)| *id == COLUMN)
            .unwrap();
        let (_, mid) = column.at.iter_mut().find(|(xi, _)| *xi == 0.5).unwrap();
        mid[index] = f64::NAN;
        let rows = check_combination(&model, &incomplete);
        assert!(
            rows.iter().any(|(xi, outcome)| *xi == 0.5
                && matches!(
                    outcome, CheckOutcome::Skipped { reason } if reason.contains("非有限")
                        && !reason.contains("長期せん断（局所")
                )),
            "{rows:?}"
        );
        assert!(rows.iter().any(|(xi, outcome)| *xi == 0.5 && matches!(
            outcome, CheckOutcome::Checked(result) if result.components.iter().any(|component| {
                component.kind == CheckKind::Shear
                    && component.ratio > 1.0
                    && component.detail.contains("αy=1.000, αz=1.000")
                    && !component.detail.contains("検定比の下限")
            })
        )), "{rows:?}");
    }
}

#[test]
fn 釣合いを満たす内力系列が重複しても他の柱の長期せん断検定を継続する() {
    let mut model = model(false, true);
    let mut other_tip = model.nodes[1].clone();
    other_tip.id = NodeId(3);
    other_tip.coord[0] = 3000.0;
    model.nodes.push(other_tip);
    let mut other = model.elements[0].clone();
    other.id = ElemId(2);
    other.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    model.elements.push(other);
    let mut solved = analyze(&mut model);
    let source = RcColumnLongTermShearInput::SavedCombination(&model.combinations[0].name);
    let original = check_report(&model, &solved, source);
    let mut duplicate = solved
        .member_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .clone();
    for (_, values) in &mut duplicate.1.at {
        values[1] += 500_000.0;
    }
    solved.member_forces.push(duplicate);
    let report = check_report(&model, &solved, source);
    let target: Vec<_> = report
        .member_checks
        .iter()
        .filter(|(id, _, _)| *id == COLUMN)
        .collect();
    assert_eq!(target.len(), 3);
    assert!(
        target.iter().all(
            |(_, _, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("解析内力が重複し、一意の断面力を確認できません"))
        ),
        "{target:?}"
    );
    let all_other_columns = |(id, _, _): &&(ElemId, f64, CheckOutcome)| *id == ElemId(2);
    assert_eq!(
        original
            .member_checks
            .iter()
            .filter(all_other_columns)
            .count(),
        3
    );
    assert!(original.member_checks.iter().filter(all_other_columns).all(|(_, _, outcome)| {
        matches!(outcome, CheckOutcome::Checked(result) if result.components.iter().any(|component| component.kind == CheckKind::Shear))
    }));
    assert!(report
        .member_checks
        .iter()
        .filter(all_other_columns)
        .eq(original.member_checks.iter().filter(all_other_columns)));
}

#[test]
fn 矩形と円形の既定断面位置で部材区間の最大せん断力を照査する() {
    for circle in [false, true] {
        for inner_peak in [false, true] {
            let mut model = model(circle, inner_peak);
            let solved = analyze(&mut model);
            let zone = model.elements[0].rigid_zone;
            assert_eq!(
                (zone.face_i, zone.face_j, zone.rigid_length_i()),
                (Some(300.0), Some(0.0), 150.0)
            );
            let forces = &solved
                .member_forces
                .iter()
                .find(|(id, _)| *id == COLUMN)
                .unwrap()
                .1
                .at;
            assert_eq!(
                forces.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
                [0.0, 0.1, 0.5, 1.0]
            );
            for &(xi, values) in forces {
                let above_mm = LENGTH_MM * (1.0 - xi);
                let expected_mz = 12_000.0 * above_mm
                    + above_mm.powi(2) / 2.0
                    + if inner_peak && xi <= 0.1 {
                        15_000_000.0
                    } else {
                        0.0
                    };
                assert_close(values[1], -12_000.0 - above_mm, 1e-5);
                assert_close(values[5], expected_mz, 1e-2);
            }
            let rows = check_combination(&model, &solved);
            assert_eq!(
                rows.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
                [0.1, 0.5, 1.0]
            );
            for (xi, outcome) in rows {
                let component = shear(&outcome);
                let (demand, location) = if inner_peak {
                    (313_900.0, 1100.0)
                } else {
                    (14_700.0, 300.0)
                };
                let capacity = if circle { 160_393.0 } else { 138_098.0 };
                assert!(component.detail.contains(&format!(
                    "局所Qy最大 |Qy|={demand:.3} N（i端から x={location:.6} mm"
                )));
                assert!(component
                    .detail
                    .contains("局所Qz最大 |Qz|=0.000 N（i端から x=300.000000 mm"));
                assert!(component.detail.contains("αy=1.000, αz=1.000"));
                assert_close(component.ratio, demand / capacity, 1e-5);
                let CheckOutcome::Checked(result) = &outcome else {
                    unreachable!()
                };
                assert_eq!(result.ok(), !inner_peak);
                let (_, station) = forces.iter().find(|(pos, _)| *pos == xi).unwrap();
                let axial_bending = result
                    .components
                    .iter()
                    .find(|c| c.kind == CheckKind::AxialBending)
                    .unwrap();
                assert!(axial_bending
                    .detail
                    .contains(&format!("N={:.1} N", -station[0])));
                assert!(axial_bending.detail.contains(&format!(
                    "mz={:.1} N·mm, my={:.1} N·mm",
                    station[5], station[4]
                )));
                println!(
                    "円形={circle} 内点ピーク={inner_peak} 検定位置={xi} 合否={} {}",
                    result.ok(),
                    component.detail
                );
            }
        }
    }
}

#[test]
fn 合成した台形荷重の強度零点と別方向の荷重切替点でそれぞれ最大になる() {
    for (scale, expected_qz, governing_ratio) in [
        (1.0, 36_000.0, 313_900.0 / 138_098.0),
        (8.3, 298_800.0, 298_800.0 / 130_496.6),
    ] {
        let mut model = model(false, true);
        let dead = model.load_cases[0].id;
        let live = model.load_cases[1].id;
        model.combinations[0].terms = vec![(dead, 0.25), (live, 1.0), (dead, 0.75)];
        model.load_cases[0]
            .member
            .push(distributed(Y, 800.0, 2800.0, 50.0 * scale, -50.0 * scale));
        model.load_cases[1].member.push(distributed(
            Y,
            800.0,
            2800.0,
            -10.0 * scale,
            -10.0 * scale,
        ));
        let solved = analyze(&mut model);
        for (_, outcome) in check_combination(&model, &solved) {
            let component = shear(&outcome);
            assert!(component
                .detail
                .contains("局所Qy最大 |Qy|=313900.000 N（i端から x=1100.000000 mm"));
            // x=1600 mm から分布の終点までは高さ -60×scale N/mm、幅 1200 mm の三角形。
            assert!(component.detail.contains(&format!(
                "局所Qz最大 |Qz|={expected_qz:.3} N（i端から x=1600.000000 mm"
            )));
            assert_close(component.ratio, governing_ratio, 1e-5);
            println!("{}", component.detail);
        }
    }
}

#[test]
fn 回転した大梁断面の実フェースを柱区間の境界にする() {
    for inner_peak in [false, true] {
        let mut model = model(false, false);
        model.elements[1].local_axis.ref_vector = Y;
        if inner_peak {
            model.load_cases[0].member.extend([
                distributed(X, 175.0, 225.0, -6000.0, -6000.0),
                distributed(X, 225.0, 275.0, 6000.0, 6000.0),
            ]);
        }
        let solved = analyze(&mut model);
        assert_eq!(model.elements[0].rigid_zone.face_i, Some(300.0));
        let rows = check_combination(&model, &solved);
        assert_eq!(
            rows.iter().map(|(xi, _)| *xi).collect::<Vec<_>>(),
            [0.05, 0.5, 1.0]
        );
        let expected = if inner_peak {
            "局所Qy最大 |Qy|=314775.000 N（i端から x=225.000000 mm"
        } else {
            "局所Qy最大 |Qy|=14850.000 N（i端から x=150.000000 mm"
        };
        for (_, outcome) in rows {
            let component = shear(&outcome);
            assert!(component.detail.contains(expected), "{}", component.detail);
            assert_eq!(component.ratio > 1.0, inner_peak);
            println!("{}", component.detail);
        }
    }
}

#[test]
fn 斜めの梁断面で材端フェースを特定できない場合は未算定になる() {
    let mut model = model(false, false);
    model.elements[1].local_axis.ref_vector = [0.0, 1.0, 1.0];
    let solved = analyze(&mut model);
    assert!(check_combination(&model, &solved).iter().any(|(_, outcome)| matches!(
        outcome, CheckOutcome::Skipped { reason } if reason.contains("接合境界を一つの断面位置で表せません")
    )));
}

#[test]
fn 接続大梁ごとの材端フェースが異なる場合は区間を確定しない() {
    for unequal in [false, true] {
        let mut model = model(false, false);
        model.load_cases[0].member.extend([
            distributed(X, 175.0, 225.0, -6000.0, -6000.0),
            distributed(X, 225.0, 275.0, 6000.0, 6000.0),
        ]);
        let mut end = model.nodes[2].clone();
        end.id = NodeId(3);
        end.coord = [-3000.0, 0.0, 0.0];
        model.nodes.push(end);
        let mut girder = model.elements[1].clone();
        girder.id = ElemId(2);
        girder.nodes = [NodeId(0), NodeId(3)].into_iter().collect();
        if unequal {
            girder.local_axis.ref_vector = Y;
        }
        model.elements.push(girder);
        let solved = analyze(&mut model);
        assert_eq!(model.elements[0].rigid_zone.face_i, Some(300.0));
        let rows = check_combination(&model, &solved);
        assert_eq!(rows.len(), 3);
        for (_, outcome) in rows {
            if unequal {
                assert!(
                    matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("接続大梁によって材端フェースの位置が異なります"))
                );
            } else {
                let component = shear(&outcome);
                assert!(component
                    .detail
                    .contains("局所Qy最大 |Qy|=14700.000 N（i端から x=300.000000 mm"));
                println!("{}", component.detail);
            }
        }
    }
}

#[test]
fn 大梁面が丸めで同じmm表示になる場合も共通面とみなさない() {
    for translate_z_mm in [0.0, 1000.0] {
        for beam_offset_mm in [0.0, 25.0, 1e-14] {
            let mut model = model(false, false);
            let mut end = model.nodes[2].clone();
            end.id = NodeId(3);
            end.coord = [-3000.0, 0.0, 0.0];
            model.nodes.push(end);
            let mut girder = model.elements[1].clone();
            girder.id = ElemId(2);
            girder.nodes = [NodeId(3), NodeId(0)].into_iter().collect();
            girder.end_offsets_mm = [[0.0, 0.0, beam_offset_mm]; 2];
            model.elements.push(girder);
            translate_z(&mut model, translate_z_mm);
            let solved = analyze(&mut model);
            let direction = if beam_offset_mm == 0.0 {
                Ok(300.0)
            } else {
                Err("接続大梁によって材端フェースの位置が異なります")
            };
            assert_ordinary_foot(&model, &solved, direction);
        }
    }
}

#[test]
fn 傾斜柱でも反対側の梁が同じ法線入力と半厚なら共通面を採用する() {
    for opposite in [false, true] {
        let mut model = model(false, false);
        model.nodes[1].coord = [0.0, 3000.0, 4000.0];
        model.stories[1].elevation = Some(4000.0);
        model.elements[1].local_axis.ref_vector = [0.0, 3.0, 4.0];
        if opposite {
            let mut end = model.nodes[2].clone();
            end.id = NodeId(3);
            end.coord[0] = -3000.0;
            model.nodes.push(end);
            let mut beam = model.elements[1].clone();
            beam.id = ElemId(2);
            beam.nodes = [NodeId(0), NodeId(3)].into_iter().collect();
            model.elements.push(beam);
        }
        let response = analyze(&mut model);
        let face = saved_column_face(&response, 0).as_ref().unwrap();
        assert_eq!(face.face.length_mm, 5000.0);
        assert_ordinary_foot(&model, &response, Ok(300.0));
    }
}

#[test]
fn 傾斜柱の共通面は法線への寄与がゼロと分かる梁の偏心では変わらない() {
    for (offset_mm, expected) in [
        ([0.0; 3], Ok(300.0)),
        ([100.0, 0.0, 0.0], Ok(300.0)),
        (
            [100.0, 1e-14, 0.0],
            Err("材端フェースが同じ位置にあることを丸め誤差の範囲で確認できません"),
        ),
    ] {
        let mut model = model(false, false);
        model.nodes[1].coord = [0.0, 3000.0, 4000.0];
        model.stories[1].elevation = Some(4000.0);
        model.elements[1].local_axis.ref_vector = [0.0, 3.0, 4.0];
        let mut end = model.nodes[2].clone();
        end.id = NodeId(3);
        end.coord[0] = -3000.0;
        model.nodes.push(end);
        let mut beam = model.elements[1].clone();
        beam.id = ElemId(2);
        beam.nodes = [NodeId(0), NodeId(3)].into_iter().collect();
        beam.end_offsets_mm = [offset_mm; 2];
        model.elements.push(beam);
        let response = analyze(&mut model);
        assert_ordinary_foot(&model, &response, expected);
    }
}

#[test]
fn 傾斜柱でも正確な柱頭面と一致する集中荷重は柱区間側の切断力に入る() {
    let mut model = model(false, false);
    model.load_cases[0].nodal.clear();
    model.nodes[1].coord = [0.0, 3000.0, 4000.0];
    model.nodes[2].coord = [3000.0, 3000.0, 4000.0];
    model.nodes[2].restraint = sepika_core::dof::Dof6Mask::FREE;
    model.stories[1].elevation = Some(4000.0);
    model.elements[1].nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    model.elements[1].local_axis.ref_vector = [0.0, 3.0, 4.0];
    let mut known_axial = Vec::new();
    for a in [4699.99, 4700.0, 4700.0 + 1e-11] {
        model.load_cases[0].member = vec![MemberLoad::manual(
            COLUMN,
            [0.0, 0.6, 0.8],
            MemberLoadKind::Point { a, p: -30_000.0 },
        )];
        let response = analyze(&mut model);
        let face = saved_column_face(&response, 1).as_ref().unwrap();
        assert_eq!(face.face.length_mm, 5000.0);
        assert_eq!(face.face.position_mm, 4700.0);
        let forces = face.forces.as_ref().unwrap();
        let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
        if a > 4700.0 {
            assert!(matches!(&forces[0], Err(reason) if reason.contains("丸め誤差")));
            assert!(rows.iter().any(|(xi, outcome)| *xi == 0.94 && matches!(
                outcome, CheckOutcome::Skipped { reason } if reason.contains("軸力N") && reason.contains("丸め誤差")
            )), "{rows:?}");
        } else {
            known_axial.push(*forces[0].as_ref().unwrap());
            assert!(rows.iter().any(|(xi, _)| *xi == 0.94), "{rows:?}");
            assert!(rows.iter().all(|(xi, outcome)| *xi != 0.94 || !matches!(
                outcome, CheckOutcome::Skipped { reason } if reason.contains("軸力N") && reason.contains("丸め誤差")
            )), "{rows:?}");
        }
    }
    assert_close(known_axial[0] - known_axial[1], 30_000.0, 0.2);
}

#[test]
fn 梁軸が直交でも断面を微小回転させると実面を確定しない() {
    for (reference, expected) in [
        ([0.0, 0.0, 1.0], Ok(300.0)),
        ([0.0, 1e-15, 1.0], Err("断面の外面が柱軸に直交しない")),
        ([0.0, 1.0, 0.0], Ok(150.0)),
        ([0.0, 1.0, 1e-15], Err("断面の外面が柱軸に直交しない")),
    ] {
        let mut model = model(false, false);
        model.elements[1].local_axis.ref_vector = reference;
        let response = analyze(&mut model);
        assert_ordinary_foot(&model, &response, expected);
    }
}

#[test]
fn 傾斜柱の直交と微小斜交は柱と梁に共通する偏心で変わらない() {
    let reason = "断面の外面が柱軸に直交しない";
    for common_x_offset_mm in [0.0, 100.0] {
        for (reference, expected) in [
            ([0.0, 3.0, 4.0], Ok(300.0)),
            ([0.0, 3.0, 4.00000000000001], Err(reason)),
            ([0.0, 3.0000000000000004, 4.000000000000001], Err(reason)),
        ] {
            let mut model = model(false, false);
            model.nodes[1].coord = [0.0, 3000.0, 4000.0];
            model.stories[1].elevation = Some(4000.0);
            for element in &mut model.elements {
                element.end_offsets_mm = [[common_x_offset_mm, 0.0, 0.0]; 2];
            }
            model.elements[1].local_axis.ref_vector = reference;
            model.load_cases[0].member = vec![MemberLoad::manual(
                COLUMN,
                [0.0, 0.6, 0.8],
                MemberLoadKind::Point {
                    a: 300.0,
                    p: -30_000.0,
                },
            )];
            let response = analyze(&mut model);
            assert_ordinary_foot(&model, &response, expected);
            if expected.is_ok() {
                let face = saved_column_face(&response, 0).as_ref().unwrap();
                assert!(face.face.position_exact);
                assert!(face.forces.as_ref().unwrap()[0].is_ok());
            }
        }
    }
}

#[test]
fn 傾斜柱の材軸方向を確認できない端では表示座標が同じでも軸力を保留する() {
    let mut model = model(false, false);
    model.nodes[1].coord = [0.0, 3000.0, 4000.0];
    model.stories[1].elevation = Some(4000.0);
    model.elements[0].end_offsets_mm[1][1] = 1e-14;
    model.elements[1].local_axis.ref_vector = [0.0, 3.0, 4.0];
    model.load_cases[0].member = vec![MemberLoad::manual(
        COLUMN,
        [0.0, 0.6, 0.8],
        MemberLoadKind::Point {
            a: 300.0,
            p: -30_000.0,
        },
    )];
    let response = analyze(&mut model);
    let face = saved_column_face(&response, 0).as_ref().unwrap();
    assert_eq!(face.face.position_mm, 300.0);
    let forces = face.forces.as_ref().unwrap();
    assert!(matches!(&forces[0], Err(reason) if reason.contains("丸め誤差")));
    assert!(forces[1..].iter().all(Result::is_ok));
    let rows = check(&model, &response, RcColumnLongTermShearInput::SingleCase);
    assert!(rows.iter().any(|(xi, outcome)| *xi == 0.06 && matches!(
        outcome, CheckOutcome::Skipped { reason } if reason.contains("軸力N") && reason.contains("丸め誤差")
    )), "{rows:?}");
}

#[test]
fn 平面内で斜めの梁でも参照の材軸方向成分は面上の荷重の切断側を変えない() {
    let reason = "断面の外面が柱軸に直交しない";
    for (reference, expected) in [
        ([0.0, 0.0, 1.0], Ok(300.0)),
        ([3.0, 4.0, 1.0], Ok(300.0)),
        ([3.0, 4.000000000000001, 1.0], Err(reason)),
        ([3.0000000000000004, 4.000000000000001, 1.0], Err(reason)),
    ] {
        let mut model = model(false, false);
        model.nodes[2].coord = [3000.0, 4000.0, 0.0];
        model.elements[1].local_axis.ref_vector = reference;
        model.load_cases[0].member = vec![MemberLoad::manual(
            COLUMN,
            [0.0, 0.0, 1.0],
            MemberLoadKind::Point {
                a: 300.0,
                p: -30_000.0,
            },
        )];
        let response = analyze(&mut model);
        assert_ordinary_foot(&model, &response, expected);
        if expected.is_ok() {
            let face = saved_column_face(&response, 0).as_ref().unwrap();
            let axial_n = *face.forces.as_ref().unwrap()[0].as_ref().unwrap();
            assert_close(axial_n, -8_472.946, 0.2);
        }
    }
}

#[test]
fn 梁遠端だけが柱軸方向に微小偏心する場合は直交面とみなさない() {
    for z_shift_mm in [0.0, 1000.0] {
        for offset_mm in [0.0, 1e-12] {
            let mut model = model(false, false);
            translate_z(&mut model, z_shift_mm);
            model.elements[1].end_offsets_mm[1][2] = offset_mm;
            let response = analyze(&mut model);
            let expected = if offset_mm == 0.0 {
                Ok(300.0)
            } else {
                Err("大梁 ID 1 の材軸が柱軸に直交しない")
            };
            assert_ordinary_foot(&model, &response, expected);
        }
    }
}

#[test]
fn 同軸でも柱区間と重なる接続柱がある場合は区間を確定しない() {
    for overlaps in [false, true] {
        let mut model = model(false, false);
        let mut end = model.nodes[0].clone();
        end.id = NodeId(3);
        end.coord = [0.0, 0.0, if overlaps { 1500.0 } else { -3000.0 }];
        model.nodes.push(end);
        let mut column = model.elements[0].clone();
        column.id = ElemId(2);
        column.nodes = [NodeId(0), NodeId(3)].into_iter().collect();
        model.elements.push(column);
        let solved = analyze(&mut model);
        let rows = check_combination(&model, &solved);
        assert_eq!(rows.len(), 3);
        for (_, outcome) in rows {
            if overlaps {
                assert!(
                    matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("柱の材軸が連続しない接合"))
                );
            } else {
                let component = shear(&outcome);
                assert!(component
                    .detail
                    .contains("局所Qy最大 |Qy|=14700.000 N（i端から x=300.000000 mm"));
            }
        }
    }
}

#[test]
fn 接続柱の偏心差による微小な離隔は平行移動後も連続とみなさない() {
    for translate_z_mm in [0.0, 1000.0] {
        for z_offset_mm in [0.0, -1e-14] {
            let mut model = model(false, false);
            let mut end = model.nodes[0].clone();
            end.id = NodeId(3);
            end.coord[2] = -3000.0;
            model.nodes.push(end);
            let mut column = model.elements[0].clone();
            column.id = ElemId(2);
            column.nodes = [NodeId(3), NodeId(0)].into_iter().collect();
            column.end_offsets_mm[1][2] = z_offset_mm;
            model.elements.push(column);
            translate_z(&mut model, translate_z_mm);
            let solved = analyze(&mut model);
            let expected = if z_offset_mm == 0.0 {
                Ok(300.0)
            } else {
                Err("柱の材軸が連続しない接合")
            };
            assert_ordinary_foot(&model, &solved, expected);
        }
    }
}

#[test]
fn 平面内の大きな平行移動でも梁側面との重なりと離隔を区別する() {
    for translate_y_mm in [0.0, 2_f64.powi(43)] {
        for offset_y_mm in [349.99975, 350.0, 350.00025] {
            let mut model = model(false, false);
            model.elements[1].end_offsets_mm = [[0.0, offset_y_mm, 0.0]; 2];
            for node in &mut model.nodes {
                node.coord[1] += translate_y_mm;
            }
            let solved = analyze(&mut model);
            let expected = if offset_y_mm < 350.0 {
                Ok(300.0)
            } else {
                Err("正の面積で接触")
            };
            assert_ordinary_foot(&model, &solved, expected);
        }
    }
}

fn translate_z(model: &mut Model, shift_mm: f64) {
    for node in &mut model.nodes {
        node.coord[2] += shift_mm;
    }
    for story in &mut model.stories {
        story.elevation = story.elevation.map(|z| z + shift_mm);
    }
}

fn saved_column_face(solved: &StaticOnce, side: usize) -> &Result<RcColumnFaceForce, String> {
    solved
        .rc_column_face_forces
        .iter()
        .find(|(id, _)| *id == COLUMN)
        .unwrap()
        .1[side]
        .as_ref()
        .unwrap()
}

fn assert_ordinary_foot(model: &Model, solved: &StaticOnce, expected: Result<f64, &str>) {
    let face = saved_column_face(solved, 0);
    let rows = check(model, solved, RcColumnLongTermShearInput::SingleCase);
    match expected {
        Ok(distance_mm) => {
            let face = face.as_ref().unwrap().face;
            assert_eq!(face.position_mm, distance_mm);
            let cut_xi = distance_mm / face.length_mm;
            assert!(rows.iter().any(|(xi, outcome)| *xi == cut_xi
                && matches!(outcome, CheckOutcome::Checked(result) if !result.components.is_empty())), "{rows:?}");
        }
        Err(expected) => {
            assert!(
                matches!(face, Err(reason) if reason.contains(expected)),
                "{face:?}"
            );
            assert!(rows.iter().any(|(_, outcome)|
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(expected))), "{rows:?}");
        }
    }
}

#[test]
fn 節点芯と実面が一致する場合は採用し隙間がある場合は未算定になる() {
    for beam_offset_z_mm in [-300.0, -320.0] {
        let mut model = model(false, false);
        model.elements[1].end_offsets_mm = [[0.0, 0.0, beam_offset_z_mm]; 2];
        let solved = analyze(&mut model);
        let rows = check_combination(&model, &solved);
        assert_eq!(rows.len(), 3);
        for (_, outcome) in rows {
            if beam_offset_z_mm == -300.0 {
                let component = shear(&outcome);
                assert!(component
                    .detail
                    .contains("局所Qy最大 |Qy|=15000.000 N（i端から x=0.000000 mm"));
                println!("{}", component.detail);
            } else {
                assert!(
                    matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("実配置と柱の接合境界を確認できません")
                        || reason.contains("正の面積で接触しません"))
                );
            }
        }
    }
}

#[test]
fn 引張専用ブレースの反復を使う保存組合せは柱区間のせん断を確定しない() {
    let mut model = model(false, true);
    let mut steel = model.materials[1].clone();
    steel.id = MaterialId(2);
    steel.category = MaterialCategory::Steel;
    model.materials.push(steel);
    let mut section = Section::zero(SectionId(2), "引張専用ブレース断面".into());
    section.material = Some(MaterialId(2));
    section.frame_use = Some(FrameSectionUse::Brace);
    section.area = 1000.0;
    model.sections.push(section);
    let mut brace = model.elements[1].clone();
    brace.id = ElemId(2);
    brace.kind = ElementKind::Brace { tension_only: true };
    brace.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    brace.section = Some(SectionId(2));
    model.elements.push(brace);
    model.stress_cfg.tension_only_iteration = true;
    let solved = analyze(&mut model);
    let rows = check_combination(&model, &solved);
    assert!(rows.iter().any(
        |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("引張専用ブレースの別ケース合成は同一解を保証できません"))
    ));
    assert!(!rows.iter().any(
        |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
        if result.components.iter().any(|component| component.kind == CheckKind::Shear))
    ));
}

#[test]
fn フェースとの一致が丸めで変わり得る集中荷重は該当方向を未算定にする() {
    let mut model = model(false, false);
    model.load_cases[0]
        .member
        .push(point(300.0 + 1e-11, 400_000.0));
    let solved = analyze(&mut model);
    assert!(check_combination(&model, &solved)
        .iter()
        .any(|(_, outcome)| matches!(
            outcome, CheckOutcome::Skipped { reason } if reason.contains("局所Qy")
                && reason.contains("材端フェースと直近の集中荷重の位置関係を確認できません")
        )));
}

#[test]
fn 集中荷重の材端フェースでは区間内側を採り区間内では両側を比較する() {
    let tip = |p| NodalLoad::manual(NodeId(1), [p, 0.0, 0.0, 0.0, 0.0, 0.0]);
    for (loads, tip_force, demand, x, side) in [
        (
            vec![
                point(300.0, 400_000.0),
                point(3000.0, 12_000.0),
                distributed(X, 300.0, 3000.0, 2.0, 2.0),
            ],
            10_000.0,
            27_400.0,
            300.0,
            "j端側",
        ),
        (
            vec![
                point(3000.0, 400_000.0),
                distributed(X, 300.0, 3000.0, -2.0, -2.0),
            ],
            -390_000.0,
            10_000.0,
            3000.0,
            "i端側",
        ),
        (
            vec![
                point(1000.0, -300_000.0),
                point(1100.0, 300_000.0),
                distributed(X, 0.0, 3000.0, 1.0, 1.0),
            ],
            12_000.0,
            314_000.0,
            1000.0,
            "j端側",
        ),
        (
            vec![
                point(1000.0, -300_000.0),
                point(1100.0, 300_000.0),
                distributed(X, 0.0, 3000.0, -1.0, -1.0),
            ],
            12_000.0,
            310_100.0,
            1100.0,
            "i端側",
        ),
    ] {
        let mut model = model(false, false);
        model.load_cases[0].member = loads;
        model.load_cases[0].nodal = vec![tip(tip_force)];
        let solved = analyze(&mut model);
        for (_, outcome) in check_combination(&model, &solved) {
            let component = shear(&outcome);
            assert!(
                component.detail.contains(&format!(
                    "局所Qy最大 |Qy|={demand:.3} N（i端から x={x:.6} mm（{side}）"
                )),
                "{}",
                component.detail
            );
            println!("{}", component.detail);
        }
    }
}

#[test]
fn 断面力や生成入力を照合できないせん断だけ未算定になる() {
    let mut model = model(false, true);
    let mut opposite_bending = [0.0; 6];
    opposite_bending[4] = 250_000_000.0;
    model.load_cases[0]
        .nodal
        .push(NodalLoad::manual(NodeId(1), opposite_bending));
    let solved = analyze(&mut model);
    let reasons = |rows: &[(f64, CheckOutcome)], wanted: &str| {
        assert!(rows.iter().any(|(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(wanted))), "{rows:?}");
    };
    let unverified = check(&model, &solved, RcColumnLongTermShearInput::Unverified);
    reasons(&unverified, "保存荷重組合せと生成入力が未確認");
    assert!(unverified.iter().any(|(_, outcome)| matches!(
        outcome, CheckOutcome::Checked(result) if result.components.iter().any(|c| c.kind == CheckKind::AxialBending && c.ratio > 1.0)
    )));
    assert!(unverified.iter().all(|(_, outcome)| !matches!(
        outcome, CheckOutcome::Checked(result) if result.components.iter().any(|c| c.kind == CheckKind::Shear)
    )));
    let mut damaged = solved.clone();
    let column = damaged
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == COLUMN)
        .unwrap();
    column.1.at[2].1[2] += 1000.0;
    let mismatch = check_combination(&model, &damaged);
    reasons(&mismatch, "局所Qz");
    assert!(mismatch.iter().any(|(_, outcome)| matches!(
        outcome, CheckOutcome::Checked(result) if result.components.iter().any(|c| c.kind == CheckKind::Shear && c.ratio > 1.0 && c.detail.contains("313900.000"))
    )));
    damaged.member_forces.retain(|(id, _)| *id != COLUMN);
    reasons(
        &check_combination(&model, &damaged),
        "対象部材の解析内力がありません",
    );
    model.elements[0].rigid_zone.face_i = None;
    reasons(
        &check_combination(&model, &solved),
        "両材端フェース位置が未算定",
    );
    model.elements[0].rigid_zone.face_i = Some(300.0);
    model.elements[0].end_offsets_mm[0][0] = 1.0;
    reasons(&check_combination(&model, &solved), "XYZ偏心");
    model.elements[0].end_offsets_mm[0][0] = 0.0;
    model.elements[0].rigid_zone.panel_offset_i = 150.0;
    reasons(&check_combination(&model, &solved), "仕口パネル");
}

#[test]
fn 単独ケースは部材区間の最大せん断力で置き換えない() {
    let mut model = model(false, true);
    sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(LoadCaseId(0))
        .unwrap();
    let rows = check(&model, &solved, RcColumnLongTermShearInput::SingleCase);
    assert_eq!(rows.len(), 3);
    for (_, outcome) in rows {
        let component = shear(&outcome);
        assert!(component.ratio < 1.0);
        assert!(!component.detail.contains("局所Qy最大"));
    }
}

#[test]
fn 準備時に材端フェースを確認できない線材で未算定になる() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = model(false, false);
        model.elements[0].kind = kind;
        let solved = analyze(&mut model);
        assert!(!model.elements[0].rigid_zone.faces_computed(), "{kind:?}");
        assert!(check_combination(&model, &solved).iter().any(|(_, outcome)| matches!(
            outcome, CheckOutcome::Skipped { reason } if reason.contains("両材端フェース位置が未算定")
        )));
    }
}

#[test]
fn 正しいフェース区間を確定できない壁側柱の長期せん断は未算定になる() {
    let mut model = model(false, false);
    let solved = analyze(&mut model);
    let extra_node = |id, coord| sepika_core::model::Node {
        id: NodeId(id),
        coord,
        ..model.nodes[0].clone()
    };
    model.nodes.push(extra_node(3, [3000.0, 0.0, 3000.0]));
    let mut wall = model.elements[1].clone();
    wall.id = ElemId(2);
    wall.kind = ElementKind::Wall;
    wall.nodes = [NodeId(0), NodeId(2), NodeId(3), NodeId(1)]
        .into_iter()
        .collect();
    model.elements.push(wall);
    assert!(check_combination(&model, &solved)
        .iter()
        .any(|(_, outcome)| matches!(
            outcome, CheckOutcome::Skipped { reason } if reason.contains("壁側柱")
        )));
}
