#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;
#[path = "../../sepika-design-jp/tests/fixtures/rc_column_long_term_shear_model.rs"]
mod input;

use input::{distributed, COLUMN, LENGTH_MM};
use sepika_core::ids::NodeId;
use sepika_core::model::{Model, NodalLoad, Section};
use sepika_core::section_shape::{RebarSize, SectionShape};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, MemberDesignCheckOptions,
    RcColumnLongTermShearInput,
};
use sepika_solver::statics::analysis::Analysis;

fn model(circle: bool) -> Model {
    let mut model = input::model(
        column_input::column_model(circle, 0.0),
        beam_input::section(300.0, 600.0, 19.0, &[3], &[3]),
        false,
    );
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
    shape.validate_rebar().unwrap();
    *section = Section {
        material: section.material,
        rebar_material: section.rebar_material,
        shear_rebar_material: section.shear_rebar_material,
        frame_use: section.frame_use,
        ..shape.to_section(section.id, section.name.clone())
    };
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [25_000.0, 0.0, 0.0, 0.0, -145_000_000.0, 0.0],
    )];
    model.load_cases[0].member = vec![distributed([1.0, 0.0, 0.0], 0.0, LENGTH_MM, 60.0, 60.0)];
    model
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn 長期許容せん断力の柱脚柱頭の割増係数を照合する() {
    for (circle, expected_qa_n, expected_alpha) in [
        (false, 193_942.878_283_6, "1.418"),
        (true, 205_560.670_518_3, "1.295"),
    ] {
        let mut model = model(circle);
        sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
        let zone = model.elements[0].rigid_zone;
        assert_eq!((zone.face_i, zone.face_j), (Some(300.0), Some(0.0)));
        let combination = &model.combinations[0];
        let solved = Analysis::prepare(&model)
            .unwrap()
            .linear_combination(combination)
            .unwrap();
        let (_, forces) = solved
            .member_forces
            .iter()
            .find(|(id, _)| *id == COLUMN)
            .unwrap();
        let reference = [
            (0.0, 200_000_000.0, -205_000.0),
            (0.1, 141_200_000.0, -187_000.0),
            (0.5, -40_000_000.0, -115_000.0),
            (1.0, -145_000_000.0, -25_000.0),
        ];
        assert_eq!(forces.at.len(), reference.len());
        for ((xi, force), (expected_xi, expected_mz, expected_qy)) in
            forces.at.iter().zip(reference)
        {
            assert_close(*xi, expected_xi, 1e-9);
            assert_close(force[5], expected_mz, 1e-2);
            assert_close(force[1], expected_qy, 1e-5);
            assert_close(force[4], 0.0, 1e-2);
            assert_close(force[2], 0.0, 1e-5);
        }

        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                rc_column_long_term_shear: RcColumnLongTermShearInput::SavedCombination(
                    &combination.name,
                ),
                ..Default::default()
            },
        )
        .unwrap();
        let rows: Vec<_> = report
            .member_checks
            .iter()
            .filter(|(id, _, _)| *id == COLUMN)
            .collect();
        assert_eq!(
            rows.iter().map(|(_, xi, _)| *xi).collect::<Vec<_>>(),
            [0.1, 0.5, 1.0]
        );

        for (_, xi, outcome) in rows {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            assert!(result.ok(), "{result:?}");
            let bending = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap();
            let shear = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Shear)
                .unwrap();
            assert!(bending.ratio < 1.0, "{bending:?}");
            assert_close(shear.ratio, 187_000.0 / expected_qa_n, 1e-8);
            assert!(
                shear.detail.contains(&format!("QAy={expected_qa_n:.1} N")),
                "{shear:?}"
            );
            assert!(
                shear.detail.contains(&format!("αy={expected_alpha}")),
                "{shear:?}"
            );
            assert!(
                shear
                    .detail
                    .contains("局所Qy最大 |Qy|=187000.000 N（i端から x=300.000000 mm"),
                "{shear:?}"
            );
            println!(
                "円形={circle}, ξ={xi}, 全体OK={}, 軸力・曲げ比={:.6}, {}",
                result.ok(),
                bending.ratio,
                shear.detail
            );
        }
    }
}
