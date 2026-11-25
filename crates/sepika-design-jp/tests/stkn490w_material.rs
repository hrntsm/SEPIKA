use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, SectionId};
use sepika_core::model::FrameSectionUse;
use sepika_core::section_shape::SectionShape;
use sepika_core::standard_material::standard_material_properties;
use sepika_design_jp::ultimate::collect_cft_ultimate_checks;
use sepika_design_jp::{run_member_design_checks, CheckOutcome};
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::StaticOnce;
use std::f64::consts::PI;

#[path = "fixtures/cft_column_buckling_model.rs"]
mod input;

const COLUMN: ElemId = ElemId(0);
const TENSION_N: f64 = 20_000_000.0;
const DIRECT_FY: f64 = 277.0;
const PIPE_DIAMETER_MM: f64 = 400.0;

#[test]
fn stkn490w_requires_explicit_strength_in_steel_and_cft_column_checks() {
    let stock = standard_material_properties("STKN490B").unwrap();
    assert_eq!(stock.fy, Some(325.0));
    assert!(standard_material_properties("STKN490W").is_none());

    for cft in [false, true] {
        for (thick, standard_f) in [(40.0, 325.0), (41.0, 295.0)] {
            let mut model = input::cantilever(true, -TENSION_N);
            let shape = if cft {
                SectionShape::CftPipe {
                    outer_dia: PIPE_DIAMETER_MM,
                    thick,
                }
            } else {
                SectionShape::SteelPipe {
                    outer_dia: PIPE_DIAMETER_MM,
                    thick,
                }
            };
            let sec = &mut model.sections[0];
            *sec = shape.to_section(SectionId(0), "円形鋼管柱".into());
            sec.frame_use = Some(FrameSectionUse::Column);
            sec.material = Some(MaterialId(if cft { 0 } else { 1 }));
            sec.steel_material = cft.then_some(MaterialId(1));
            model.steel_design_attrs =
                vec![input::direct_lengths(COLUMN, Some(6000.0), Some(6000.0))];
            let steel_area_mm2 = PI * (PIPE_DIAMETER_MM - thick) * thick;
            for (name, fy, adopted_f) in [
                ("STKN490B", stock.fy, standard_f),
                ("STKN490B", Some(DIRECT_FY), standard_f),
                ("STKN490W", Some(DIRECT_FY), DIRECT_FY),
                ("STKN490W", None, 0.0),
            ] {
                let mat = &mut model.materials[1];
                mat.name = name.into();
                mat.fy = fy;
                let check = |solved: &StaticOnce| {
                    run_member_design_checks(
                        &model,
                        &solved.member_forces,
                        &solved.panel_moments,
                        &Default::default(),
                    )
                };
                let case = format!("CFT={cft}, t={thick}, {name}, fy={fy:?}");
                if fy.is_none() {
                    let error = if cft {
                        let error = Analysis::prepare(&model).err().unwrap().to_string();
                        assert!(
                            error.contains("CFT 断面の鋼管材料が不正です") && error.contains("fy"),
                            "{case}: {error}",
                        );
                        let mut force_model = model.clone();
                        force_model.materials[1].fy = Some(DIRECT_FY);
                        let solved = Analysis::prepare(&force_model)
                            .unwrap()
                            .linear_static(LoadCaseId(0))
                            .unwrap();
                        let report = check(&solved).unwrap();
                        assert_eq!(report.member_checks.len(), 3);
                        for (_, _, outcome) in report.member_checks {
                            let CheckOutcome::Skipped { reason } = outcome else {
                                panic!("{case}: {outcome:?}");
                            };
                            assert!(reason.contains("鋼管材料"), "{case}: {reason}");
                            assert!(reason.contains("fy が不正"), "{case}: {reason}");
                        }
                        let reason = collect_cft_ultimate_checks(&model, &[(COLUMN, -TENSION_N)])
                            .unwrap_err();
                        assert!(
                            reason.contains("鋼管 F 値が未設定または不正"),
                            "{case}: {reason}"
                        );
                        continue;
                    } else {
                        let solved = Analysis::prepare(&model)
                            .unwrap()
                            .linear_static(LoadCaseId(0))
                            .unwrap();
                        check(&solved).unwrap_err()
                    };
                    assert!(
                        error.contains(name) && error.contains("fy"),
                        "{case}: {error}"
                    );
                    continue;
                }

                let solved = Analysis::prepare(&model)
                    .unwrap()
                    .linear_static(LoadCaseId(0))
                    .unwrap();
                let report = check(&solved).unwrap();
                assert_eq!(report.member_checks.len(), 3, "{case}");
                let expected_ratio = 1.5 * TENSION_N / (steel_area_mm2 * adopted_f);
                for (_, _, outcome) in report.member_checks {
                    let CheckOutcome::Checked(result) = outcome else {
                        panic!("{case}: {outcome:?}");
                    };
                    assert!(
                        (result.ratio() - expected_ratio).abs() < 1e-9,
                        "{case}: expected {expected_ratio}, got {result:?}"
                    );
                    assert!(!result.ok(), "{case}: {result:?}");
                }
                if cft {
                    let rows =
                        collect_cft_ultimate_checks(&model, &[(COLUMN, -TENSION_N)]).unwrap();
                    assert_eq!(rows.len(), 1, "{case}");
                    assert!(
                        (rows[0].ntu - steel_area_mm2 * adopted_f).abs() < 0.01,
                        "{case}: {rows:?}"
                    );
                    assert!(!rows[0].ok, "{case}: {rows:?}");
                }
                assert_eq!(model.materials[1].fy, fy, "{case}");
            }
        }
    }
}
