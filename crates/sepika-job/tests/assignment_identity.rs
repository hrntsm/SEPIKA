use sepika_core as core_fixture_api;
use sepika_job::prepare::prepare_model_for_analysis;
use sepika_job::settings::AnalysisSettings;
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

#[test]
fn 解析準備は孤立版と入力荷重を候補で診断しモデルを確定しない() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        if wall {
            model.unassigned_posts.push(fixture::divider(true));
        } else {
            model.unassigned_beams.push(fixture::divider(false));
        }
        let before = model.clone();
        let error = match prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
        {
            Ok(_) => panic!("孤立版は拒否"),
            Err(e) => e.to_string(),
        };
        assert!(error.contains("孤立版"));
        assert!(error.contains("0.0025"));
        assert!(error.contains("未更新"));
        fixture::assert_inputs_eq(&model, &before);
    }
}
