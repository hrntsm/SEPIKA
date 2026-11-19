#[path = "../../sepika-design-jp/tests/fixtures/cft_column_buckling_model.rs"]
mod cft_column_buckling_input;
#[path = "support/cft_long_case2_model.rs"]
mod cft_long_case2_model;
#[path = "support/cft_long_case2_reference.rs"]
mod cft_long_case2_reference;
#[path = "support/column_model.rs"]
mod column_model;

use cft_long_case2_model::{cft_long_case2_model, tip_compression};
use cft_long_case2_reference::assert_cft_long_case2_summary;
use column_model::rc_column_model;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, SectionId};
use sepika_core::model::{ElementKind, LoadCase, LoadCaseKind, LoadCombination, Model};
use sepika_core::section_shape::SectionShape;
use sepika_mcp::{compute_job, JobKind, JobOutcome, JobParams};

#[test]
fn cft_long_case2_job_reports_independent_mu_and_never_passes_axial_overload() {
    for (dead_n, live_n, ok) in [
        (2_000_000.0, 2_000_000.0, true),
        (4_000_000.0, 4_000_000.0, false),
    ] {
        let model = cft_long_case2_model(dead_n, live_n);
        let summary = ultimate_check_summary(&model);
        assert_cft_long_case2_summary(&summary, ok);
        assert_eq!(
            summary["load_target"],
            serde_json::json!({
                "source": "saved_combination",
                "combination_index": 0,
                "purpose": "frame_long",
                "mode": "static_hinge",
                "terms": [[0, 1.0], [1, 1.0]],
            }),
        );
    }
}

fn ultimate_check_summary(model: &Model) -> serde_json::Value {
    let JobOutcome::UltimateCheck { summary } =
        compute_job(model, JobKind::UltimateCheck, &JobParams::default()).unwrap()
    else {
        panic!("終局検定の結果が必要");
    };
    summary
}

fn ultimate_check_error(model: &Model, params: &JobParams) -> String {
    let Err(sepika_job::JobError::InvalidInput(reason)) =
        compute_job(model, JobKind::UltimateCheck, params)
    else {
        panic!("不完全な終局検定を拒否する必要があります");
    };
    reason
}

#[test]
fn ultimate_check_rejects_single_case_and_combination_override() {
    let model = cft_long_case2_model(4_000_000.0, 4_000_000.0);
    for (params, arg) in [
        (
            JobParams {
                load_case: Some(0),
                ..Default::default()
            },
            "load_case",
        ),
        (
            JobParams {
                load_combination: Some(0),
                ..Default::default()
            },
            "load_combination",
        ),
    ] {
        let reason = ultimate_check_error(&model, &params);
        assert!(
            reason.contains(arg) && reason.contains("長期架構用組合せ"),
            "{reason}"
        );
    }
}

#[test]
fn ultimate_check_rejects_missing_saved_gravity_or_seismic_live_substitution() {
    let mut model = cft_long_case2_model(4_000_000.0, 4_000_000.0);
    let params = JobParams::default();
    for selection in [None, Some(99)] {
        model.ultimate_gravity_combination = selection;
        assert!(ultimate_check_error(&model, &params).contains("gravity_combination_missing"));
    }

    model.ultimate_gravity_combination = Some(0);
    model.load_cases[1].kind = LoadCaseKind::LiveSeismic;
    let reason = ultimate_check_error(&model, &params);
    assert!(
        reason.contains("gravity_combination_unsupported") && reason.contains("荷重ケース 1"),
        "{reason}"
    );

    model.load_cases.pop();
    let reason = ultimate_check_error(&model, &params);
    assert!(
        reason.contains("gravity_combination_unsupported")
            && reason.contains("荷重ケース 1 がありません"),
        "{reason}"
    );
}

#[test]
fn ultimate_check_reports_snow_coefficient_and_uses_only_effective_terms() {
    let mut model = cft_long_case2_model(2_000_000.0, 1_300_000.0);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(2),
        name: "積雪".into(),
        kind: LoadCaseKind::Snow,
        nodal: vec![tip_compression(1_000_000.0)],
        member: Vec::new(),
    });
    let mut snow_combination = model.combinations[0].clone();
    snow_combination.terms = vec![
        (LoadCaseId(2), 0.7),
        (LoadCaseId(1), 0.5),
        (LoadCaseId(0), f64::MAX),
        (LoadCaseId(0), 1.0),
        (LoadCaseId(0), -f64::MAX),
        (LoadCaseId(1), 0.5),
    ];
    model.combinations.push(snow_combination);
    model.ultimate_gravity_combination = Some(1);

    let summary = ultimate_check_summary(&model);
    assert_cft_long_case2_summary(&summary, true);
    assert_eq!(summary["load_target"]["combination_index"], 1);
    assert_eq!(
        summary["load_target"]["terms"],
        serde_json::json!([[0, 1.0], [1, 1.0], [2, 0.7]])
    );
}

#[test]
fn ultimate_check_rejects_failed_gravity_case() {
    let mut model = cft_long_case2_model(4_000_000.0, 4_000_000.0);
    model.load_cases[1].nodal = vec![tip_compression(f64::NAN)];
    let reason = ultimate_check_error(&model, &JobParams::default());
    assert!(
        reason.contains("gravity_case_missing: case=1") && reason.contains("解析エラー"),
        "{reason}"
    );
}

#[test]
fn ultimate_check_rejects_superposition_with_tension_only_iteration() {
    let mut model = cft_long_case2_model(4_000_000.0, 4_000_000.0);
    let mut brace = model.elements[0].clone();
    brace.id = ElemId(1);
    brace.kind = ElementKind::Brace { tension_only: true };
    brace.section = Some(SectionId(1));
    model.sections.push(
        SectionShape::SteelRoundBar { dia: 20.0 }.to_section(SectionId(1), "ブレース".into()),
    );
    model.sections[1].material = Some(MaterialId(1));
    model.elements.push(brace);
    model.stress_cfg.tension_only_iteration = true;
    let reason = ultimate_check_error(&model, &JobParams::default());
    assert!(
        reason.contains("gravity_superposition_unsupported") && reason.contains("引張専用ブレース"),
        "{reason}"
    );
}

#[test]
fn cft_ultimate_job_rejects_missing_or_zero_direct_length_with_axis() {
    let mut model = cft_long_case2_model(2_000_000.0, 2_000_000.0);
    for invalid in [None, Some(0.0)] {
        model.steel_design_attrs[0].lk_z_direct = invalid;
        let Err(sepika_job::JobError::InvalidInput(reason)) =
            compute_job(&model, JobKind::UltimateCheck, &JobParams::default())
        else {
            panic!("CFT終局検定は片軸で成立しません");
        };
        assert!(reason.contains("CFT柱 ID 0: 弱軸の座屈長さ"), "{reason}");
    }
}

#[test]
fn test_compute_ultimate_check_job_rejects_incomplete_rc_gravity_demand() {
    let mut model = rc_column_model();
    model.combinations = vec![LoadCombination {
        name: "Gのみ".into(),
        terms: vec![(LoadCaseId(0), 1.0)],
    }];
    model.ultimate_gravity_combination = Some(0);
    let reason = ultimate_check_error(&model, &JobParams::default());
    assert!(
        reason.contains("gravity_combination_unsupported") && reason.contains("架構用積載P"),
        "{reason}"
    );
}

#[test]
fn ultimate_check_keeps_rc_gravity_demand_unavailable_even_with_cft_and_saved_gravity() {
    let mut model = cft_long_case2_model(2_000_000.0, 2_000_000.0);
    let rc = rc_column_model();
    let mut column = model.elements[0].clone();
    column.id = ElemId(1);
    column.section = Some(SectionId(1));
    let mut concrete = rc.sections[0].clone();
    concrete.id = SectionId(1);
    let mut rebar = rc.materials[1].clone();
    rebar.id = MaterialId(2);
    concrete.rebar_material = Some(rebar.id);
    concrete.shear_rebar_material = Some(rebar.id);
    model.materials.push(rebar);
    model.sections.push(concrete);
    model.elements.push(column);

    let reason = ultimate_check_error(&model, &JobParams::default());
    assert!(reason.contains("ultimate_gravity_missing"), "{reason}");
    assert!(reason.contains("member=1"), "{reason}");
    assert!(reason.contains("purpose=frame_long"), "{reason}");
}
