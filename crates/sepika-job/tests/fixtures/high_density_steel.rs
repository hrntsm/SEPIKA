#[allow(dead_code)]
mod seismic {
    include!("seismic_freshness.rs");
}

pub fn steel_frame() -> sepika_core::model::Model {
    use sepika_core::model::{MaterialCategory, PropertyBasis};
    let mut model = seismic::two_storeys_with_weightless_steel_girders();
    model.sections[1].material = Some(sepika_core::ids::MaterialId(0));
    model.sections[1].area = 120000.0;
    model.materials.truncate(3);
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.materials[0].category = MaterialCategory::Steel;
    model.materials[0].name = "SN400B".into();
    model.materials[0].density = 7.85e-9;
    model.materials[0].young = 205000.0;
    model.materials[0].fc = None;
    model.materials[0].fy = Some(235.0);
    for section in &mut model.sections[..2] {
        section.shape = None;
        section.property_basis.j = PropertyBasis::Supplied;
        section.rebar_material = None;
        section.shear_rebar_material = None;
    }
    model
}
