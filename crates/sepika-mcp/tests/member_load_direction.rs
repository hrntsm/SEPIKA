use sepika_core::model::{MemberLoadKind, Model};
use sepika_mcp::{compute_job, JobKind, JobOutcome, JobParams};
use serde_json::{json, Value};

fn fixture(distributed: bool) -> Model {
    let mut model: Model =
        serde_json::from_str(include_str!("fixtures/member_load_direction_src.json")).unwrap();
    let load = &mut model.load_cases[0].member[0];
    load.name = "手入力100kN".into();
    if distributed {
        load.kind = MemberLoadKind::Distributed {
            a: 0.0,
            b: 6000.0,
            w1: 100_000.0 / 6000.0,
            w2: 100_000.0 / 6000.0,
        };
    }
    model.validate().unwrap();
    model
}

fn evidence(label: &str, value: &Value) {
    if let Some(dir) = std::env::var_os("SEPIKA687_EVIDENCE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{label}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

fn design(model: &Model, combination: usize, label: &str) -> Result<Value, String> {
    evidence(
        &format!("{label}-input"),
        &serde_json::to_value(model).unwrap(),
    );
    let outcome = compute_job(
        model,
        JobKind::DesignCheck,
        &JobParams {
            load_combination: Some(combination),
            ..Default::default()
        },
    );
    let response = match outcome {
        Ok(JobOutcome::DesignCheck {
            case,
            member_force_rows,
            summary,
        }) => Ok(json!({"case": case, "member_force_rows": member_force_rows, "summary": summary})),
        Ok(_) => panic!("DesignCheck以外の応答"),
        Err(error) => Err(error.to_string()),
    };
    evidence(
        &format!("{label}-input-after"),
        &serde_json::to_value(model).unwrap(),
    );
    evidence(
        &format!("{label}-response"),
        &match &response {
            Ok(value) => value.clone(),
            Err(error) => json!({"error": error}),
        },
    );
    response
}

fn assert_support_shear(response: &Value, shear_n: f64) {
    let support = response["member_force_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[0] == 0 && row[1] == 0.0)
        .unwrap();
    assert!((support[2][1].as_f64().unwrap().abs() - shear_n).abs() < 1e-6);
}

fn assert_normal(response: &Value) {
    // 手入力100kN + 選択自重42kN + 先端地震10kNの釣合い。
    assert_support_shear(response, 152_000.0);
    assert_eq!(response["summary"]["n_checks"], 3);
    assert_eq!(response["summary"]["n_skipped"], 0);
    assert_eq!(response["summary"]["all_members_checked_and_ok"], true);
}

#[test]
fn public_job_rejects_both_load_directions_after_success_and_recovers_only_after_repair() {
    for (kind, distributed) in [("point", false), ("distributed", true)] {
        let mut model = fixture(distributed);
        let original = serde_json::to_vec(&model).unwrap();
        let mut normal = None;
        for (step, dir) in [[0.0, 0.0, -1.0], [0.0; 3], [0.0, 0.0, -7.0], [0.0; 3]]
            .into_iter()
            .enumerate()
        {
            model.load_cases[0].member[0].dir = dir;
            let response = design(&model, 0, &format!("{kind}-step{}", step + 1));
            if step % 2 == 0 {
                let response = response.unwrap();
                assert_normal(&response);
                if let Some(first) = &normal {
                    assert_eq!(&response, first);
                } else {
                    normal = Some(response);
                    let gravity = design(&model, 1, &format!("{kind}-gravity")).unwrap();
                    assert_support_shear(&gravity, 142_000.0);
                }
            } else {
                let error = response.unwrap_err();
                for expected in [
                    "荷重ケース 0 (DL)",
                    "member[0]",
                    "手入力100kN",
                    "部材 0",
                    "方向",
                    "1e-12",
                ] {
                    assert!(error.contains(expected), "{error}");
                }
            }
        }
        model.load_cases[0].member[0].dir = [0.0, 0.0, -1.0];
        assert_eq!(serde_json::to_vec(&model).unwrap(), original);
    }
}
