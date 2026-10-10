#[allow(dead_code)]
mod frame {
    include!("high_density_steel.rs");
}

pub fn one_cubic_metre(attached: bool) -> sepika_core::model::Model {
    use sepika_core::{ids::*, model::*};
    let mut model = frame::steel_frame();
    model.nodes.truncate(4);
    model.elements.retain(|e| e.nodes.iter().all(|n| n.0 < 4));
    for (index, element) in model.elements.iter_mut().enumerate() {
        element.id = ElemId(index as u32);
    }
    model.materials[0].density = 0.0;
    model.materials[1].category = MaterialCategory::Steel;
    model.materials[1].density = 7.85e-9;
    model.materials[1].fc = None;
    model.sections[2].shape = None;
    for node in &mut model.nodes {
        if node.coord[0] != 0.0 {
            node.coord[0] = 5000.0;
        }
    }
    model.load_cases.clear();
    model.slabs.truncate(1);
    if !attached {
        let mut boundary = vec![NodeId(2), NodeId(3)];
        for coord in [[5000.0, 2000.0, 12500.0], [0.0, 2000.0, 12500.0]] {
            let id = NodeId(model.nodes.len() as u32);
            model.nodes.push(Node {
                id,
                coord,
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
            boundary.push(id);
        }
        let plate = model.slabs.remove(0).plate;
        model.add_enclosed_slab_from_nodes(&boundary, plate);
        for element in &mut model.elements {
            if element.section.is_none() {
                element.section = Some(SectionId(1));
            }
        }
    }
    model
}
