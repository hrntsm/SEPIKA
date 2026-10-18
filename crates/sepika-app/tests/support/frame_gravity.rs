use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, Model};

pub fn select_frame_gravity(model: &mut Model) {
    assert!(model
        .load_cases
        .iter()
        .any(|case| case.kind == LoadCaseKind::Dead));
    assert!(!model
        .load_cases
        .iter()
        .any(|case| matches!(case.kind, LoadCaseKind::Snow | LoadCaseKind::Other)));
    if !model
        .load_cases
        .iter()
        .any(|case| case.kind == LoadCaseKind::Live)
    {
        let id = LoadCaseId(model.load_cases.iter().map(|case| case.id.0).max().unwrap() + 1);
        model.load_cases.push(LoadCase {
            id,
            name: "無載荷の架構用P".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
    }
    let mut terms: Vec<_> = model
        .load_cases
        .iter()
        .filter(|case| matches!(case.kind, LoadCaseKind::Dead | LoadCaseKind::Live))
        .map(|case| (case.id, 1.0))
        .collect();
    terms.sort_by_key(|(id, _)| *id);
    let index = model
        .combinations
        .iter()
        .position(|combination| {
            let mut saved = combination.terms.clone();
            saved.sort_by_key(|(id, _)| *id);
            saved == terms
        })
        .unwrap_or_else(|| {
            model.combinations.push(LoadCombination {
                name: "保存長期架構G+P".into(),
                terms,
            });
            model.combinations.len() - 1
        });
    model.ultimate_gravity_combination = Some(index);
    sepika_core::load_combo::ultimate_gravity_terms(model).expect("架構用長期重力の入力契約");
}
