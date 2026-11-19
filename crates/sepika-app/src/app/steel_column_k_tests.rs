use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{NodeId, StoryId};
use sepika_core::model::{Model, PanelZoneMode, Story};
use sepika_design_jp::CheckOutcome;
#[path = "../../../sepika-design-jp/tests/fixtures/steel_column_k_model.rs"]
pub(crate) mod input;

pub(crate) fn asymmetric_built_h_cantilever() -> Model {
    let mut model = input::portal(1.0);
    model.panel_zone = PanelZoneMode::None;
    model.elements.truncate(1);
    model.nodes.remove(1);
    model.nodes.truncate(2);
    model.nodes[0].restraint = Dof6Mask::FIXED;
    model.nodes[1].id = NodeId(1);
    model.nodes[1].restraint = Dof6Mask::FREE;
    model.elements[0].nodes[1] = NodeId(1);
    model.load_cases[0].nodal.truncate(1);
    model.load_cases[0].nodal[0].node = NodeId(1);
    model.load_cases[0].nodal[0].values[2] = -10_000.0;
    model.steel_design_attrs.truncate(1);
    model.stories = [0.0, 3000.0]
        .into_iter()
        .enumerate()
        .map(|(i, fl)| {
            Story::new(
                StoryId(i as u32),
                format!("{}F", i + 1),
                Some(fl),
                Some(0.0),
            )
        })
        .collect();
    let former = &model.sections[0];
    let mut section = sepika_core::section_shape::SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 80.0,
        upper_thick: 12.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 8.0,
    }
    .to_section(former.id, "上下非対称組立H".into());
    section.material = former.material;
    section.frame_use = former.frame_use;
    model.sections[0] = section;
    model.sections.truncate(1);
    model
}

#[test]
fn steel_column_gui_keeps_resolved_lengths_and_invalidates_old_design_contract() {
    let mut app = App::default();
    let mut model = input::portal(1.0);
    model.steel_design_attrs[1].lk_z_direct = None;
    assert!(app.load_model(model));
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let expected = app.current_design_results().unwrap().member_checks.clone();
    let columns: Vec<_> = expected.iter().filter(|member| member.elem.0 < 2).collect();
    assert_eq!(columns.len(), 2);
    for (column, lengths) in columns
        .into_iter()
        .zip(["強軸 3600 ／ 弱軸 3000", "強軸 3600 ／ 弱軸 3600"])
    {
        assert_eq!(column.positions.len(), 3);
        assert!(column.positions.iter().all(|position| matches!(&position.outcome, CheckOutcome::Checked(result) if result.detail.contains(lengths))));
    }
    for version in [60, sepika_core::result_contract::DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-buckling-length-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        if version == 60 {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
        }
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}
