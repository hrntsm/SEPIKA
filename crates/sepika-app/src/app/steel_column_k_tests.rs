use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{ElementKind, Model, PanelZoneMode, Story};
use sepika_design_jp::CheckOutcome;
use sepika_edit::{AddDamper, AddMember, EditCommand};
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

#[test]
fn steel_design_attr_without_a_member_is_rejected_before_addition_and_check() {
    let mut model = input::portal(1.0);
    let members = std::mem::take(&mut model.elements);
    model.steel_design_attrs.truncate(1);
    model.steel_design_attrs[0].lk_y_direct = Some(1000.0);
    let diagnosis = "鉄骨部材の検定属性 -> 部材 0";

    let mut app = App::default();
    assert!(!app.load_model(model.clone()));
    assert!(app
        .core
        .scoped
        .last_error
        .as_deref()
        .unwrap()
        .contains(diagnosis));
    let error = sepika_design_jp::run_member_design_checks(&model, &[], &[], &Default::default())
        .err()
        .unwrap();
    assert!(error.contains(diagnosis), "{error}");

    let mut damper = members[0].clone();
    damper.kind = ElementKind::Damper;
    damper.section = None;
    let attempts: [Box<dyn EditCommand>; 2] = [
        Box::new(AddMember {
            elem: members[0].clone(),
        }),
        Box::new(AddDamper {
            elem: damper,
            props: Default::default(),
        }),
    ];
    let before = model.clone();
    for attempt in attempts {
        let rejected = attempt.apply(&mut model);
        assert!(rejected.rejection().unwrap().contains(diagnosis));
        assert!(model.eq_ignoring_dofmap(&before));
    }

    model.steel_design_attrs.clear();
    for elem in members {
        let inverse = AddMember { elem }.apply(&mut model);
        assert!(!inverse.is_noop(), "{:?}", inverse.rejection());
    }
    assert!(app.load_model(model));
    assert_steel_column_lengths(&mut app, &[(ElemId(0), "強軸 3600 ／ 弱軸 3600")]);
}

#[test]
#[cfg(feature = "gui")]
fn steel_design_attrs_stay_with_their_members_across_load_delete_undo_and_redo() {
    let mut model = input::portal(1.0);
    model.panel_zone = PanelZoneMode::None;
    model.steel_design_attrs[0].lk_y_direct = Some(1000.0);
    let mut duplicate = model.steel_design_attrs[0].clone();
    duplicate.lk_y_direct = Some(1300.0);
    model.steel_design_attrs.push(duplicate);
    let original_attrs = model.steel_design_attrs.clone();
    let mut replacement = model.elements[0].clone();
    replacement.id = ElemId(2);
    let path = std::env::temp_dir().join(format!("sepika1751-attrs-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let mut app = App::default();
    app.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(app.core.model.steel_design_attrs, original_attrs);
    assert_steel_column_lengths(&mut app, &[(ElemId(0), "強軸 1000 ／ 弱軸 3000")]);
    assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteMember { id: ElemId(0) })));
    let mut shifted_attr = original_attrs[1].clone();
    shifted_attr.elem = ElemId(0);
    assert_eq!(app.core.model.steel_design_attrs, [shifted_attr.clone()]);

    app.undo_action();
    assert_eq!(app.core.model.steel_design_attrs, original_attrs);
    assert_steel_column_lengths(&mut app, &[(ElemId(0), "強軸 1000 ／ 弱軸 3000")]);

    app.redo_action();
    assert_eq!(app.core.model.steel_design_attrs, [shifted_attr]);
    assert!(app.apply_model_edit(Box::new(AddMember { elem: replacement })));
    assert_steel_column_lengths(
        &mut app,
        &[
            (ElemId(0), "強軸 3600 ／ 弱軸 3000"),
            (ElemId(2), "強軸 3600 ／ 弱軸 3600"),
        ],
    );
}

fn assert_steel_column_lengths(app: &mut App, expected: &[(ElemId, &str)]) {
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.current_design_results().unwrap();
    for &(id, lengths) in expected {
        let column = results
            .member_checks
            .iter()
            .find(|member| member.elem == id)
            .unwrap();
        assert_eq!(column.positions.len(), 3);
        for position in &column.positions {
            let CheckOutcome::Checked(result) = &position.outcome else {
                panic!("{:?}", position.outcome);
            };
            assert!(result.detail.contains(lengths), "{:?}", result.detail);
        }
    }
}
