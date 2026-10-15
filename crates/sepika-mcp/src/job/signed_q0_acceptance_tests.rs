use super::*;
#[path = "../../../sepika-app/src/app/signed_q0_acceptance_fixture.rs"]
mod fixture;

fn summary(model: &Model, automatic: bool) -> serde_json::Value {
    let outcome = compute_job(
        model,
        JobKind::DesignCheck,
        &JobParams {
            load_case: automatic.then_some(2),
            load_combination: (!automatic).then_some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let JobOutcome::DesignCheck { summary, .. } = outcome else {
        panic!("DesignCheck")
    };
    summary
}

#[test]
fn signed_q0_mcp_saved_and_automatic_selection_preserve_case_partition() {
    for automatic in [false, true] {
        let mut model = fixture::model();
        let split = summary(&model, automatic);
        let live = model.load_cases[1].member.pop().unwrap();
        model.load_cases[0].member.push(live);
        let merged = summary(&model, automatic);
        assert_eq!(split["n_skipped"], 0);
        assert_eq!(split["qd_wired"], true);
        assert_eq!(split["n_checks"], merged["n_checks"]);
        let a = split["max_ratio"].as_f64().unwrap();
        let b = merged["max_ratio"].as_f64().unwrap();
        assert!((a - b).abs() < 1e-10, "{split}\n{merged}");
        assert_eq!(
            split["load_target"]["source"],
            if automatic {
                "automatic_gravity_combination"
            } else {
                "saved_combination"
            }
        );
    }
}

#[test]
fn signed_q0_mcp_nonstandard_live_factor_is_reasoned_unchecked_and_steel_needs_no_q0() {
    let mut model = fixture::model();
    model.combinations[0].terms[1].1 = 0.5;
    let skipped = summary(&model, false);
    assert_eq!(skipped["all_checked_and_ok"], false);
    assert!(skipped["load_target"]["diagnostic"]
        .as_str()
        .unwrap()
        .contains("係数"));
    model.combinations[0].terms[1].1 = 1.0;
    let mut steel = sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: 300.0,
        thick: 600.0,
    }
    .to_section(sepika_core::ids::SectionId(0), "鋼梁".into());
    steel.material = Some(sepika_core::ids::MaterialId(0));
    steel.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
    model.sections[0] = steel;
    model.materials[0].category = sepika_core::model::MaterialCategory::Steel;
    model.materials[0].name = "SS400".into();
    model.materials[0].young = 205000.0;
    model.materials[0].density = 7.85e-9;
    model.materials[0].fy = Some(235.0);
    model.load_cases[0].member.clear();
    model.load_cases[1].member.clear();
    for automatic in [false, true] {
        let checked = summary(&model, automatic);
        assert_eq!(checked["n_skipped"], 0, "{checked}");
        assert!(checked["max_ratio"].as_f64().is_some());
    }
}

#[test]
fn signed_q0_mcp_missing_repaired_missing_never_reports_success() {
    let mut model = fixture::model();
    for valid in [false, true, false] {
        model.load_cases[0].member[0].dir = if valid { [0.0, 0.0, -1.0] } else { [0.0; 3] };
        for automatic in [false, true] {
            let result = compute_job(
                &model,
                JobKind::DesignCheck,
                &JobParams {
                    load_case: automatic.then_some(2),
                    load_combination: (!automatic).then_some(0),
                    ..Default::default()
                },
            );
            if valid {
                let JobOutcome::DesignCheck { summary, .. } = result.unwrap() else {
                    panic!()
                };
                assert_eq!(summary["n_skipped"], 0);
            } else if let Ok(JobOutcome::DesignCheck { summary, .. }) = result {
                assert_eq!(summary["all_checked_and_ok"], false);
                assert!(summary["n_skipped"].as_u64().unwrap() > 0);
                assert!(summary["member_skipped"].to_string().contains("Q0"));
            } else {
                assert!(result.is_err());
            }
        }
    }
}
