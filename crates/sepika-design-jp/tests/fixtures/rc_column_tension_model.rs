use sepika_core::{model::Model, section_shape::SectionShape};

pub fn tension_model(mut model: Model, tension_n: f64, moments_nmm: [f64; 2]) -> Model {
    if let Some(SectionShape::RcColumnRect { rebar, .. }) = &model.sections[0].shape {
        let shape = SectionShape::RcColumnRect {
            b: 500.0,
            d: 500.0,
            rebar: rebar.clone(),
        };
        let old = &model.sections[0];
        let mut section = shape.to_section(old.id, old.name.clone());
        section.material = old.material;
        section.rebar_material = old.rebar_material;
        section.shear_rebar_material = old.shear_rebar_material;
        section.frame_use = old.frame_use;
        model.sections[0] = section;
    }
    model.load_cases[0].nodal[0].values =
        [0.0, 0.0, tension_n, moments_nmm[0], moments_nmm[1], 0.0];
    model.validate().unwrap();
    model
}
