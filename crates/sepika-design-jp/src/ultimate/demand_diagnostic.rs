use sepika_core::ids::ElemId;
use sepika_core::model::Model;

pub(super) fn demand_diagnostic(model: &Model, member: ElemId, reason: String) -> String {
    let format_terms = |terms: &[(sepika_core::ids::LoadCaseId, f64)]| {
        terms
            .iter()
            .map(|(case, coefficient)| format!("case={} coefficient={coefficient}", case.0))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let context = match sepika_core::load_combo::ultimate_gravity_terms(model) {
        Ok(terms) => format!("gravity_terms=[{}]", format_terms(&terms)),
        Err(resolution) => {
            let selected = model
                .ultimate_gravity_combination
                .and_then(|index| model.combinations.get(index))
                .map(|combination| format!("[{}]", format_terms(&combination.terms)))
                .unwrap_or_else(|| "unresolved".into());
            format!("gravity_terms=unresolved selected_terms={selected} gravity_resolution={resolution}")
        }
    };
    format!("{reason}; member={} purpose=frame_long {context}", member.0)
}
