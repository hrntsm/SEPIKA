use sepika_core::ids::{MaterialId, NodeId, SectionId};
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::section_shape::SectionShape;

pub fn cantilever(mut model: Model, ref_vector: [f64; 3]) -> Model {
    model.elements.truncate(1);
    model.sections.truncate(1);
    model.stories.clear();
    model.load_cases.clear();
    model.combinations.clear();
    model.nodes.remove(1);
    model.nodes.pop();
    model.nodes[1].id = NodeId(1);
    model.nodes[1].coord[2] = 3000.0;
    let element = &mut model.elements[0];
    element.nodes[1] = NodeId(1);
    element.local_axis.ref_vector = ref_vector;
    let mut section = SectionShape::SteelH {
        height: 600.0,
        width: 300.0,
        web_thick: 12.0,
        flange_thick: 20.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "H-600x300x12x20".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    model.sections[0] = section;
    model
}
