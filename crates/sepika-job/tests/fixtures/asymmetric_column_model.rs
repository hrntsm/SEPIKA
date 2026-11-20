use sepika_core::{model::Model, section_shape::SectionShape};

#[path = "../../../sepika-design-jp/tests/fixtures/steel_h_column_combined_model.rs"]
mod steel_column;

pub fn column_model() -> Model {
    let mut model = steel_column::cantilever(0.0, 0.0, 0.0);
    let old = &model.sections[0];
    let mut section = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 80.0,
        upper_thick: 12.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 8.0,
    }
    .to_section(old.id, "上下非対称組立H".into());
    section.material = old.material;
    section.frame_use = old.frame_use;
    model.sections[0] = section;
    model
}
