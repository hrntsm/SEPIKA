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
    MemberDesignCheckReport, RcColumnLongTermShearInput,
};
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::StaticOnce;

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
                let rows = check_combination(&model, &incomplete);
                let y_peak_mm = if w == 0.0 { 300.0 } else { 1100.0 };
                let y_demand =
                    format!("局所Qy最大 |Qy|={demand_n:.3} N（i端から x={y_peak_mm:.6} mm");
                let z_demand = "局所Qz最大 |Qz|=0.000 N（i端から x=300.000000 mm";
                for target in omitted {
                    assert!(rows.iter().any(|(xi, outcome)| xi == target && matches!(
                        outcome, CheckOutcome::Skipped { reason } if
                            reason.contains("軸力・曲げを検定する断面位置の解析内力がありません")
                            && reason.contains(&y_demand)
                            && reason.contains(z_demand)
                            && !reason.contains("未算定")
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
            [0.1, 0.5, 1.0]
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
    assert!(
        matches!(target.as_slice(), [(_, _, CheckOutcome::Skipped { reason })]
        if reason.contains("解析内力が重複し、一意の断面力を確認できません"))
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
            [0.1, 0.5, 1.0]
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
                    matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("実配置と柱の接合境界を確認できません"))
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
