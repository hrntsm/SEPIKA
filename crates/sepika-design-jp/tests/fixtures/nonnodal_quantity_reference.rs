use sepika_core::{ids::SecondaryMemberId, model::Model};

pub fn assert_quantity(model: &Model, id: SecondaryMemberId, volume_m3: f64, formwork_m2: f64) {
    let q = sepika_design_jp::quantity::try_compute_quantity_takeoff(model, &model.quantity_cfg)
        .unwrap();
    let rows: Vec<_> = q
        .items
        .iter()
        .filter(|item| item.secondary == Some(id))
        .collect();
    assert_eq!(rows.len(), 1);
    let item = rows[0];
    assert_eq!(item.elem, None);
    assert_eq!(item.slab, None);
    assert_eq!(item.story, "1F");
    assert!(
        (item.concrete_m3 - volume_m3).abs() < 1e-12,
        "{}",
        item.concrete_m3
    );
    assert!(
        (item.formwork_m2 - formwork_m2).abs() < 1e-12,
        "{}",
        item.formwork_m2
    );
}
