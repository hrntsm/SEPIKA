use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::load_combo::{combination_design_state, LoadAction, LoadDuration};
use sepika_core::model::*;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_solver::statics::analysis::Analysis;

#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;

const COLUMN: ElemId = ElemId(0);
const LENGTH_MM: f64 = 3000.0;
const PEAK_MM: f64 = 1100.0;
const TIP_FORCE_N: f64 = 12_000.0;

fn model(circle: bool, inner_peak: bool, observe_peak: bool) -> Model {
    let mut model = column_input::column_model(circle, 0.0);
    let mut beam_section = beam_input::section(300.0, 600.0, 19.0, &[3], &[3]);
    beam_section.id = SectionId(1);
    model.sections.push(beam_section);
    let mut beam_end = model.nodes[0].clone();
    beam_end.id = NodeId(2);
    beam_end.coord = [3000.0, 0.0, 0.0];
    beam_end.restraint = Dof6Mask::FIXED;
    model.nodes.push(beam_end);
    let mut beam = model.elements[0].clone();
    beam.id = ElemId(1);
    beam.nodes = [NodeId(0), NodeId(2)].into_iter().collect();
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    beam.section = Some(SectionId(1));
    model.elements.push(beam);
    model.panel_zone = PanelZoneMode::None;
    model.girder_torsion = GirderTorsionMode::Keep;

    model.load_cases = default_load_cases();
    model.combinations = vec![default_combinations().remove(0)];
    let dead = &mut model.load_cases[0];
    dead.nodal.push(NodalLoad::manual(
        NodeId(1),
        [TIP_FORCE_N, 0.0, 0.0, 0.0, 0.0, 0.0],
    ));
    let distributed = |a, b, w| {
        MemberLoad::manual(
            COLUMN,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Distributed { a, b, w1: w, w2: w },
        )
    };
    dead.member.push(distributed(0.0, LENGTH_MM, 1.0));
    if inner_peak {
        dead.member.push(distributed(1050.0, PEAK_MM, -6000.0));
        dead.member.push(distributed(PEAK_MM, 1150.0, 6000.0));
    }
    if observe_peak {
        let mut detail = MemberDetailAttr::new(COLUMN);
        detail.joints.push(MemberJoint {
            distance: PEAK_MM,
            kind: JointKind::Site,
        });
        model.member_detail_attrs.push(detail);
    }
    model
}

fn close(actual: f64, expected: f64, abs_tol: f64) {
    assert!(
        (actual - expected).abs() < abs_tol,
        "{actual} != {expected} (許容差 {abs_tol})"
    );
}

fn by_equilibrium(x_mm: f64, inner_peak: bool) -> (f64, f64) {
    let remaining_mm = LENGTH_MM - x_mm;
    let mut shear_n = TIP_FORCE_N + remaining_mm;
    let mut moment_nmm = TIP_FORCE_N * remaining_mm + remaining_mm.powi(2) / 2.0;
    if inner_peak {
        for (start, end, direction) in [(1050.0, 1100.0, -1.0), (1100.0, 1150.0, 1.0)] {
            let lower = x_mm.max(start);
            if lower < end {
                let force_n = direction * 6000.0 * (end - lower);
                shear_n += force_n;
                moment_nmm += force_n * ((end + lower) / 2.0 - x_mm);
            }
        }
    }
    (shear_n, moment_nmm)
}

#[test]
fn compare_long_term_rc_column_shear_stations() {
    for circle in [false, true] {
        for (inner_peak, observe_peak) in [(false, false), (true, false), (true, true)] {
            let mut model = model(circle, inner_peak, observe_peak);
            sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
            let combo = &model.combinations[0];
            let state = combination_design_state(combo, &model.load_cases).unwrap();
            assert_eq!(state.duration, LoadDuration::Long);
            assert_eq!(state.action, LoadAction::Gravity);
            let solved = Analysis::prepare(&model)
                .unwrap()
                .linear_combination(combo)
                .unwrap();
            let zone = model.elements[0].rigid_zone;
            assert_eq!(zone.face_i, Some(300.0));
            assert_eq!(zone.face_j, Some(0.0));
            assert_eq!(zone.rigid_length_i(), 150.0);
            assert_eq!(zone.rigid_length_j(), 0.0);
            let shape = if circle { "円形" } else { "矩形" };
            println!("{shape} 内点ピーク={inner_peak} 追加位置={observe_peak} 組合せ={} フェース=({:?}, {:?}) 剛体アーム=({}, {})", combo.name, zone.face_i, zone.face_j, zone.rigid_length_i(), zone.rigid_length_j());
            let (_, forces) = solved
                .member_forces
                .iter()
                .find(|(id, _)| *id == COLUMN)
                .unwrap();
            let mut stations_mm = vec![0.0, 300.0, 1500.0, LENGTH_MM];
            if observe_peak {
                stations_mm.insert(2, PEAK_MM);
            }
            assert_eq!(forces.at.len(), stations_mm.len());
            for ((pos, values), x_mm) in forces.at.iter().zip(stations_mm.iter().copied()) {
                close(pos * LENGTH_MM, x_mm, 1e-8);
                let (shear_n, moment_nmm) = by_equilibrium(x_mm, inner_peak);
                println!(
                    "  解析 x={x_mm:.0} Qy={:.3} Qz={:.3} My={:.3} Mz={:.3}",
                    values[1], values[2], values[4], values[5]
                );
                close(values[1], -shear_n, 1e-5);
                close(values[2], 0.0, 1e-5);
                close(values[4], 0.0, 1e-2);
                close(values[5], moment_nmm, 1e-2);
            }
            let peak_mm = if inner_peak { PEAK_MM } else { 300.0 };
            let peak_n = by_equilibrium(peak_mm, inner_peak).0;
            close(peak_n, if inner_peak { 313_900.0 } else { 14_700.0 }, 1e-8);
            let width_mm = if circle {
                300.0 * std::f64::consts::PI.sqrt()
            } else {
                400.0
            };
            let upper_capacity_n = width_mm * 525.0 * 1.5 * 0.73;
            if inner_peak {
                assert!(peak_n > upper_capacity_n);
            }
            println!("  αとjを上限に取ったQy許容={upper_capacity_n:.3} N");
            println!("  フェース間の独立最大 x={peak_mm:.0} |Qy|={peak_n:.3}");

            let report = run_member_design_checks(
                &model,
                &solved.member_forces,
                &solved.panel_moments,
                &MemberDesignCheckOptions {
                    term: LoadTerm::Long,
                    girder_column_face_forces: &solved.girder_column_face_forces,
                    wall_case: &combo.name,
                    load_terms: Some(&combo.terms),
                    ..Default::default()
                },
            )
            .unwrap();
            let rows: Vec<_> = report
                .member_checks
                .iter()
                .filter(|(id, _, _)| *id == COLUMN)
                .collect();
            assert_eq!(rows.len(), stations_mm.len() - 1);
            let mut ratios = Vec::new();
            for ((_, pos, outcome), x_mm) in rows.into_iter().zip(stations_mm.into_iter().skip(1)) {
                close(pos * LENGTH_MM, x_mm, 1e-8);
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("x={x_mm} {outcome:?}");
                };
                assert_eq!(
                    result.ok(),
                    !observe_peak || x_mm != PEAK_MM,
                    "x={x_mm} {result:?}"
                );
                for component in &result.components {
                    assert!(component.ratio.is_finite() && component.ratio >= 0.0);
                    if component.kind != CheckKind::Shear {
                        assert!(component.ratio <= 1.0);
                    }
                    println!(
                        "  検定 x={x_mm:.0} {} 比={:.9} {}",
                        component.kind.label(),
                        component.ratio,
                        component.detail
                    );
                }
                let shear = result
                    .components
                    .iter()
                    .find(|c| c.kind == CheckKind::Shear)
                    .unwrap();
                assert!(result
                    .components
                    .iter()
                    .any(|component| component.kind == CheckKind::AxialBending));
                let demand_n = by_equilibrium(x_mm, inner_peak).0;
                let capacity_n = demand_n / shear.ratio;
                assert!(capacity_n <= upper_capacity_n);
                ratios.push((shear.ratio, capacity_n));
                println!(
                    "  判定 x={x_mm:.0} {} 総合比={:.9} Qy許容={capacity_n:.3} N",
                    if result.ok() { "OK" } else { "NG" },
                    result.ratio()
                );
            }
            for &(_, capacity_n) in &ratios {
                close(capacity_n, ratios[0].1, 1e-5);
            }
            let governing_shear = ratios.iter().map(|pair| pair.0).reduce(f64::max).unwrap();
            assert_eq!(governing_shear > 1.0, observe_peak);
            assert_eq!(peak_n / ratios[0].1 > 1.0, inner_peak);
            println!(
                "  せん断最大比={governing_shear:.9} 独立最大需要/同許容={:.9}",
                peak_n / ratios[0].1
            );
        }
    }
}
