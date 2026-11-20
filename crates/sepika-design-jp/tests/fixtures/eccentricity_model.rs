use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase, LoadCaseKind,
    LocalAxis, Material, MaterialCategory, MemberLoad, Model, NodalLoad, Node, RigidZone, Section,
    Story,
};
use sepika_element::factory::build_behavior;
use sepika_solver::statics::linear::StaticOnce;
use smallvec::smallvec;

pub const TOP: StoryId = StoryId(1);

pub fn column_model(heads_mm: &[[f64; 2]], first_dx_mm: f64, first_dy_mm: f64) -> Model {
    let mut model = Model {
        stories: vec![
            Story::new(StoryId(0), "基礎".into(), Some(0.0), Some(0.0)),
            Story::new(TOP, "上階".into(), Some(3000.0), Some(0.0)),
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "鋼".into(),
            category: MaterialCategory::Steel,
            young: 200_000.0,
            poisson: 0.25,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        ..Default::default()
    };
    let mut section = Section::zero(SectionId(0), "柱断面".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.area = 2_000.0;
    section.as_y = 1_600.0;
    section.as_z = 1_600.0;
    section.iy = 8_000_000.0;
    section.iz = 8_000_000.0;
    section.j = 4_000_000.0;
    model.sections.push(section);
    for (index, &[x, y]) in heads_mm.iter().enumerate() {
        let bottom = NodeId((2 * index) as u32);
        let top = NodeId(bottom.0 + 1);
        let base_xy = if index == 0 {
            [x - first_dx_mm, y - first_dy_mm]
        } else {
            [x, y]
        };
        for (id, coord, restraint, story) in [
            (
                bottom,
                [base_xy[0], base_xy[1], 0.0],
                Dof6Mask::FIXED,
                StoryId(0),
            ),
            (top, [x, y, 3000.0], Dof6Mask::FREE, TOP),
        ] {
            model.nodes.push(Node {
                id,
                coord,
                restraint,
                mass: None,
                story: Some(story),
                support_spring: None,
            });
            model.stories[story.index()].node_ids.push(id);
        }
        model.elements.push(ElementData {
            id: ElemId(index as u32),
            kind: ElementKind::Beam,
            nodes: smallvec![bottom, top],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    model
}

pub fn add_case(model: &mut Model, name: &str, values: [f64; 6]) -> LoadCaseId {
    let id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id,
        name: name.into(),
        kind: LoadCaseKind::Seismic,
        nodal: vec![NodalLoad::manual(NodeId(1), values)],
        member: Vec::new(),
    });
    id
}

pub fn element_nodal_residual(
    model: &Model,
    element: &ElementData,
    result: &StaticOnce,
    loads: &[MemberLoad],
) -> [f64; 12] {
    let mut disp = [0.0; 12];
    for (dofs, node) in disp.as_chunks_mut::<6>().0.iter_mut().zip(&element.nodes) {
        dofs.copy_from_slice(&result.disp[node.index()]);
    }
    build_behavior(element, model)
        .frame_load_response(&disp, loads)
        .unwrap()
        .unwrap()
        .nodal_residual
        .data
        .as_slice()
        .try_into()
        .unwrap()
}
